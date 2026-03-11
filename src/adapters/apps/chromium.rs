use anyhow::{Context, Result};

use crate::adapters::apps::AppAdapter;
use crate::engine::contract::{
    AdapterCapabilities, AppKind, MergeExecutionMode, MergePreparation, MoveDecision, TearResult,
    TopologyHandler,
};
use crate::engine::topology::Direction;

use super::librewolf::native_bridge::{self, BrowserTabState, NativeBrowserDescriptor};

pub const ADAPTER_NAME: &str = "chromium";
pub const ADAPTER_ALIASES: &[&str] = &["chromium", "chrome", "brave", "brave-browser"];
pub const APP_IDS: &[&str] = &[
    "brave-browser",
    "Brave Browser",
    "com.brave.Browser",
    "brave-browser-beta",
    "brave-browser-nightly",
    "chromium",
    "Chromium",
    "org.chromium.Chromium",
    "google-chrome",
    "Google Chrome",
    "com.google.Chrome",
    "google-chrome-beta",
    "google-chrome-dev",
];
pub const CHROMIUM_EXTENSION_ID: &str = "oigofebnnajpegmncnciacecfhlokkbp";
pub const CHROMIUM_EXTENSION_ORIGIN: &str = "chrome-extension://oigofebnnajpegmncnciacecfhlokkbp/";
pub const CHROMIUM_NATIVE_HOST_NAME: &str = "com.yeet_and_yoink.chromium_bridge";
pub const CHROMIUM_NATIVE_SOCKET_ENV: &str = "NIRI_DEEP_CHROMIUM_NATIVE_SOCKET";

const NATIVE_BRIDGE: NativeBrowserDescriptor = NativeBrowserDescriptor {
    socket_env: CHROMIUM_NATIVE_SOCKET_ENV,
    socket_basename: "chromium-bridge.sock",
    unavailable_browser_hint:
        "Install/enable the yeet-and-yoink Chromium browser extension and keep Brave/Chromium running.",
};

pub struct Chromium;

#[derive(Debug, Clone, Copy)]
struct BrowserMergePreparation {
    source_window_id: u64,
    source_tab_id: u64,
}

fn focus_target_index(state: BrowserTabState, dir: Direction) -> Option<usize> {
    match dir {
        Direction::West => state.active_tab_index.checked_sub(1),
        Direction::East => {
            (state.active_tab_index + 1 < state.tab_count).then_some(state.active_tab_index + 1)
        }
        Direction::North | Direction::South => None,
    }
}

fn move_target_index(state: BrowserTabState, dir: Direction) -> Option<usize> {
    match dir {
        Direction::West => {
            if state.active_tab_pinned {
                state.active_tab_index.checked_sub(1)
            } else if state.active_tab_index > state.pinned_tab_count {
                Some(state.active_tab_index - 1)
            } else {
                None
            }
        }
        Direction::East => {
            let upper_bound = if state.active_tab_pinned {
                state.pinned_tab_count
            } else {
                state.tab_count
            };
            (state.active_tab_index + 1 < upper_bound).then_some(state.active_tab_index + 1)
        }
        Direction::North | Direction::South => None,
    }
}

impl Chromium {
    fn tab_state(&self) -> Result<BrowserTabState> {
        Ok(native_bridge::tab_state(&NATIVE_BRIDGE)?)
    }
}

pub fn run_native_host() -> Result<()> {
    native_bridge::run_native_host(&NATIVE_BRIDGE)
}

impl AppAdapter for Chromium {
    fn adapter_name(&self) -> &'static str {
        ADAPTER_NAME
    }

    fn config_aliases(&self) -> Option<&'static [&'static str]> {
        Some(ADAPTER_ALIASES)
    }

    fn kind(&self) -> AppKind {
        AppKind::Browser
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            probe: true,
            focus: true,
            move_internal: true,
            resize_internal: false,
            rearrange: false,
            tear_out: true,
            merge: true,
        }
    }
}

impl TopologyHandler for Chromium {
    fn can_focus(&self, dir: Direction, _pid: u32) -> Result<bool> {
        Ok(focus_target_index(self.tab_state()?, dir).is_some())
    }

    fn window_count(&self, _pid: u32) -> Result<u32> {
        Ok(self.tab_state()?.tab_count as u32)
    }

    fn move_decision(&self, dir: Direction, _pid: u32) -> Result<MoveDecision> {
        let state = self.tab_state()?;
        if state.tab_count <= 1 {
            return Ok(MoveDecision::Passthrough);
        }
        match dir {
            Direction::West | Direction::East => Ok(if move_target_index(state, dir).is_some() {
                MoveDecision::Internal
            } else {
                MoveDecision::TearOut
            }),
            Direction::North | Direction::South => Ok(MoveDecision::TearOut),
        }
    }

    fn focus(&self, dir: Direction, _pid: u32) -> Result<()> {
        let state = self.tab_state()?;
        focus_target_index(state, dir)
            .with_context(|| format!("{ADAPTER_NAME} cannot focus {dir} inside the tab strip"))?;
        Ok(native_bridge::focus(&NATIVE_BRIDGE, dir)?)
    }

    fn move_internal(&self, dir: Direction, _pid: u32) -> Result<()> {
        let state = self.tab_state()?;
        move_target_index(state, dir)
            .with_context(|| format!("{ADAPTER_NAME} cannot move the current tab {dir}"))?;
        Ok(native_bridge::move_tab(&NATIVE_BRIDGE, dir)?)
    }

    fn move_out(&self, _dir: Direction, _pid: u32) -> Result<TearResult> {
        native_bridge::tear_out(&NATIVE_BRIDGE)?;
        Ok(TearResult {
            spawn_command: None,
        })
    }

    fn merge_execution_mode(&self) -> MergeExecutionMode {
        MergeExecutionMode::TargetFocused
    }

    fn prepare_merge(
        &self,
        _source_pid: Option<crate::engine::runtime::ProcessId>,
    ) -> Result<MergePreparation> {
        let state = self.tab_state()?;
        let source_window_id = state
            .window_id
            .context("browser bridge did not report the source window id")?;
        let source_tab_id = state
            .active_tab_id
            .context("browser bridge did not report the active tab id")?;
        Ok(MergePreparation::with_payload(BrowserMergePreparation {
            source_window_id,
            source_tab_id,
        }))
    }

    fn merge_into_target(
        &self,
        dir: Direction,
        _source_pid: Option<crate::engine::runtime::ProcessId>,
        _target_pid: Option<crate::engine::runtime::ProcessId>,
        preparation: MergePreparation,
    ) -> Result<()> {
        let preparation = preparation
            .into_payload::<BrowserMergePreparation>()
            .context("chromium merge requires source browser window and tab ids")?;
        native_bridge::merge_tab_into_focused_window(
            &NATIVE_BRIDGE,
            dir,
            preparation.source_window_id,
            preparation.source_tab_id,
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Chromium, MergeExecutionMode, MoveDecision, TopologyHandler, ADAPTER_ALIASES, ADAPTER_NAME,
        CHROMIUM_NATIVE_SOCKET_ENV,
    };
    use crate::engine::contract::AppAdapter;
    use crate::engine::topology::Direction;
    use serde_json::{json, Value};
    use std::collections::VecDeque;
    use std::ffi::OsString;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    };
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    struct NativeHarness {
        socket_path: PathBuf,
        log: Arc<Mutex<Vec<Value>>>,
        queue: Arc<Mutex<VecDeque<Value>>>,
        running: Arc<AtomicBool>,
        handle: Option<thread::JoinHandle<()>>,
        old_socket: Option<OsString>,
    }

    impl NativeHarness {
        fn new(responses: Vec<Value>) -> Self {
            let socket_path = std::env::temp_dir().join(format!(
                "yny-chromium-bridge-{}-{}.sock",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("clock should be monotonic enough for tests")
                    .as_nanos()
            ));
            let listener = UnixListener::bind(&socket_path)
                .expect("failed to bind fake browser bridge socket");
            listener
                .set_nonblocking(true)
                .expect("failed to make fake bridge listener nonblocking");
            let queue = Arc::new(Mutex::new(VecDeque::from(responses)));
            let log = Arc::new(Mutex::new(Vec::new()));
            let running = Arc::new(AtomicBool::new(true));
            let queue_thread = Arc::clone(&queue);
            let log_thread = Arc::clone(&log);
            let running_thread = Arc::clone(&running);
            let handle = thread::spawn(move || {
                while running_thread.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            handle_native_connection(stream, &queue_thread, &log_thread)
                        }
                        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(err) => panic!("fake bridge listener accept failed: {err}"),
                    }
                }
            });

            let old_socket = std::env::var_os(CHROMIUM_NATIVE_SOCKET_ENV);
            std::env::set_var(CHROMIUM_NATIVE_SOCKET_ENV, &socket_path);

            Self {
                socket_path,
                log,
                queue,
                running,
                handle: Some(handle),
                old_socket,
            }
        }

        fn requests(&self) -> Vec<Value> {
            self.log.lock().expect("log mutex poisoned").clone()
        }
    }

    impl Drop for NativeHarness {
        fn drop(&mut self) {
            self.running.store(false, Ordering::Relaxed);
            let _ = UnixStream::connect(&self.socket_path);
            if let Some(handle) = self.handle.take() {
                handle.join().expect("fake bridge listener should join");
            }
            let _ = std::fs::remove_file(&self.socket_path);
            if let Some(value) = &self.old_socket {
                std::env::set_var(CHROMIUM_NATIVE_SOCKET_ENV, value);
            } else {
                std::env::remove_var(CHROMIUM_NATIVE_SOCKET_ENV);
            }
            assert!(
                self.queue.lock().expect("queue mutex poisoned").is_empty(),
                "all fake browser bridge responses should be consumed"
            );
        }
    }

    fn handle_native_connection(
        mut stream: UnixStream,
        queue: &Arc<Mutex<VecDeque<Value>>>,
        log: &Arc<Mutex<Vec<Value>>>,
    ) {
        let mut reader = BufReader::new(stream.try_clone().expect("local stream should clone"));
        let mut line = String::new();
        let bytes = reader
            .read_line(&mut line)
            .expect("local bridge request should be readable");
        assert!(bytes > 0, "local bridge client should send a request");
        let payload = serde_json::from_str::<Value>(line.trim())
            .expect("local bridge request should be json");
        log.lock().expect("log mutex poisoned").push(payload);

        let response = queue
            .lock()
            .expect("queue mutex poisoned")
            .pop_front()
            .expect("unexpected extra browser bridge request");
        serde_json::to_writer(&mut stream, &response)
            .expect("local bridge response should serialize");
        stream
            .write_all(b"\n")
            .expect("local bridge response newline should write");
        stream.flush().expect("local bridge response should flush");
    }

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::utils::env_guard()
    }

    #[test]
    fn declares_explicit_capability_contract() {
        let app = Chromium;
        let caps = AppAdapter::capabilities(&app);
        assert!(caps.probe);
        assert!(caps.focus);
        assert!(caps.move_internal);
        assert!(!caps.resize_internal);
        assert!(caps.tear_out);
        assert!(caps.merge);
        assert!(!caps.rearrange);
        assert_eq!(app.adapter_name(), ADAPTER_NAME);
        assert_eq!(app.config_aliases(), Some(ADAPTER_ALIASES));
    }

    #[test]
    fn move_internal_uses_chromium_bridge_socket() {
        let _guard = env_guard();
        let harness = NativeHarness::new(vec![
            json!({
                "ok": true,
                "state": {
                    "activeTabIndex": 1,
                    "tabCount": 3,
                    "pinnedTabCount": 0,
                    "activeTabPinned": false
                }
            }),
            json!({ "ok": true }),
        ]);
        let app = Chromium;

        app.move_internal(Direction::East, 0)
            .expect("move east should succeed through chromium bridge");
        assert_eq!(
            harness.requests(),
            vec![
                json!({ "command": "get_tab_state" }),
                json!({
                    "command": "move_tab",
                    "direction": "East",
                }),
            ]
        );
    }

    #[test]
    fn merge_moves_torn_out_tab_into_focused_target_window() {
        let _guard = env_guard();
        let harness = NativeHarness::new(vec![
            json!({
                "ok": true,
                "state": {
                    "windowId": 8,
                    "activeTabId": 42,
                    "activeTabIndex": 0,
                    "tabCount": 1,
                    "pinnedTabCount": 0,
                    "activeTabPinned": false
                }
            }),
            json!({ "ok": true }),
        ]);
        let app = Chromium;

        assert!(matches!(
            TopologyHandler::merge_execution_mode(&app),
            MergeExecutionMode::TargetFocused
        ));
        let preparation = app
            .prepare_merge(None)
            .expect("prepare_merge should capture source browser tab ids");
        app.merge_into_target(Direction::North, None, None, preparation)
            .expect("merge_into_target should succeed");
        assert_eq!(
            harness.requests(),
            vec![
                json!({ "command": "get_tab_state" }),
                json!({
                    "command": "merge_tab",
                    "source_window_id": 8,
                    "source_tab_id": 42,
                    "direction": "North",
                }),
            ]
        );
    }

    #[test]
    fn vertical_move_decision_tears_out() {
        let _guard = env_guard();
        let _harness = NativeHarness::new(vec![json!({
            "ok": true,
            "state": {
                "activeTabIndex": 1,
                "tabCount": 3,
                "pinnedTabCount": 0,
                "activeTabPinned": false
            }
        })]);
        let app = Chromium;
        let decision = app
            .move_decision(Direction::South, 0)
            .expect("vertical move_decision should succeed");
        assert!(matches!(decision, MoveDecision::TearOut));
    }
}
