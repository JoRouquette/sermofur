//! `smf mcp serve` (`McpBridge.RunAsync` on the `StdioServerTransport` of the SDK): the MCP
//! server on stdio. It answers `initialize`, `ping`, `logging/setLevel` and `tools/list` without
//! a daemon; every `tools/call` goes to the daemon through the channel. Nothing else is ever
//! written to stdout.
//!
//! Lines are read as `StreamReader.ReadLine` splits them (`\n`, `\r\n` or a lone `\r`), UTF-8
//! with replacement, a byte order mark skipped at the start; blank lines are skipped. A request
//! is answered on its own: calls run side by side (one command in flight at a time on the
//! channel), so answers may come in another order than the requests, as with the SDK. The end
//! of stdin, or the stop of the process, ends the server: calls still running get no answer.

use std::collections::HashMap;
use std::future::Future;
use std::io::{BufRead, BufReader, Read, Write};
use std::pin::pin;
use std::rc::Rc;

use sermofur_core::json::quoted_basic_latin;
use sermofur_core::stj::Node;
use sermofur_daemon::log::DaemonLog;
use sermofur_daemon::server::first;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio::task::{AbortHandle, LocalSet};

use crate::tools::{self, CommandChannel};
use crate::wire::{self, Message, RequestId, params_fit, property, string};

/// `McpBridge.ServerName`.
pub const SERVER_NAME: &str = "sermofur";

/// `McpServerOptions.ServerInstructions`.
pub const INSTRUCTIONS: &str = "Sermofur keeps a contestable memory of this project: recall before deciding, challenge a claim before relying on it, and record what you learn as claims, evidence or RETEX. What you write is recorded as coming from an LLM.";

/// Protocol versions of the SDK 2.2.0 (`SupportedProtocolVersions`), the last one the latest.
pub const PROTOCOL_VERSIONS: [&str; 4] = ["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"];

const INTERNAL_ERROR: (i32, &str) = (-32603, "An error occurred.");
const PARSE_ERROR: (i32, &str) = (-32700, "Failed to parse the JSON-RPC request.");
const METHOD_NOT_FOUND: i32 = -32601;

enum Event {
    Line(String),
    End,
    /// The id, the generation of the call that answers, the line.
    Answer(RequestId, u64, String),
}

/// Serves until the end of `input` or until `stop` completes. A failure to write `out` (the
/// host went away) ends the server too.
pub fn serve<C: CommandChannel + 'static>(
    channel: C,
    version: &str,
    log: Option<DaemonLog>,
    input: impl Read + Send + 'static,
    out: &mut dyn Write,
    stop: impl Future<Output = ()>,
) -> std::io::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let (events, mut received) = unbounded_channel();
    read_lines(input, events.clone());
    if let Some(log) = &log {
        log.write("mcp_started", None, None, None);
    }
    let mut server = Server {
        channel: Rc::new(channel),
        version,
        log: log.clone(),
        client_name: None,
        running: HashMap::new(),
        calls: 0,
        events,
    };
    LocalSet::new().block_on(&runtime, async {
        let mut stop = pin!(stop);
        loop {
            let event = match first(received.recv(), stop.as_mut()).await {
                Ok(Some(event)) => event,
                Ok(None) | Err(()) => break,
            };
            let line = match event {
                Event::End => break,
                Event::Answer(id, call, line) => {
                    // A later call of the same id keeps its entry, so it can still be cancelled.
                    if server
                        .running
                        .get(&id)
                        .is_some_and(|(running, _)| *running == call)
                    {
                        server.running.remove(&id);
                    }
                    Some(line)
                }
                Event::Line(text) => server.handle(&text),
            };
            if let Some(line) = line
                && write_line(out, &line).is_err()
            {
                break;
            }
        }
        for (_, task) in server.running.values() {
            task.abort();
        }
    });
    if let Some(log) = &log {
        log.write("mcp_stopped", None, None, None);
        log.flush_before_exit();
    }
    // The reader may still wait on stdin: the runtime does not wait for it.
    runtime.shutdown_background();
    Ok(())
}

fn write_line(out: &mut dyn Write, line: &str) -> std::io::Result<()> {
    out.write_all(line.as_bytes())?;
    out.write_all(b"\n")?;
    out.flush()
}

/// The reader of stdin, on a thread of its own: each line, then the end.
fn read_lines(input: impl Read + Send + 'static, events: UnboundedSender<Event>) {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(input);
        let mut bytes = Vec::new();
        let mut start = true;
        loop {
            bytes.clear();
            match reader.read_until(b'\n', &mut bytes) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            if bytes.last() == Some(&b'\n') {
                bytes.pop();
            }
            let body = if start {
                start = false;
                bytes
                    .strip_prefix(&[0xEF, 0xBB, 0xBF][..])
                    .unwrap_or(&bytes)
            } else {
                &bytes[..]
            };
            let text = String::from_utf8_lossy(body);
            let text = text.strip_suffix('\r').unwrap_or(&text);
            for line in text.split('\r') {
                if events.send(Event::Line(line.to_owned())).is_err() {
                    return;
                }
            }
        }
        let _ = events.send(Event::End);
    });
}

struct Server<'a, C> {
    channel: Rc<C>,
    version: &'a str,
    log: Option<DaemonLog>,
    /// `ClientInfo.Name` of the last `initialize`.
    client_name: Option<String>,
    /// The running calls by id, the last one for an id reused, with its generation.
    running: HashMap<RequestId, (u64, AbortHandle)>,
    /// Calls spawned so far: the generation of the next one.
    calls: u64,
    events: UnboundedSender<Event>,
}

impl<C: CommandChannel + 'static> Server<'_, C> {
    /// The answer to one line, unless it has none or comes later (a call).
    fn handle(&mut self, text: &str) -> Option<String> {
        match wire::read(text)? {
            Message::Invalid(id) => id.map(|id| wire::error(&id, PARSE_ERROR.0, PARSE_ERROR.1)),
            Message::Response => None,
            Message::Notification { method, params } => {
                if method == "notifications/cancelled"
                    && params_fit(params.as_ref(), wire::CANCELLED)
                    && let Some(id) = wire::request_id(property(params.as_ref(), "requestId"))
                    && let Some((_, task)) = self.running.remove(&id)
                {
                    task.abort();
                }
                None
            }
            Message::Request { id, method, params } => self.request(id, &method, params),
        }
    }

    fn request(&mut self, id: RequestId, method: &str, params: Option<Node>) -> Option<String> {
        let params = params.as_ref();
        let internal = |id: &RequestId| Some(wire::error(id, INTERNAL_ERROR.0, INTERNAL_ERROR.1));
        match method {
            "initialize" => {
                if !params_fit(params, wire::INITIALIZE) {
                    return internal(&id);
                }
                let asked = string(property(params, "protocolVersion"));
                let version = asked
                    .as_deref()
                    .filter(|asked| PROTOCOL_VERSIONS.contains(asked))
                    .unwrap_or(PROTOCOL_VERSIONS[3]);
                self.client_name = string(property(property(params, "clientInfo"), "name"));
                Some(wire::result(
                    &id,
                    &format!(
                        "{{\"protocolVersion\":{},\"capabilities\":{{\"logging\":{{}},\"tools\":{{}}}},\"serverInfo\":{{\"name\":{},\"version\":{}}},\"instructions\":{}}}",
                        quoted_basic_latin(version),
                        quoted_basic_latin(SERVER_NAME),
                        quoted_basic_latin(self.version),
                        quoted_basic_latin(INSTRUCTIONS)
                    ),
                ))
            }
            "ping" => Some(wire::result(&id, "{}")),
            "logging/setLevel" if params_fit(params, wire::SET_LEVEL) => {
                Some(wire::result(&id, "{}"))
            }
            "tools/list" if params_fit(params, wire::LIST_TOOLS) => {
                Some(wire::result(&id, &tools::list()))
            }
            "tools/call" if params_fit(params, wire::CALL_TOOL) => {
                self.spawn_call(id, params);
                None
            }
            "logging/setLevel" | "tools/list" | "tools/call" => internal(&id),
            _ => Some(wire::error(
                &id,
                METHOD_NOT_FOUND,
                &format!("Method '{method}' is not available."),
            )),
        }
    }

    /// Runs a call beside the reading of stdin; its answer comes back as an event.
    fn spawn_call(&mut self, id: RequestId, params: Option<&Node>) {
        let name = string(property(params, "name")).unwrap_or_default();
        let arguments = property(params, "arguments").cloned();
        let actor = tools::actor(self.client_name.as_deref());
        let (channel, log, events) = (
            Rc::clone(&self.channel),
            self.log.clone(),
            self.events.clone(),
        );
        let answered = id.clone();
        let call = self.calls;
        self.calls += 1;
        let task = tokio::task::spawn_local(async move {
            let line = match tools::call(&name, arguments.as_ref(), &actor, &*channel, log.as_ref())
                .await
            {
                Some(result) => wire::result(&answered, &result),
                None => wire::error(&answered, INTERNAL_ERROR.0, INTERNAL_ERROR.1),
            };
            let _ = events.send(Event::Answer(answered, call, line));
        });
        self.running.insert(id, (call, task.abort_handle()));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, mpsc};
    use std::time::Duration;

    use sermofur_core::error::Result;
    use sermofur_daemon::server::CommandOutcome;
    use tokio::sync::Notify;

    use super::*;

    /// Call 0 and call 1 wait for their gate; call 1 signals `done` when it ends or is dropped;
    /// later calls wait for that signal, so a call 1 still alive answers before them.
    #[derive(Clone, Default)]
    struct Gates {
        calls: Arc<AtomicUsize>,
        gates: [Arc<Notify>; 2],
        done: Arc<Notify>,
    }

    struct Signal(Arc<Notify>);

    impl Drop for Signal {
        fn drop(&mut self) {
            self.0.notify_one();
        }
    }

    impl CommandChannel for Gates {
        async fn run(&self, _argv: Vec<String>) -> Result<CommandOutcome> {
            match self.calls.fetch_add(1, Ordering::SeqCst) {
                0 => self.gates[0].notified().await,
                1 => {
                    let _signal = Signal(Arc::clone(&self.done));
                    self.gates[1].notified().await;
                }
                _ => self.done.notified().await,
            }
            Ok(CommandOutcome {
                exit_code: 0,
                stdout: "{}".to_owned(),
                stderr: String::new(),
            })
        }
    }

    struct Lines(mpsc::Sender<String>);

    impl Write for Lines {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let text = String::from_utf8_lossy(bytes);
            for line in text.split_terminator('\n').filter(|line| !line.is_empty()) {
                let _ = self.0.send(line.to_owned());
            }
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn call(id: u32) -> String {
        format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"tools/call\",\"params\":{{\"name\":\"sermofur_status\"}}}}\n"
        )
    }

    /// Two calls of the same id: the answer of the first leaves the second cancellable.
    #[test]
    fn the_answer_of_a_call_leaves_a_later_call_of_its_id_cancellable() {
        let channel = Gates::default();
        let (input, mut host) = std::io::pipe().expect("a pipe");
        let (sender, answers) = mpsc::channel();
        let served = channel.clone();
        let server = std::thread::spawn(move || {
            serve(
                served,
                "0.0.0",
                None,
                input,
                &mut Lines(sender),
                std::future::pending(),
            )
            .expect("served");
        });
        let next = || {
            answers
                .recv_timeout(Duration::from_secs(10))
                .expect("an answer")
        };
        host.write_all(format!("{}{}", call(1), call(1)).as_bytes())
            .expect("sent");
        for _ in 0..1000 {
            if channel.calls.load(Ordering::SeqCst) == 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        channel.gates[0].notify_one();
        assert!(next().ends_with(r#""id":1,"jsonrpc":"2.0"}"#));
        host.write_all(
            concat!(
                r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}"#,
                "\n",
                r#"{"jsonrpc":"2.0","id":9,"method":"ping"}"#,
                "\n"
            )
            .as_bytes(),
        )
        .expect("sent");
        assert_eq!(next(), r#"{"result":{},"id":9,"jsonrpc":"2.0"}"#);
        channel.gates[1].notify_one();
        host.write_all(call(2).as_bytes()).expect("sent");
        let answer = next();
        assert!(answer.ends_with(r#""id":2,"jsonrpc":"2.0"}"#), "{answer}");
        drop(host);
        server.join().expect("stopped");
    }
}
