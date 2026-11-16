//! Port of `tests/Sermofur.Tests/FramingTests.cs`, one test per .NET case, plus the properties
//! of the decoder on arbitrary bytes.

use std::io::Cursor;

use proptest::prelude::*;
use sermofur_daemon::framing::{
    self, FrameError, IpcMessage, MAX_BODY_BYTES, MAX_RESPONSE_BYTES, kind,
};

fn read(frame: &[u8]) -> Result<Option<IpcMessage>, FrameError> {
    framing::read(&mut Cursor::new(frame), MAX_BODY_BYTES)
}

fn refusal(result: Result<Option<IpcMessage>, FrameError>) -> (String, i32, String) {
    match result {
        Err(FrameError::Refused(e)) => (e.code, e.exit_code, e.message),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn message_survives_a_round_trip() {
    let sent = IpcMessage {
        id: Some(7),
        argv: Some(
            ["claim", "add", "é -- \"quoted\"", ""]
                .map(|a| Some(a.to_owned()))
                .to_vec(),
        ),
        cwd: Some(std::env::temp_dir().display().to_string()),
        ..IpcMessage::new(kind::RUN)
    };
    let mut stream = Cursor::new(Vec::new());
    framing::write(&mut stream, &sent, MAX_BODY_BYTES).expect("writes");
    stream.set_position(0);
    let received = framing::read(&mut stream, MAX_BODY_BYTES)
        .expect("reads")
        .expect("a message");
    assert_eq!(received, sent);
    assert!(matches!(
        framing::read(&mut stream, MAX_BODY_BYTES),
        Ok(None)
    ));
}

#[test]
fn null_fields_are_not_written() {
    let frame = framing::encode(&IpcMessage::new(kind::STATUS), MAX_BODY_BYTES).unwrap();
    assert_eq!(&frame[4..], b"{\"kind\":\"status\"}");
}

#[test]
fn body_at_the_limit_is_accepted_and_one_more_byte_is_refused() {
    let padding = "a".repeat(MAX_BODY_BYTES - "{\"kind\":\"\"}".len());
    let at_limit = framing::encode(&IpcMessage::new(&padding), MAX_BODY_BYTES).unwrap();
    assert_eq!(at_limit.len(), MAX_BODY_BYTES + 4);
    assert_eq!(read(&at_limit).unwrap().unwrap().kind, padding);
    let refused = framing::encode(&IpcMessage::new(&(padding + "a")), MAX_BODY_BYTES)
        .expect_err("one byte over the limit");
    assert_eq!(
        (refused.code.as_str(), refused.exit_code),
        ("request_too_large", 1)
    );
}

#[test]
fn oversized_header_is_refused_without_reading_the_body() {
    let header = u32::try_from(MAX_BODY_BYTES + 1).unwrap().to_le_bytes();
    assert_eq!(refusal(read(&header)).0, "request_too_large");
}

#[test]
fn broken_frames_are_protocol_errors() {
    for (frame, message) in [
        (&[0u8, 0, 0, 0][..], "Empty frame."),
        (&[5, 0][..], "Truncated frame header."),
        (&[5, 0, 0, 0, b'{'][..], "Truncated frame body."),
    ] {
        assert_eq!(
            refusal(read(frame)),
            ("protocol_error".to_owned(), 3, message.to_owned())
        );
    }
}

#[test]
fn malformed_bodies_are_protocol_errors() {
    for body in ["not json", "{}", "{\"kind\":\"\"}", "{\"kind\":3}", "[]"] {
        let error = framing::decode(body.as_bytes()).expect_err(body);
        assert_eq!(error.code, "protocol_error", "{body}");
    }
}

#[test]
fn invalid_utf8_is_not_repaired() {
    let body = [&b"{\"kind\":\""[..], &[0xC3, 0x28], b"\"}"].concat();
    assert_eq!(framing::decode(&body).unwrap_err().code, "protocol_error");
}

#[test]
fn unknown_fields_are_ignored() {
    let message = framing::decode(b"{\"kind\":\"status\",\"future\":{\"x\":1}}").unwrap();
    assert_eq!(message.kind, kind::STATUS);
}

#[test]
fn a_response_limit_reads_what_a_request_limit_refuses() {
    let frame = framing::encode(
        &IpcMessage {
            stdout: Some("r".repeat(MAX_BODY_BYTES)),
            ..IpcMessage::new(kind::RESULT)
        },
        MAX_RESPONSE_BYTES,
    )
    .unwrap();
    assert_eq!(refusal(read(&frame)).0, "request_too_large");
    let back = framing::read(&mut Cursor::new(&frame), MAX_RESPONSE_BYTES).unwrap();
    assert_eq!(back.unwrap().stdout.unwrap().len(), MAX_BODY_BYTES);
}

#[test]
fn a_failing_stream_is_an_io_error() {
    struct Broken;
    impl std::io::Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("broken"))
        }
    }
    assert!(matches!(
        framing::read(&mut Broken, MAX_BODY_BYTES),
        Err(FrameError::Io(_))
    ));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// `ArgumentsRoundTrip`: any argument vector within the limit comes back unchanged (a Rust
    /// string cannot hold a lone surrogate, the case the .NET property leaves out).
    #[test]
    fn arguments_round_trip(argv in prop::collection::vec(any::<String>(), 0..8)) {
        let message = IpcMessage {
            argv: Some(argv.iter().cloned().map(Some).collect()),
            ..IpcMessage::new(kind::RUN)
        };
        match framing::encode(&message, MAX_BODY_BYTES) {
            Ok(frame) => prop_assert_eq!(framing::decode(&frame[4..]).unwrap(), message),
            Err(e) => prop_assert_eq!(e.code, "request_too_large"),
        }
    }

    /// Every field, any value: what is written reads back the same.
    #[test]
    fn messages_round_trip(
        kind in ".{1,20}",
        text in proptest::option::of(any::<String>()),
        number in proptest::option::of(any::<i32>()),
        id in proptest::option::of(any::<i64>()),
    ) {
        let message = IpcMessage {
            kind,
            protocol: number,
            tool_version: text.clone(),
            cwd: text.clone(),
            pid: number,
            id,
            argv: text.clone().map(|t| vec![Some(t), None]),
            exit_code: number,
            stdout: text.clone(),
            stderr: text.clone(),
            code: text.clone(),
            message: text.clone(),
            started_at: text.clone(),
            instances_open: number,
            clients: number,
            registry: text,
        };
        let frame = framing::encode(&message, MAX_RESPONSE_BYTES).unwrap();
        // An independent reader sees the same strings as the input.
        let json: serde_json::Value = serde_json::from_slice(&frame[4..]).unwrap();
        let strings = [
            ("kind", Some(&message.kind)),
            ("toolVersion", message.tool_version.as_ref()),
            ("cwd", message.cwd.as_ref()),
            ("stdout", message.stdout.as_ref()),
            ("stderr", message.stderr.as_ref()),
            ("code", message.code.as_ref()),
            ("message", message.message.as_ref()),
            ("startedAt", message.started_at.as_ref()),
            ("registry", message.registry.as_ref()),
        ];
        for (name, value) in strings {
            prop_assert_eq!(json.get(name).and_then(|v| v.as_str()), value.map(String::as_str), "{}", name);
        }
        let argv: Option<Vec<Option<&str>>> = json
            .get("argv")
            .map(|a| a.as_array().unwrap().iter().map(|v| v.as_str()).collect());
        let expected: Option<Vec<Option<&str>>> = message
            .argv
            .as_ref()
            .map(|a| a.iter().map(Option::as_deref).collect());
        prop_assert_eq!(argv, expected);
        prop_assert_eq!(read_any(&frame), Some(message));
    }

    /// A frame read in pieces of any size, with `Interrupted` at any point, gives the same
    /// message and consumes the same bytes as when it is read at once.
    #[test]
    fn partial_reads_give_the_same_message(
        kind in ".{1,20}",
        stdout in proptest::option::of(any::<String>()),
        tail in prop::collection::vec(any::<u8>(), 0..8),
        chunks in prop::collection::vec(1usize..8, 0..64),
        interrupts in prop::collection::vec(any::<bool>(), 0..64),
    ) {
        let message = IpcMessage { stdout, ..IpcMessage::new(&kind) };
        let mut input = framing::encode(&message, MAX_RESPONSE_BYTES).unwrap();
        input.extend(&tail);
        let mut whole = Cursor::new(input.clone());
        let expected = framing::read(&mut whole, MAX_RESPONSE_BYTES).unwrap();
        let mut pieces = Pieces { data: Cursor::new(input), chunks, interrupts, step: 0 };
        let got = framing::read(&mut pieces, MAX_RESPONSE_BYTES).unwrap();
        prop_assert_eq!(&got, &expected);
        prop_assert_eq!(got, Some(message));
        prop_assert_eq!(pieces.data.position(), whole.position());
    }
}

/// A stream that serves its bytes in the given chunk sizes (then 1 byte at a time), and fails
/// with `Interrupted` before a chunk when the matching flag is set.
struct Pieces {
    data: Cursor<Vec<u8>>,
    chunks: Vec<usize>,
    interrupts: Vec<bool>,
    step: usize,
}

impl std::io::Read for Pieces {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let step = self.step;
        self.step += 1;
        if self.interrupts.get(step).copied().unwrap_or(false) {
            self.interrupts[step] = false;
            self.step = step;
            return Err(std::io::ErrorKind::Interrupted.into());
        }
        let size = self
            .chunks
            .get(step)
            .copied()
            .unwrap_or(1)
            .min(buffer.len());
        std::io::Read::read(&mut self.data, &mut buffer[..size])
    }
}

fn read_any(frame: &[u8]) -> Option<IpcMessage> {
    framing::read(&mut Cursor::new(frame), MAX_RESPONSE_BYTES).unwrap()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// `ArbitraryBytesAreAMessageOrARefusal`: arbitrary bytes never escape as anything but a
    /// message or a known refusal, and never panic.
    #[test]
    fn arbitrary_bytes_are_a_message_or_a_refusal(bytes in prop::collection::vec(any::<u8>(), 0..64)) {
        match read(&bytes) {
            Ok(_) => {}
            Err(FrameError::Refused(e)) => {
                prop_assert!(e.code == "protocol_error" || e.code == "request_too_large");
            }
            Err(FrameError::Io(e)) => prop_assert!(false, "unexpected I/O error {e}"),
        }
    }

    /// The same for an arbitrary body behind a valid header, so that the JSON reader is reached.
    #[test]
    fn arbitrary_bodies_are_a_message_or_a_protocol_error(
        body in prop_oneof![
            prop::collection::vec(any::<u8>(), 1..256),
            r#"[{}\[\]":, a-z0-9\\.-]{1,64}"#.prop_map(String::into_bytes),
        ]
    ) {
        if let Err(e) = framing::decode(&body) {
            prop_assert_eq!(e.code, "protocol_error");
        }
    }

    /// The same through `read`, behind a header that may be off by up to two bytes and before
    /// arbitrary trailing bytes: a message consumes exactly its header and its body. Half of the
    /// bodies are a valid message (`framing::encode` of an arbitrary kind), so that the
    /// consumption check runs on a known share of cases (every exact header among them).
    #[test]
    fn framed_arbitrary_bodies_are_a_message_or_a_refusal(
        body in prop_oneof![
            prop::collection::vec(any::<u8>(), 1..256),
            ".{1,32}".prop_map(|kind| {
                framing::encode(&IpcMessage::new(&kind), MAX_BODY_BYTES).expect("a valid message")
                    [4..]
                    .to_vec()
            }),
        ],
        delta in -2i64..=2,
        tail in prop::collection::vec(any::<u8>(), 0..8),
    ) {
        let length = u32::try_from((i64::try_from(body.len()).unwrap() + delta).max(0)).unwrap();
        let mut stream = Cursor::new([&length.to_le_bytes()[..], &body, &tail].concat());
        match framing::read(&mut stream, MAX_BODY_BYTES) {
            Ok(Some(_)) => prop_assert_eq!(stream.position(), 4 + u64::from(length)),
            Ok(None) => prop_assert!(false, "a header was there"),
            Err(FrameError::Refused(e)) => {
                prop_assert!(e.code == "protocol_error" || e.code == "request_too_large");
            }
            Err(FrameError::Io(e)) => prop_assert!(false, "unexpected I/O error {e}"),
        }
    }
}
