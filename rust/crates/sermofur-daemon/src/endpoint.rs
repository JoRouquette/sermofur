//! Local endpoint of the daemon, as `IpcEndpoint` of the .NET oracle (research R3): a named pipe
//! on Windows, a Unix domain socket `0600` in a folder `0700` elsewhere. No network port.
//!
//! Under Windows the pipe keeps the default rights of tokio, where the oracle restricts it to
//! the current user (`PipeOptions.CurrentUserOnly`): a client never detects a foreign pipe it can
//! open (a pipe made first by another account is connected to as if it were the daemon), and the
//! server accepts any local client the default rights let in (PARITY.md, Écarts assumés, lot 7.3;
//! no `unsafe`, decision of John of 2026-11-16).
//! Cancellation is the drop of the future, where .NET passes a token.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use sermofur_core::daemon_paths::DaemonPaths;
use sermofur_core::error::{Result, SermofurError};
use sermofur_core::ownership::FileOwnership;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

pub use crate::file_turn::already_running;
pub use sermofur_core::daemon_paths::EndpointState;

/// State of the endpoint, from file system metadata only (`IpcEndpoint.Inspect`): it never
/// connects.
pub fn inspect(paths: &DaemonPaths, owners: &dyn FileOwnership) -> Result<EndpointState> {
    sermofur_core::daemon_paths::endpoint_state(&paths.endpoint, owners)
}

/// `foreign_endpoint`, exit 4 (`IpcEndpoint.ForeignEndpoint`).
pub fn foreign_endpoint(paths: &DaemonPaths) -> SermofurError {
    SermofurError::new(
        "foreign_endpoint",
        format!(
            "The daemon endpoint {} or its folder belongs to another account or is not private; it is never used.",
            paths.endpoint.display()
        ),
        4,
    )
}

/// An I/O failure, as the .NET command runner maps an `IOException`: `invalid_storage_or_path`.
fn io_failure(error: io::Error) -> SermofurError {
    crate::file_turn::FileError::from(error).into()
}

trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

/// One connection to or from the daemon: a pipe or a socket.
pub struct Stream(Box<dyn Io>);

impl AsyncRead for Stream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut *self.0).poll_read(cx, buf)
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut *self.0).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut *self.0).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut *self.0).poll_shutdown(cx)
    }
}

/// Connects to the daemon of the current user, or `None` when nothing answers within
/// `timeout` (`IpcEndpoint.ConnectAsync`). An absent endpoint gives `None` at once; a foreign
/// one is `foreign_endpoint`.
pub async fn connect(
    paths: &DaemonPaths,
    owners: &dyn FileOwnership,
    timeout: Duration,
) -> Result<Option<Stream>> {
    match inspect(paths, owners)? {
        EndpointState::Foreign => Err(foreign_endpoint(paths)),
        EndpointState::Absent => Ok(None),
        EndpointState::Present => platform::connect(paths, timeout).await,
    }
}

/// Opens the server side of the endpoint (`IpcEndpoint.ListenAsync`): `daemon_already_running`
/// when a daemon already answers, `foreign_endpoint` when the endpoint is not ours. A socket
/// left by a crash is removed when it is ours and nothing answers on it.
pub async fn listen(paths: &DaemonPaths, owners: &dyn FileOwnership) -> Result<Listener> {
    platform::listen(paths, owners).await
}

pub use platform::Listener;

#[cfg(windows)]
mod platform {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use sermofur_core::daemon_paths::DaemonPaths;
    use sermofur_core::error::Result;
    use sermofur_core::ownership::FileOwnership;
    use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions};

    use super::{Stream, already_running, foreign_endpoint, io_failure};

    const ERROR_FILE_NOT_FOUND: i32 = 2;
    const ERROR_ACCESS_DENIED: i32 = 5;
    const ERROR_PIPE_BUSY: i32 = 231;

    fn pipe_name(paths: &DaemonPaths) -> PathBuf {
        PathBuf::from(r"\\.\pipe\").join(&paths.endpoint)
    }

    /// `NamedPipeClientStream.ConnectAsync(timeout)`: a busy pipe, or one not there yet, is
    /// tried again until the timeout; then, or on any other error, `None`. A pipe that refuses
    /// the current user is `foreign_endpoint`, as the `UnauthorizedAccessException` of the oracle.
    pub(super) async fn connect(paths: &DaemonPaths, timeout: Duration) -> Result<Option<Stream>> {
        let name = pipe_name(paths);
        let deadline = Instant::now() + timeout;
        loop {
            match ClientOptions::new().open(&name) {
                Ok(client) => return Ok(Some(Stream(Box::new(client)))),
                Err(error)
                    if matches!(
                        error.raw_os_error(),
                        Some(ERROR_PIPE_BUSY | ERROR_FILE_NOT_FOUND)
                    ) && Instant::now() < deadline =>
                {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                Err(error) if error.raw_os_error() == Some(ERROR_ACCESS_DENIED) => {
                    return Err(foreign_endpoint(paths));
                }
                Err(_) => return Ok(None),
            }
        }
    }

    pub(super) async fn listen(
        paths: &DaemonPaths,
        _owners: &dyn FileOwnership,
    ) -> Result<Listener> {
        let name = pipe_name(paths);
        // `first_pipe_instance`: fails when any process already owns a pipe of this name.
        let first = create(&name, true).map_err(|_| already_running())?;
        Ok(Listener {
            name,
            next: Some(first),
        })
    }

    fn create(name: &PathBuf, first: bool) -> std::io::Result<NamedPipeServer> {
        ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .create(name)
    }

    /// Named pipe server (`PipeListener`): one instance made ahead, then one per accept.
    pub struct Listener {
        name: PathBuf,
        next: Option<NamedPipeServer>,
    }

    impl Listener {
        /// Waits for the next client.
        pub async fn accept(&mut self) -> Result<Stream> {
            let pipe = match self.next.take() {
                Some(pipe) => pipe,
                None => create(&self.name, false).map_err(io_failure)?,
            };
            pipe.connect().await.map_err(io_failure)?;
            // One instance ahead, as `PipeListener`: a client arriving before the next accept
            // finds a listening instance instead of a busy pipe.
            self.next = create(&self.name, false).ok();
            Ok(Stream(Box::new(pipe)))
        }
    }
}

#[cfg(unix)]
mod platform {
    use std::fs;
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    use std::path::PathBuf;
    use std::time::Duration;

    use sermofur_core::daemon_paths::DaemonPaths;
    use sermofur_core::error::Result;
    use sermofur_core::ownership::{FileOwnership, effective_user_id};
    use tokio::net::{UnixListener, UnixStream};

    use super::{EndpointState, Stream, already_running, foreign_endpoint, inspect, io_failure};

    /// `ConnectSocketAsync`: `None` on any failure or when the timeout passes.
    pub(super) async fn connect(paths: &DaemonPaths, timeout: Duration) -> Result<Option<Stream>> {
        Ok(
            match tokio::time::timeout(timeout, UnixStream::connect(&paths.endpoint)).await {
                Ok(Ok(stream)) => Some(Stream(Box::new(stream))),
                _ => None,
            },
        )
    }

    pub(super) async fn listen(
        paths: &DaemonPaths,
        owners: &dyn FileOwnership,
    ) -> Result<Listener> {
        let folder = paths
            .runtime_directory()
            .ok_or_else(|| io_failure(std::io::ErrorKind::InvalidInput.into()))?;
        if owners.inspect(&folder).map_err(io_failure)?.is_none() {
            // `Directory.CreateDirectory(folder, 0700)`: the parents too, the folder itself private.
            if let Some(parent) = folder.parent() {
                fs::create_dir_all(parent).map_err(io_failure)?;
            }
            fs::DirBuilder::new()
                .mode(0o700)
                .create(&folder)
                .map_err(io_failure)?;
        }
        match inspect(paths, owners)? {
            EndpointState::Foreign => return Err(foreign_endpoint(paths)),
            EndpointState::Present => {
                if connect(paths, Duration::from_millis(500)).await?.is_some() {
                    return Err(already_running());
                }
                fs::remove_file(&paths.endpoint).map_err(io_failure)?;
            }
            EndpointState::Absent => {}
        }
        // ponytail: tokio binds and listens in one call (backlog 1024), then the mode is set;
        // .NET binds, sets 0600, then listens with 64. The 0700 folder keeps others out between.
        let socket = UnixListener::bind(&paths.endpoint).map_err(io_failure)?;
        let listener = Listener {
            socket,
            path: paths.endpoint.clone(),
            owner: effective_user_id(),
        };
        fs::set_permissions(&paths.endpoint, fs::Permissions::from_mode(0o600))
            .map_err(io_failure)?;
        Ok(listener)
    }

    /// Unix domain socket server that only keeps peers of the current user (`SocketListener`).
    pub struct Listener {
        socket: UnixListener,
        path: PathBuf,
        owner: u32,
    }

    impl Listener {
        /// Waits for the next client of the current user; a peer of another account, or of
        /// unknown identity, is closed unanswered.
        pub async fn accept(&mut self) -> Result<Stream> {
            loop {
                let (client, _) = self.socket.accept().await.map_err(io_failure)?;
                if client
                    .peer_cred()
                    .is_ok_and(|peer| peer.uid() == self.owner)
                {
                    return Ok(Stream(Box::new(client)));
                }
            }
        }
    }

    impl Drop for Listener {
        /// The socket is removed; one that stays is removed on the next start when it is ours
        /// and nothing answers on it.
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
        }
    }
}
