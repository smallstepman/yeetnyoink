use anyhow::{Context, Result};

use crate::adapters::apps::{browser_common, AppAdapter};
use crate::engine::browser_native::{
    self as native_bridge, BrowserTabState, NativeBrowserDescriptor,
};
use crate::engine::contracts::{
    AdapterCapabilities, AppKind, MergeExecutionMode, MergePreparation, MoveDecision, TearResult,
    TopologyHandler,
};
use crate::engine::topology::Direction;

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
pub const CHROMIUM_NATIVE_HOST_NAME: &str = "com.yeetnyoink.chromium_bridge";

const NATIVE_BRIDGE: NativeBrowserDescriptor = NativeBrowserDescriptor {
    socket_path_override: crate::config::chromium_native_socket_path,
    socket_basename: "chromium-bridge.sock",
    unavailable_browser_hint:
        "Install/enable the yeetnyoink Chromium browser extension and keep Brave/Chromium running.",
};

pub struct Chromium;

#[derive(Debug, Clone, Copy)]
struct BrowserMergePreparation {
    source_window_id: u64,
    source_tab_id: u64,
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
        if browser_common::focus_routes_through_wm(ADAPTER_ALIASES, dir) {
            return Ok(false);
        }
        Ok(browser_common::can_focus(
            ADAPTER_ALIASES,
            self.tab_state()?,
            dir,
        ))
    }

    fn window_count(&self, _pid: u32) -> Result<u32> {
        Ok(self.tab_state()?.tab_count as u32)
    }

    fn move_decision(&self, dir: Direction, _pid: u32) -> Result<MoveDecision> {
        if browser_common::move_routes_through_wm(ADAPTER_ALIASES, dir) {
            return Ok(MoveDecision::Passthrough);
        }
        Ok(browser_common::move_decision(
            ADAPTER_ALIASES,
            self.tab_state()?,
            dir,
        ))
    }

    fn focus(&self, dir: Direction, _pid: u32) -> Result<()> {
        browser_common::execute_focus(
            ADAPTER_NAME,
            ADAPTER_ALIASES,
            self.tab_state()?,
            dir,
            |mapped| {
                native_bridge::focus(&NATIVE_BRIDGE, mapped)?;
                Ok(())
            },
        )
    }

    fn move_internal(&self, dir: Direction, _pid: u32) -> Result<()> {
        browser_common::execute_move(
            ADAPTER_NAME,
            ADAPTER_ALIASES,
            self.tab_state()?,
            dir,
            |mapped| {
                native_bridge::move_tab(&NATIVE_BRIDGE, mapped)?;
                Ok(())
            },
        )
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
    };
    use crate::engine::contracts::AppAdapter;
    use crate::engine::topology::Direction;
    use serde_json::{json, Value};
    use std::collections::VecDeque;
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
        old_socket: Option<PathBuf>,
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
                            // On macOS, accepted sockets may inherit nonblocking from listener
                            stream
                                .set_nonblocking(false)
                                .expect("failed to make accepted stream blocking");
                            handle_native_connection(stream, &queue_thread, &log_thread)
                        }
                        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(err) => panic!("fake bridge listener accept failed: {err}"),
                    }
                }
            });

            let old_socket = crate::config::chromium_native_socket_path();
            crate::config::update(|cfg| {
                cfg.runtime.browser_native.chromium_socket_path = Some(socket_path.clone());
            });

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
            crate::config::update(|cfg| {
                cfg.runtime.browser_native.chromium_socket_path = self.old_socket.clone();
            });
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
        stream
            .set_nonblocking(false)
            .expect("local bridge stream should become blocking");
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

    fn unique_socket_path(prefix: &str) -> PathBuf {
        PathBuf::from(format!(
            "/tmp/{}-{}-{}.sock",
            prefix,
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock should be monotonic enough for tests")
                .as_nanos()
        ))
    }

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::utils::env_guard()
    }

    fn install_config(raw: &str) -> crate::config::Config {
        let old = crate::config::snapshot();
        let parsed: crate::config::Config =
            toml::from_str(raw).expect("browser config should parse");
        crate::config::install(parsed);
        old
    }

    fn restore_config(old: crate::config::Config) {
        crate::config::install(old);
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
    fn fake_native_harness_waits_for_delayed_request_on_nonblocking_listener() {
        let socket_path = unique_socket_path("yny-chromium-fake-harness");
        let listener =
            UnixListener::bind(&socket_path).expect("fake harness test socket should bind");
        listener
            .set_nonblocking(true)
            .expect("fake harness test listener should become nonblocking");
        let queue = Arc::new(Mutex::new(VecDeque::from(vec![json!({
            "ok": true,
            "state": {
                "activeTabIndex": 1,
                "tabCount": 3,
                "pinnedTabCount": 0,
                "activeTabPinned": false
            }
        })])));
        let log = Arc::new(Mutex::new(Vec::new()));
        let queue_thread = Arc::clone(&queue);
        let log_thread = Arc::clone(&log);
        let server = thread::spawn(move || {
            let (stream, _) = loop {
                match listener.accept() {
                    Ok(pair) => break pair,
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(err) => panic!("fake harness test accept failed: {err}"),
                }
            };
            handle_native_connection(stream, &queue_thread, &log_thread);
        });

        let mut client = UnixStream::connect(&socket_path).expect("fake harness client connects");
        thread::sleep(Duration::from_millis(50));
        client
            .write_all(b"{\"command\":\"get_tab_state\"}\n")
            .expect("fake harness request should write");
        client.flush().expect("fake harness request should flush");

        let mut response_line = String::new();
        let bytes = BufReader::new(client)
            .read_line(&mut response_line)
            .expect("fake harness response should read");
        assert!(bytes > 0, "fake harness client should receive a response");
        assert_eq!(
            serde_json::from_str::<Value>(response_line.trim())
                .expect("fake harness response should parse"),
            json!({
                "ok": true,
                "state": {
                    "activeTabIndex": 1,
                    "tabCount": 3,
                    "pinnedTabCount": 0,
                    "activeTabPinned": false
                }
            })
        );
        assert_eq!(
            log.lock().expect("log mutex poisoned").clone(),
            vec![json!({ "command": "get_tab_state" })]
        );

        server.join().expect("fake harness thread should join");
        let _ = std::fs::remove_file(&socket_path);
        assert!(
            queue.lock().expect("queue mutex poisoned").is_empty(),
            "fake harness test should consume queued response"
        );
    }

    #[test]
    fn can_focus_uses_active_tab_index_via_native_bridge() {
        let _guard = env_guard();
        let old = install_config(
            r#"
[app.browser.chromium]
enabled = true
"#,
        );
        let harness = NativeHarness::new(vec![
            json!({
                "ok": true,
                "state": {
                    "activeTabIndex": 0,
                    "tabCount": 3,
                    "pinnedTabCount": 0,
                    "activeTabPinned": false
                }
            }),
            json!({
                "ok": true,
                "state": {
                    "activeTabIndex": 1,
                    "tabCount": 3,
                    "pinnedTabCount": 0,
                    "activeTabPinned": false
                }
            }),
        ]);
        let app = Chromium;

        assert!(!app
            .can_focus(Direction::West, 0)
            .expect("west focus probe should succeed"));
        assert!(app
            .can_focus(Direction::East, 0)
            .expect("east focus probe should succeed"));
        assert_eq!(
            harness.requests(),
            vec![
                json!({ "command": "get_tab_state" }),
                json!({ "command": "get_tab_state" }),
            ]
        );

        drop(harness);
        restore_config(old);
    }

    #[test]
    fn move_internal_uses_chromium_bridge_socket() {
        let _guard = env_guard();
        let old = install_config(
            r#"
[app.browser.chromium]
enabled = true
"#,
        );
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

        drop(harness);
        restore_config(old);
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
    fn default_vertical_move_passes_through_to_wm() {
        let _guard = env_guard();
        let old = install_config(
            r#"
[app.browser.chromium]
enabled = true
"#,
        );
        let app = Chromium;
        let decision = app
            .move_decision(Direction::South, 0)
            .expect("vertical move_decision should succeed");
        assert!(matches!(decision, MoveDecision::Passthrough));
        restore_config(old);
    }

    #[test]
    fn vertical_tab_axis_passes_horizontal_requests_to_wm_without_bridge_probes() {
        let _guard = env_guard();
        let old = install_config(
            r#"
[app.browser.chromium]
enabled = true
tab_axis = "vertical"
"#,
        );
        let app = Chromium;

        assert!(!app
            .can_focus(Direction::West, 0)
            .expect("west focus should pass through to the WM"));
        assert!(matches!(
            app.move_decision(Direction::East, 0)
                .expect("east move should pass through to the WM"),
            MoveDecision::Passthrough
        ));

        restore_config(old);
    }

    #[test]
    fn vertical_tab_axis_maps_north_south_into_browser_tab_actions() {
        let _guard = env_guard();
        let old = install_config(
            r#"
[app.browser.chromium]
enabled = true
tab_axis = "vertical"
"#,
        );
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

        app.focus(Direction::North, 0)
            .expect("north focus should map to the previous browser tab");
        app.move_internal(Direction::South, 0)
            .expect("south move should map to the next browser tab");
        assert_eq!(
            harness.requests(),
            vec![
                json!({ "command": "get_tab_state" }),
                json!({
                    "command": "focus",
                    "direction": "West",
                }),
                json!({ "command": "get_tab_state" }),
                json!({
                    "command": "move_tab",
                    "direction": "East",
                }),
            ]
        );

        drop(harness);
        restore_config(old);
    }

    #[test]
    fn vertical_flipped_tab_axis_preserves_previous_north_south_behavior() {
        let _guard = env_guard();
        let old = install_config(
            r#"
[app.browser.chromium]
enabled = true
tab_axis = "vertical_flipped"
"#,
        );
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

        app.focus(Direction::North, 0)
            .expect("north focus should preserve the previous flipped browser mapping");
        app.move_internal(Direction::South, 0)
            .expect("south move should preserve the previous flipped browser mapping");
        assert_eq!(
            harness.requests(),
            vec![
                json!({ "command": "get_tab_state" }),
                json!({
                    "command": "focus",
                    "direction": "East",
                }),
                json!({ "command": "get_tab_state" }),
                json!({
                    "command": "move_tab",
                    "direction": "West",
                }),
            ]
        );

        drop(harness);
        restore_config(old);
    }
}
