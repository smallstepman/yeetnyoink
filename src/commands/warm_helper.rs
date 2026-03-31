use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};

use crate::engine::topology::Direction;

pub(crate) const WARM_HELPER_SOCKET_ENV: &str = "YNY_WARM_HELPER_SOCKET";
// Helper-backed focus can legitimately spend noticeable time in real WM/app focus work.
const FOCUS_FORWARD_TIMEOUT: Duration = Duration::from_secs(5);
const FOCUS_SERVER_CONNECTION_TIMEOUT: Duration = Duration::from_millis(500);
static SOCKET_GUARD_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Args)]
pub struct WarmHelperArgs {
    #[command(subcommand)]
    mode: WarmHelperMode,
}

#[derive(Debug, Clone, Subcommand)]
pub enum WarmHelperMode {
    Serve {
        #[arg(long, value_name = "PATH")]
        socket: PathBuf,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FocusDirection {
    West,
    East,
    North,
    South,
}

impl From<FocusDirection> for Direction {
    fn from(direction: FocusDirection) -> Self {
        match direction {
            FocusDirection::West => Self::West,
            FocusDirection::East => Self::East,
            FocusDirection::North => Self::North,
            FocusDirection::South => Self::South,
        }
    }
}

impl From<Direction> for FocusDirection {
    fn from(direction: Direction) -> Self {
        match direction {
            Direction::West => Self::West,
            Direction::East => Self::East,
            Direction::North => Self::North,
            Direction::South => Self::South,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FocusRequest {
    pub direction: FocusDirection,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FocusResponse {
    pub handled: bool,
    pub elapsed_ms: u64,
    pub error: Option<String>,
}

pub(crate) fn focus_forward_socket_path() -> Result<Option<PathBuf>> {
    match std::env::var(WARM_HELPER_SOCKET_ENV) {
        Ok(socket) => {
            let socket = socket.trim();
            if socket.is_empty() {
                bail!("{WARM_HELPER_SOCKET_ENV} was set but empty");
            }
            Ok(Some(PathBuf::from(socket)))
        }
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => {
            bail!("{WARM_HELPER_SOCKET_ENV} contains non-unicode data")
        }
    }
}

pub(crate) fn forward_focus_request(socket_path: &Path, direction: Direction) -> Result<()> {
    forward_focus_request_with_timeout(socket_path, direction, FOCUS_FORWARD_TIMEOUT)
}

fn forward_focus_request_with_timeout(
    socket_path: &Path,
    direction: Direction,
    timeout: Duration,
) -> Result<()> {
    let mut stream = UnixStream::connect(socket_path).with_context(|| {
        format!(
            "failed to connect warm helper socket {}",
            socket_path.display()
        )
    })?;
    configure_focus_stream_timeouts(&stream, timeout).with_context(|| {
        format!(
            "failed to configure warm helper focus socket timeouts for {}",
            socket_path.display()
        )
    })?;

    match forward_focus_over_stream(&mut stream, direction) {
        Ok(()) => Ok(()),
        Err(err) if is_timeout_error(&err) => Err(err).with_context(|| {
            format!(
                "warm helper focus request via {} timed out after {} ms",
                socket_path.display(),
                timeout.as_millis()
            )
        }),
        Err(err) => Err(anyhow!(
            "warm helper focus request failed via {}: {err:#}",
            socket_path.display()
        )),
    }
}

fn configure_focus_stream_timeouts(stream: &UnixStream, timeout: Duration) -> Result<()> {
    stream
        .set_write_timeout(Some(timeout))
        .context("failed to set warm helper focus write timeout")?;
    stream
        .set_read_timeout(Some(timeout))
        .context("failed to set warm helper focus read timeout")?;
    Ok(())
}

fn is_timeout_error(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause.downcast_ref::<io::Error>().is_some_and(|io_err| {
            matches!(
                io_err.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
            )
        })
    })
}

pub(crate) fn forward_focus_over_stream<S>(stream: &mut S, direction: Direction) -> Result<()>
where
    S: Read + Write,
{
    let request = FocusRequest {
        direction: direction.into(),
    };
    serde_json::to_writer(&mut *stream, &request)
        .context("failed to serialize warm helper focus request")?;
    stream
        .write_all(b"\n")
        .context("failed to terminate warm helper focus request")?;
    stream
        .flush()
        .context("failed to flush warm helper focus request")?;

    let mut response_line = String::new();
    let bytes = {
        let mut reader = BufReader::new(&mut *stream);
        reader
            .read_line(&mut response_line)
            .context("failed to read warm helper focus response")?
    };
    if bytes == 0 {
        bail!("warm helper closed the socket before replying");
    }

    let response: FocusResponse = serde_json::from_str(response_line.trim())
        .context("failed to parse warm helper focus response")?;
    if !response.handled {
        match response.error {
            Some(error) => {
                bail!("warm helper reported handled: false for focus request: {error}")
            }
            None => bail!("warm helper reported handled: false for focus request"),
        }
    }
    Ok(())
}

fn read_focus_request<S>(stream: &mut S) -> Result<FocusRequest>
where
    S: Read,
{
    let mut request_line = String::new();
    let bytes = {
        let mut reader = BufReader::new(stream);
        reader
            .read_line(&mut request_line)
            .context("failed to read warm helper focus request")?
    };
    if bytes == 0 {
        bail!("warm helper client closed the socket before sending a request");
    }

    serde_json::from_str(request_line.trim()).context("failed to parse warm helper focus request")
}

fn write_focus_response<S>(stream: &mut S, response: &FocusResponse) -> Result<()>
where
    S: Write,
{
    serde_json::to_writer(&mut *stream, response)
        .context("failed to serialize warm helper focus response")?;
    stream
        .write_all(b"\n")
        .context("failed to terminate warm helper focus response")?;
    stream
        .flush()
        .context("failed to flush warm helper focus response")?;
    Ok(())
}

fn elapsed_millis(started_at: Instant) -> u64 {
    u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn response_from_focus_result(result: Result<()>, elapsed_ms: u64) -> FocusResponse {
    match result {
        Ok(()) => FocusResponse {
            handled: true,
            elapsed_ms,
            error: None,
        },
        Err(err) => FocusResponse {
            handled: false,
            elapsed_ms,
            error: Some(format!("{err:#}")),
        },
    }
}

fn handle_focus_connection<S, H>(stream: &mut S, handle_focus: &mut H) -> Result<()>
where
    S: Read + Write,
    H: FnMut(Direction) -> Result<()>,
{
    let request = read_focus_request(stream)?;
    let started_at = Instant::now();
    let response = response_from_focus_result(
        handle_focus(request.direction.into()),
        elapsed_millis(started_at),
    );
    write_focus_response(stream, &response)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SocketFileIdentity {
    device: u64,
    inode: u64,
}

impl SocketFileIdentity {
    fn from_metadata(metadata: &fs::Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }
}

fn socket_path_metadata(socket_path: &Path) -> Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(socket_path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => {
            Err(err).with_context(|| format!("failed to inspect {}", socket_path.display()))
        }
    }
}

fn socket_file_identity(socket_path: &Path) -> Result<Option<SocketFileIdentity>> {
    Ok(socket_path_metadata(socket_path)?
        .map(|metadata| SocketFileIdentity::from_metadata(&metadata)))
}

fn unique_socket_guard_path(socket_path: &Path) -> PathBuf {
    let mut file_name = socket_path
        .file_name()
        .map(|value| value.to_os_string())
        .unwrap_or_else(|| OsString::from("yny-warm-helper.sock"));
    let serial = SOCKET_GUARD_COUNTER.fetch_add(1, Ordering::Relaxed);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    file_name.push(format!(".guard-{}-{stamp}-{serial}", std::process::id()));
    socket_path.with_file_name(file_name)
}

fn remove_socket_path_if_present(socket_path: &Path) -> Result<()> {
    match fs::remove_file(socket_path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err)
            .with_context(|| format!("failed to remove stale socket {}", socket_path.display())),
    }
}

struct SocketPathLink {
    socket_path: PathBuf,
    link_path: PathBuf,
}

impl SocketPathLink {
    fn capture(socket_path: &Path) -> Result<Option<Self>> {
        let link_path = unique_socket_guard_path(socket_path);
        match fs::hard_link(socket_path, &link_path) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => {
                return Err(err).with_context(|| {
                    format!(
                        "failed to create warm helper socket link {}",
                        link_path.display()
                    )
                })
            }
        }

        let metadata = match socket_path_metadata(&link_path)? {
            Some(metadata) => metadata,
            None => {
                return Err(anyhow!(
                    "warm helper socket link {} disappeared during capture",
                    link_path.display()
                ))
            }
        };
        if !metadata.file_type().is_socket() {
            let _ = fs::remove_file(&link_path);
            bail!(
                "warm helper socket path {} already exists and is not a socket",
                socket_path.display()
            );
        }

        Ok(Some(Self {
            socket_path: socket_path.to_path_buf(),
            link_path,
        }))
    }

    fn matches_socket_path(&self) -> Result<bool> {
        Ok(matches!(
            (
                socket_file_identity(&self.socket_path)?,
                socket_file_identity(&self.link_path)?,
            ),
            (Some(socket_identity), Some(link_identity)) if socket_identity == link_identity
        ))
    }

    fn connect(&self) -> io::Result<UnixStream> {
        UnixStream::connect(&self.link_path)
    }
}

impl Drop for SocketPathLink {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.link_path);
    }
}

fn is_stale_socket_connect_error(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::ConnectionRefused
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::NotFound
    )
}

fn prepare_socket_path_for_rebind_inner<F>(socket_path: &Path, before_remove: F) -> Result<()>
where
    F: FnOnce(),
{
    let Some(snapshot) = SocketPathLink::capture(socket_path)? else {
        return Ok(());
    };

    match snapshot.connect() {
        Ok(stream) => {
            drop(stream);
            bail!(
                "warm helper socket {} is already in use by a live listener",
                socket_path.display()
            );
        }
        Err(err) if is_stale_socket_connect_error(&err) => {
            before_remove();
            if snapshot.matches_socket_path()? {
                remove_socket_path_if_present(socket_path)?;
            }
            Ok(())
        }
        Err(err) => Err(err).with_context(|| {
            format!(
                "failed to probe existing warm helper socket {}",
                socket_path.display()
            )
        }),
    }
}

fn prepare_socket_path_for_rebind(socket_path: &Path) -> Result<()> {
    prepare_socket_path_for_rebind_inner(socket_path, || {})
}

#[cfg(test)]
fn prepare_socket_path_for_rebind_with_hook<F>(socket_path: &Path, before_remove: F) -> Result<()>
where
    F: FnOnce(),
{
    prepare_socket_path_for_rebind_inner(socket_path, before_remove)
}

fn bind_foreground_listener(socket_path: &Path) -> Result<UnixListener> {
    match UnixListener::bind(socket_path) {
        Ok(listener) => Ok(listener),
        Err(err)
            if matches!(
                err.kind(),
                io::ErrorKind::AddrInUse | io::ErrorKind::AlreadyExists
            ) =>
        {
            prepare_socket_path_for_rebind(socket_path)?;
            UnixListener::bind(socket_path).with_context(|| {
                format!(
                    "failed to bind warm helper socket {}",
                    socket_path.display()
                )
            })
        }
        Err(err) => Err(err).with_context(|| {
            format!(
                "failed to bind warm helper socket {}",
                socket_path.display()
            )
        }),
    }
}

struct SocketFileGuard {
    ownership: SocketPathLink,
}

impl SocketFileGuard {
    fn capture(socket_path: &Path) -> Result<Self> {
        let ownership = SocketPathLink::capture(socket_path)?.ok_or_else(|| {
            anyhow!(
                "warm helper socket {} disappeared before cleanup guard initialization",
                socket_path.display()
            )
        })?;
        Ok(Self { ownership })
    }
}

impl Drop for SocketFileGuard {
    fn drop(&mut self) {
        if self.ownership.matches_socket_path().unwrap_or(false) {
            let _ = fs::remove_file(&self.ownership.socket_path);
        }
    }
}

fn bind_foreground_listener_with_guard(
    socket_path: &Path,
) -> Result<(UnixListener, SocketFileGuard)> {
    let listener = bind_foreground_listener(socket_path)?;
    let guard = match SocketFileGuard::capture(socket_path) {
        Ok(guard) => guard,
        Err(err) => {
            drop(listener);
            return Err(err);
        }
    };
    Ok((listener, guard))
}

#[cfg(test)]
fn bind_test_listener_with_guard(socket_path: &Path) -> Result<(UnixListener, SocketFileGuard)> {
    bind_foreground_listener_with_guard(socket_path)
}

fn serve_connections_with_handler<H>(
    socket_path: &Path,
    mut handle_focus: H,
    max_connections: Option<usize>,
) -> Result<()>
where
    H: FnMut(Direction) -> Result<()>,
{
    serve_connections_with_handler_and_timeout(
        socket_path,
        &mut handle_focus,
        max_connections,
        FOCUS_SERVER_CONNECTION_TIMEOUT,
    )
}

fn serve_connections_with_handler_and_timeout<H>(
    socket_path: &Path,
    handle_focus: &mut H,
    max_connections: Option<usize>,
    connection_timeout: Duration,
) -> Result<()>
where
    H: FnMut(Direction) -> Result<()>,
{
    let (listener, _socket_guard) = bind_foreground_listener_with_guard(socket_path)?;
    let mut accepted_connections = 0usize;

    loop {
        let (mut stream, _) = listener.accept().with_context(|| {
            format!(
                "failed to accept warm helper connection on {}",
                socket_path.display()
            )
        })?;
        accepted_connections += 1;
        let connection_result = configure_focus_stream_timeouts(&stream, connection_timeout)
            .context("failed to configure warm helper server connection timeouts")
            .and_then(|()| handle_focus_connection(&mut stream, handle_focus))
            .with_context(|| {
                format!(
                    "failed to handle warm helper focus request on {}",
                    socket_path.display()
                )
            });
        if let Err(err) = connection_result {
            eprintln!("yeetnyoink: {err:#}");
        }

        if max_connections.is_some_and(|limit| accepted_connections >= limit) {
            return Ok(());
        }
    }
}

fn serve_foreground_with_handler<H>(socket_path: &Path, handle_focus: H) -> Result<()>
where
    H: FnMut(Direction) -> Result<()>,
{
    serve_connections_with_handler(socket_path, handle_focus, None)
}

#[cfg(test)]
fn serve_n_connections_with_handler<H>(
    socket_path: &Path,
    max_connections: usize,
    handle_focus: H,
) -> Result<()>
where
    H: FnMut(Direction) -> Result<()>,
{
    serve_connections_with_handler(socket_path, handle_focus, Some(max_connections))
}

#[cfg(test)]
fn serve_n_connections_with_handler_and_timeout<H>(
    socket_path: &Path,
    max_connections: usize,
    connection_timeout: Duration,
    mut handle_focus: H,
) -> Result<()>
where
    H: FnMut(Direction) -> Result<()>,
{
    serve_connections_with_handler_and_timeout(
        socket_path,
        &mut handle_focus,
        Some(max_connections),
        connection_timeout,
    )
}

#[cfg(test)]
fn serve_with_handler<H>(socket_path: &Path, handle_focus: H) -> Result<()>
where
    H: FnMut(Direction) -> Result<()>,
{
    serve_n_connections_with_handler(socket_path, 1, handle_focus)
}

pub fn run(args: WarmHelperArgs, _config_path: Option<&Path>) -> Result<()> {
    match args.mode {
        WarmHelperMode::Serve { socket } => {
            serve_foreground_with_handler(&socket, crate::commands::focus::run_local)
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::ffi::{OsStr, OsString};
    use std::sync::MutexGuard;

    use super::WARM_HELPER_SOCKET_ENV;

    pub(crate) struct WarmHelperSocketEnvGuard {
        _lock: MutexGuard<'static, ()>,
        original: Option<OsString>,
    }

    impl WarmHelperSocketEnvGuard {
        pub(crate) fn set(&self, value: impl AsRef<OsStr>) {
            std::env::set_var(WARM_HELPER_SOCKET_ENV, value);
        }

        pub(crate) fn remove(&self) {
            std::env::remove_var(WARM_HELPER_SOCKET_ENV);
        }
    }

    impl Drop for WarmHelperSocketEnvGuard {
        fn drop(&mut self) {
            if let Some(value) = self.original.as_ref() {
                std::env::set_var(WARM_HELPER_SOCKET_ENV, value);
            } else {
                std::env::remove_var(WARM_HELPER_SOCKET_ENV);
            }
        }
    }

    pub(crate) fn warm_helper_socket_env_guard() -> WarmHelperSocketEnvGuard {
        WarmHelperSocketEnvGuard {
            _lock: crate::utils::env_guard(),
            original: std::env::var_os(WARM_HELPER_SOCKET_ENV),
        }
    }
}

#[cfg(test)]
mod integration_tests {
    use super::{
        focus_forward_socket_path, forward_focus_over_stream, forward_focus_request,
        FocusDirection, FocusRequest, FocusResponse, WarmHelperArgs, WarmHelperMode,
    };
    use crate::engine::topology::Direction;
    use clap::{Parser, Subcommand};
    use serde_json::json;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    fn unique_socket_path(prefix: &str) -> PathBuf {
        let short_prefix = prefix.chars().take(16).collect::<String>();
        // Keep test socket paths comfortably under macOS Unix-domain path limits.
        PathBuf::from(format!(
            "/tmp/{short_prefix}-{}-{}.sock",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("test clock should be monotonic enough")
                .as_nanos()
        ))
    }

    fn connect_when_ready(socket_path: &Path) -> UnixStream {
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            match UnixStream::connect(socket_path) {
                Ok(stream) => return stream,
                Err(err)
                    if matches!(
                        err.kind(),
                        std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                    ) && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!(
                    "client should connect to helper socket {}: {err}",
                    socket_path.display()
                ),
            }
        }
    }

    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: TestCmd,
    }

    #[derive(Subcommand)]
    enum TestCmd {
        WarmHelper(WarmHelperArgs),
    }

    #[test]
    fn warm_helper_args_parse_serve_socket() {
        let cli =
            TestCli::try_parse_from(["yny", "warm-helper", "serve", "--socket", "/tmp/yny.sock"])
                .expect("warm-helper serve should parse");

        assert!(matches!(
            cli.command,
            TestCmd::WarmHelper(WarmHelperArgs {
                mode: WarmHelperMode::Serve { ref socket }
            }) if socket == Path::new("/tmp/yny.sock")
        ));
    }

    #[test]
    fn focus_protocol_uses_stable_wire_format() {
        let request = FocusRequest {
            direction: FocusDirection::West,
        };
        let response = FocusResponse {
            handled: true,
            elapsed_ms: 7,
            error: None,
        };

        assert_eq!(
            serde_json::to_value(&request).expect("focus request should serialize"),
            json!({ "direction": "west" })
        );
        assert_eq!(
            serde_json::to_value(&response).expect("focus response should serialize"),
            json!({ "handled": true, "elapsed_ms": 7, "error": null })
        );
        assert_eq!(
            serde_json::from_value::<FocusRequest>(json!({ "direction": "east" }))
                .expect("focus request should deserialize"),
            FocusRequest {
                direction: FocusDirection::East,
            }
        );
        assert!(
            serde_json::from_value::<FocusRequest>(json!({ "direction": "Right" })).is_err(),
            "focus request should reject engine-specific aliases"
        );
        assert_eq!(
            serde_json::from_value::<FocusResponse>(
                json!({ "handled": false, "elapsed_ms": 12, "error": "boom" })
            )
            .expect("focus response should deserialize"),
            FocusResponse {
                handled: false,
                elapsed_ms: 12,
                error: Some("boom".to_string()),
            }
        );
    }

    #[test]
    fn focus_direction_converts_to_engine_direction() {
        assert_eq!(Direction::from(FocusDirection::West), Direction::West);
        assert_eq!(Direction::from(FocusDirection::East), Direction::East);
        assert_eq!(Direction::from(FocusDirection::North), Direction::North);
        assert_eq!(Direction::from(FocusDirection::South), Direction::South);
    }

    #[test]
    fn focus_direction_converts_from_engine_direction() {
        assert_eq!(FocusDirection::from(Direction::West), FocusDirection::West);
        assert_eq!(FocusDirection::from(Direction::East), FocusDirection::East);
        assert_eq!(
            FocusDirection::from(Direction::North),
            FocusDirection::North
        );
        assert_eq!(
            FocusDirection::from(Direction::South),
            FocusDirection::South
        );
    }

    #[test]
    fn focus_forward_socket_path_returns_none_when_env_is_absent() {
        let guard = super::tests::warm_helper_socket_env_guard();
        guard.remove();

        assert_eq!(
            focus_forward_socket_path().expect("missing helper env should not error"),
            None
        );
    }

    #[test]
    fn focus_forward_socket_path_returns_configured_socket() {
        let guard = super::tests::warm_helper_socket_env_guard();
        guard.set("/tmp/yny-warm-helper.sock");

        assert_eq!(
            focus_forward_socket_path().expect("configured helper env should resolve"),
            Some(PathBuf::from("/tmp/yny-warm-helper.sock"))
        );
    }

    #[test]
    fn focus_forward_socket_path_rejects_empty_socket_value() {
        let guard = super::tests::warm_helper_socket_env_guard();
        guard.set("");

        let err = focus_forward_socket_path()
            .expect_err("empty helper env should be rejected explicitly");
        assert!(
            err.to_string().contains(super::WARM_HELPER_SOCKET_ENV),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn forward_focus_over_stream_uses_newline_delimited_json_protocol() {
        let (mut client, server) = UnixStream::pair().expect("unix stream pair should open");
        let server = thread::spawn(move || {
            let mut reader = BufReader::new(server);
            let mut request_line = String::new();
            reader
                .read_line(&mut request_line)
                .expect("server should read request line");
            assert_eq!(
                serde_json::from_str::<FocusRequest>(request_line.trim())
                    .expect("request line should deserialize"),
                FocusRequest {
                    direction: FocusDirection::South,
                }
            );

            let mut stream = reader.into_inner();
            stream
                .write_all(b"{\"handled\":true,\"elapsed_ms\":1,\"error\":null}\n")
                .expect("server should write response");
            stream.flush().expect("server should flush response");
        });

        forward_focus_over_stream(&mut client, Direction::South)
            .expect("handled focus response should succeed");
        server
            .join()
            .expect("protocol test server thread should complete");
    }

    #[test]
    fn forward_focus_over_stream_errors_when_helper_reports_unhandled() {
        let (mut client, server) = UnixStream::pair().expect("unix stream pair should open");
        let server = thread::spawn(move || {
            let mut reader = BufReader::new(server);
            let mut request_line = String::new();
            reader
                .read_line(&mut request_line)
                .expect("server should read request line");
            let mut stream = reader.into_inner();
            stream
                .write_all(b"{\"handled\":false,\"elapsed_ms\":1,\"error\":\"focus failed\"}\n")
                .expect("server should write response");
            stream.flush().expect("server should flush response");
        });

        let err = forward_focus_over_stream(&mut client, Direction::North)
            .expect_err("unhandled focus response should surface as an error");
        assert!(
            err.to_string().contains("focus failed"),
            "unexpected error: {err:#}"
        );
        server
            .join()
            .expect("protocol test server thread should complete");
    }

    #[test]
    fn warm_helper_server_handles_focus_request_over_real_unix_socket() {
        let socket_path = unique_socket_path("yny-warm-helper-success");
        let (request_tx, request_rx) = mpsc::channel();
        let server = thread::spawn({
            let socket_path = socket_path.clone();
            move || {
                super::serve_with_handler(&socket_path, move |direction| {
                    request_tx
                        .send(direction)
                        .expect("test should capture forwarded direction");
                    thread::sleep(Duration::from_millis(10));
                    Ok(())
                })
            }
        });

        let mut stream = connect_when_ready(&socket_path);
        serde_json::to_writer(
            &mut stream,
            &FocusRequest {
                direction: FocusDirection::West,
            },
        )
        .expect("request should serialize");
        stream
            .write_all(b"\n")
            .expect("request should be newline delimited");
        stream.flush().expect("request should flush");

        let mut response_line = String::new();
        BufReader::new(&mut stream)
            .read_line(&mut response_line)
            .expect("response should be readable");
        let response: FocusResponse =
            serde_json::from_str(response_line.trim()).expect("response should deserialize");
        assert_eq!(
            request_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("server should receive request direction"),
            Direction::West
        );
        assert!(response.handled);
        assert!(response.error.is_none());
        assert!(
            response.elapsed_ms >= 10,
            "elapsed_ms should include helper-side work: {response:?}"
        );

        server
            .join()
            .expect("server thread should join")
            .expect("server should finish without error");
        assert!(
            !socket_path.exists(),
            "server should clean up the socket path when it exits"
        );
    }

    #[test]
    fn warm_helper_server_removes_stale_socket_before_bind() {
        let socket_path = unique_socket_path("yny-warm-helper-stale");
        let listener = UnixListener::bind(&socket_path).expect("stale test listener should bind");
        drop(listener);

        let server = thread::spawn({
            let socket_path = socket_path.clone();
            move || super::serve_with_handler(&socket_path, |_direction| Ok(()))
        });

        let mut stream = connect_when_ready(&socket_path);
        serde_json::to_writer(
            &mut stream,
            &FocusRequest {
                direction: FocusDirection::East,
            },
        )
        .expect("request should serialize");
        stream
            .write_all(b"\n")
            .expect("request should be newline delimited");
        stream.flush().expect("request should flush");

        let mut response_line = String::new();
        BufReader::new(&mut stream)
            .read_line(&mut response_line)
            .expect("response should be readable");
        let response: FocusResponse =
            serde_json::from_str(response_line.trim()).expect("response should deserialize");
        assert!(response.handled, "response should indicate handled");

        server
            .join()
            .expect("server thread should join")
            .expect("server should finish without error");
    }

    #[test]
    fn warm_helper_server_refuses_to_replace_live_socket_listener() {
        let socket_path = unique_socket_path("yny-warm-helper-live");
        let listener = UnixListener::bind(&socket_path).expect("live test listener should bind");

        let err = super::bind_foreground_listener(&socket_path)
            .expect_err("live listener should prevent warm helper takeover");
        assert!(
            err.to_string().contains(&socket_path.display().to_string()),
            "unexpected error: {err:#}"
        );

        let client = UnixStream::connect(&socket_path)
            .expect("existing live listener should remain reachable after failed takeover");
        drop(client);
        drop(listener);
        std::fs::remove_file(&socket_path).expect("live test socket should be removed");
    }

    #[test]
    fn warm_helper_server_refuses_to_delete_regular_file_socket_path() {
        let socket_path = unique_socket_path("yny-warm-helper-file");
        std::fs::write(&socket_path, b"keep-me").expect("regular file test fixture should exist");

        let err = super::bind_foreground_listener(&socket_path)
            .expect_err("regular file path should not be deleted for socket binding");
        assert!(
            err.to_string().contains(&socket_path.display().to_string()),
            "unexpected error: {err:#}"
        );
        assert_eq!(
            std::fs::read(&socket_path).expect("regular file should remain after failed bind"),
            b"keep-me"
        );

        std::fs::remove_file(&socket_path).expect("regular file test fixture should be removed");
    }

    #[test]
    fn socket_file_guard_leaves_replaced_socket_path_in_place() {
        let socket_path = unique_socket_path("yny-warm-helper-guard");
        let (listener, guard) = super::bind_test_listener_with_guard(&socket_path)
            .expect("guard test listener should bind with cleanup tracking");

        std::fs::remove_file(&socket_path).expect("original socket path should be unlinked");
        std::fs::write(&socket_path, b"replacement")
            .expect("replacement file should occupy the path");

        drop(listener);
        drop(guard);

        assert_eq!(
            std::fs::read(&socket_path).expect("replacement file should remain after guard drop"),
            b"replacement"
        );
        std::fs::remove_file(&socket_path).expect("replacement file should be removed");
    }

    #[test]
    fn remove_socket_path_if_present_ignores_missing_path() {
        let socket_path = unique_socket_path("yny-warm-helper-gone");
        super::remove_socket_path_if_present(&socket_path)
            .expect("missing path should be tolerated during stale cleanup");
    }

    #[test]
    fn stale_socket_rebind_does_not_remove_replacement_path() {
        let socket_path = unique_socket_path("yny-warm-helper-stale-race");
        let listener = UnixListener::bind(&socket_path).expect("stale-race listener should bind");
        drop(listener);

        super::prepare_socket_path_for_rebind_with_hook(&socket_path, || {
            std::fs::remove_file(&socket_path).expect("stale socket path should be unlinked");
            std::fs::write(&socket_path, b"replacement")
                .expect("replacement file should occupy the stale socket path");
        })
        .expect("stale cleanup should tolerate a replaced path");

        assert_eq!(
            std::fs::read(&socket_path)
                .expect("replacement file should remain after stale cleanup"),
            b"replacement"
        );
        std::fs::remove_file(&socket_path).expect("replacement file should be removed");
    }

    #[test]
    fn warm_helper_server_survives_bad_client_and_handles_next_request() {
        let socket_path = unique_socket_path("yny-warm-helper-bad-client");
        let (request_tx, request_rx) = mpsc::channel();
        let server = thread::spawn({
            let socket_path = socket_path.clone();
            move || {
                super::serve_n_connections_with_handler(&socket_path, 2, move |direction| {
                    request_tx
                        .send(direction)
                        .expect("test should capture forwarded direction");
                    Ok(())
                })
            }
        });

        let mut bad_client = connect_when_ready(&socket_path);
        bad_client
            .write_all(b"{not valid json}\n")
            .expect("bad client should write malformed request");
        bad_client.flush().expect("bad client should flush request");
        drop(bad_client);

        let mut good_client = connect_when_ready(&socket_path);
        serde_json::to_writer(
            &mut good_client,
            &FocusRequest {
                direction: FocusDirection::North,
            },
        )
        .expect("valid request should serialize");
        good_client
            .write_all(b"\n")
            .expect("valid request should be newline delimited");
        good_client.flush().expect("valid request should flush");

        let mut response_line = String::new();
        BufReader::new(&mut good_client)
            .read_line(&mut response_line)
            .expect("valid client should receive response");
        let response: FocusResponse =
            serde_json::from_str(response_line.trim()).expect("response should deserialize");

        assert_eq!(
            request_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("server should handle the follow-up valid request"),
            Direction::North
        );
        assert!(response.handled, "valid request should still succeed");
        assert!(
            response.error.is_none(),
            "valid request should not report an error"
        );

        server
            .join()
            .expect("server thread should join")
            .expect("server should keep serving after a bad client");
    }

    #[test]
    fn warm_helper_server_times_out_silent_client_and_handles_next_request() {
        let socket_path = unique_socket_path("yny-warm-helper-silent");
        let (request_tx, request_rx) = mpsc::channel();
        let server = thread::spawn({
            let socket_path = socket_path.clone();
            move || {
                super::serve_n_connections_with_handler_and_timeout(
                    &socket_path,
                    2,
                    Duration::from_millis(20),
                    move |direction| {
                        request_tx
                            .send(direction)
                            .expect("test should capture forwarded direction");
                        Ok(())
                    },
                )
            }
        });

        let silent_client = connect_when_ready(&socket_path);
        thread::sleep(Duration::from_millis(50));

        let mut good_client = connect_when_ready(&socket_path);
        serde_json::to_writer(
            &mut good_client,
            &FocusRequest {
                direction: FocusDirection::South,
            },
        )
        .expect("valid request should serialize");
        good_client
            .write_all(b"\n")
            .expect("valid request should be newline delimited");
        good_client.flush().expect("valid request should flush");

        let mut response_line = String::new();
        BufReader::new(&mut good_client)
            .read_line(&mut response_line)
            .expect("valid client should receive response");
        let response: FocusResponse =
            serde_json::from_str(response_line.trim()).expect("response should deserialize");

        assert_eq!(
            request_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("server should handle the follow-up valid request"),
            Direction::South
        );
        assert!(response.handled, "valid request should still succeed");
        assert!(
            response.error.is_none(),
            "valid request should not report an error"
        );

        drop(silent_client);
        server
            .join()
            .expect("server thread should join")
            .expect("server should keep serving after a silent client");
    }

    #[test]
    fn forward_focus_request_errors_when_helper_socket_is_unavailable() {
        let socket_path = unique_socket_path("yny-warm-helper-missing");
        let err = forward_focus_request(&socket_path, Direction::West)
            .expect_err("missing helper socket should be an explicit error");
        assert!(
            err.to_string().contains(&socket_path.display().to_string()),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn forward_focus_request_times_out_when_helper_never_replies() {
        let socket_path = unique_socket_path("yny-warm-helper-timeout");
        let listener = UnixListener::bind(&socket_path).expect("timeout test listener should bind");
        let server = thread::spawn({
            let socket_path = socket_path.clone();
            move || {
                let (server, _) = listener.accept().expect("timeout helper should accept");
                let mut reader = BufReader::new(server);
                let mut request_line = String::new();
                reader
                    .read_line(&mut request_line)
                    .expect("timeout helper should read request");
                assert_eq!(
                    serde_json::from_str::<FocusRequest>(request_line.trim())
                        .expect("timeout request should deserialize"),
                    FocusRequest {
                        direction: FocusDirection::East,
                    }
                );
                thread::sleep(Duration::from_millis(100));
                drop(reader);
                std::fs::remove_file(socket_path).expect("timeout test socket should be removed");
            }
        });

        let err = super::forward_focus_request_with_timeout(
            &socket_path,
            Direction::East,
            Duration::from_millis(20),
        )
        .expect_err("stuck helper should time out explicitly");
        assert!(
            err.to_string().contains("timed out"),
            "unexpected error: {err:#}"
        );
        server
            .join()
            .expect("timeout test server thread should complete");
    }

    #[test]
    fn forward_focus_request_succeeds_when_helper_replies_within_timeout() {
        let socket_path = unique_socket_path("yny-warm-helper-slow-ok");
        let listener =
            UnixListener::bind(&socket_path).expect("slow-response test listener should bind");
        let server = thread::spawn({
            let socket_path = socket_path.clone();
            move || {
                let (server, _) = listener.accept().expect("slow helper should accept");
                let mut reader = BufReader::new(server);
                let mut request_line = String::new();
                reader
                    .read_line(&mut request_line)
                    .expect("slow helper should read request");
                assert_eq!(
                    serde_json::from_str::<FocusRequest>(request_line.trim())
                        .expect("slow helper request should deserialize"),
                    FocusRequest {
                        direction: FocusDirection::West,
                    }
                );
                thread::sleep(Duration::from_millis(50));
                let mut stream = reader.into_inner();
                stream
                    .write_all(b"{\"handled\":true,\"elapsed_ms\":50,\"error\":null}\n")
                    .expect("slow helper should write response");
                stream.flush().expect("slow helper should flush response");
                std::fs::remove_file(socket_path)
                    .expect("slow-response test socket should be removed");
            }
        });

        super::forward_focus_request_with_timeout(
            &socket_path,
            Direction::West,
            Duration::from_millis(200),
        )
        .expect("helper response within timeout should succeed");
        server
            .join()
            .expect("slow-response test server thread should complete");
    }

    #[test]
    fn forward_focus_request_surfaces_helper_error_text() {
        let socket_path = unique_socket_path("yny-warm-helper-failure");
        let listener = UnixListener::bind(&socket_path).expect("failure test listener should bind");
        let server = thread::spawn({
            let socket_path = socket_path.clone();
            move || {
                let (server, _) = listener.accept().expect("failure helper should accept");
                let mut reader = BufReader::new(server);
                let mut request_line = String::new();
                reader
                    .read_line(&mut request_line)
                    .expect("failure helper should read request");
                let mut stream = reader.into_inner();
                stream
                    .write_all(
                        b"{\"handled\":false,\"elapsed_ms\":3,\"error\":\"native focus failed\"}\n",
                    )
                    .expect("failure helper should write response");
                stream
                    .flush()
                    .expect("failure helper should flush response");
                std::fs::remove_file(socket_path).expect("failure test socket should be removed");
            }
        });

        let err = forward_focus_request(&socket_path, Direction::South)
            .expect_err("helper failure should fail closed");
        assert!(
            err.to_string().contains("native focus failed"),
            "unexpected error: {err:#}"
        );

        server
            .join()
            .expect("failure test server thread should complete");
    }
}
