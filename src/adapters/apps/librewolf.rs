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

pub const ADAPTER_NAME: &str = "librewolf";
pub const ADAPTER_ALIASES: &[&str] = &["librewolf", "firefox", "librefox"];
pub const APP_IDS: &[&str] = &["librewolf", "LibreWolf", "firefox", "Firefox"];
pub const FIREFOX_EXTENSION_ID: &str = "browser-bridge@yeet-and-yoink";
pub const FIREFOX_NATIVE_HOST_NAME: &str = "com.yeet_and_yoink.firefox_bridge";

const NATIVE_BRIDGE: NativeBrowserDescriptor = NativeBrowserDescriptor {
    socket_path_override: crate::config::firefox_native_socket_path,
    socket_basename: "firefox-bridge.sock",
    unavailable_browser_hint:
        "Install/enable the yeet-and-yoink browser extension and keep LibreWolf/Firefox running.",
};

pub struct Librewolf;

#[derive(Debug, Clone, Copy)]
struct BrowserMergePreparation {
    source_window_id: u64,
    source_tab_id: u64,
}

impl Librewolf {
    fn tab_state(&self) -> Result<BrowserTabState> {
        Ok(native_bridge::tab_state(&NATIVE_BRIDGE)?)
    }
}

pub fn run_native_host() -> Result<()> {
    native_bridge::run_native_host(&NATIVE_BRIDGE)
}

impl AppAdapter for Librewolf {
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

impl TopologyHandler for Librewolf {
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
            .context("librewolf merge requires source browser window and tab ids")?;
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
        native_bridge, Librewolf, MergeExecutionMode, MoveDecision, TopologyHandler,
        ADAPTER_ALIASES, ADAPTER_NAME, NATIVE_BRIDGE,
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
                "yny-firefox-bridge-{}-{}.sock",
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

            let old_socket = crate::config::firefox_native_socket_path();
            crate::config::update(|cfg| {
                cfg.runtime.browser_native.firefox_socket_path = Some(socket_path.clone());
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
                cfg.runtime.browser_native.firefox_socket_path = self.old_socket.clone();
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
    fn socket_path_uses_config_override() {
        let _guard = env_guard();
        let old = crate::config::snapshot();
        crate::config::update(|cfg| {
            cfg.runtime.browser_native.firefox_socket_path =
                Some(std::path::PathBuf::from("/tmp/yny-firefox-test.sock"));
        });

        assert_eq!(
            native_bridge::browser_bridge_socket_path(&NATIVE_BRIDGE),
            std::path::PathBuf::from("/tmp/yny-firefox-test.sock")
        );

        crate::config::install(old);
    }

    #[test]
    fn native_message_roundtrips() {
        let payload = json!({
            "id": 7,
            "command": "focus",
            "direction": "East",
        });
        let mut bytes = Vec::new();
        native_bridge::write_native_message(&mut bytes, &payload).expect("message should encode");
        let decoded = native_bridge::read_native_message(&mut std::io::Cursor::new(bytes))
            .expect("message should decode")
            .expect("message should exist");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&decoded).expect("json should parse"),
            payload
        );
    }

    #[test]
    fn browser_bridge_command_serializes_direction() {
        let value = serde_json::to_value(native_bridge::BrowserBridgeCommand::MoveTab {
            direction: Direction::East,
        })
        .expect("command should serialize");
        assert_eq!(
            value,
            json!({
                "command": "move_tab",
                "direction": "East",
            })
        );
    }

    #[test]
    fn browser_bridge_merge_command_serializes_payload() {
        let value = serde_json::to_value(native_bridge::BrowserBridgeCommand::MergeTab {
            source_window_id: 17,
            source_tab_id: 23,
            direction: Direction::North,
        })
        .expect("command should serialize");
        assert_eq!(
            value,
            json!({
                "command": "merge_tab",
                "source_window_id": 17,
                "source_tab_id": 23,
                "direction": "North",
            })
        );
    }

    #[test]
    fn fake_native_harness_waits_for_delayed_request_on_nonblocking_listener() {
        let socket_path = unique_socket_path("yny-firefox-fake-harness");
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
    fn declares_explicit_capability_contract() {
        let app = Librewolf;
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
    fn can_focus_uses_active_tab_index_via_native_bridge() {
        let _guard = env_guard();
        let old = install_config(
            r#"
[app.browser.librewolf]
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
        let app = Librewolf;

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
    fn window_count_comes_from_native_bridge_state() {
        let _guard = env_guard();
        let _harness = NativeHarness::new(vec![json!({
            "ok": true,
            "state": {
                "activeTabIndex": 1,
                "tabCount": 4,
                "pinnedTabCount": 1,
                "activeTabPinned": false
            }
        })]);
        let app = Librewolf;

        assert_eq!(app.window_count(0).expect("window_count should succeed"), 4);
    }

    #[test]
    fn move_decision_tears_out_at_pinned_boundary() {
        let _guard = env_guard();
        let old = install_config(
            r#"
[app.browser.librewolf]
enabled = true
"#,
        );
        let harness = NativeHarness::new(vec![json!({
            "ok": true,
            "state": {
                "activeTabIndex": 2,
                "tabCount": 5,
                "pinnedTabCount": 2,
                "activeTabPinned": false
            }
        })]);
        let app = Librewolf;

        let decision = app
            .move_decision(Direction::West, 0)
            .expect("move_decision should succeed");
        assert!(matches!(decision, MoveDecision::TearOut));
        drop(harness);
        restore_config(old);
    }

    #[test]
    fn default_vertical_move_passes_through_to_wm() {
        let _guard = env_guard();
        let old = install_config(
            r#"
[app.browser.librewolf]
enabled = true
"#,
        );
        let app = Librewolf;

        let decision = app
            .move_decision(Direction::North, 0)
            .expect("vertical move_decision should succeed");
        assert!(matches!(decision, MoveDecision::Passthrough));
        restore_config(old);
    }

    #[test]
    fn focus_moves_to_adjacent_tab_via_native_bridge() {
        let _guard = env_guard();
        let old = install_config(
            r#"
[app.browser.librewolf]
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
        let app = Librewolf;

        app.focus(Direction::East, 0)
            .expect("focus east should succeed through native bridge");
        assert_eq!(
            harness.requests(),
            vec![
                json!({ "command": "get_tab_state" }),
                json!({
                    "command": "focus",
                    "direction": "East",
                }),
            ]
        );

        drop(harness);
        restore_config(old);
    }

    #[test]
    fn move_internal_moves_current_tab_via_native_bridge() {
        let _guard = env_guard();
        let old = install_config(
            r#"
[app.browser.librewolf]
enabled = true
"#,
        );
        let harness = NativeHarness::new(vec![
            json!({
                "ok": true,
                "state": {
                    "activeTabIndex": 2,
                    "tabCount": 5,
                    "pinnedTabCount": 1,
                    "activeTabPinned": false
                }
            }),
            json!({ "ok": true }),
        ]);
        let app = Librewolf;

        app.move_internal(Direction::East, 0)
            .expect("move east should succeed through native bridge");
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
    fn tear_out_returns_no_spawn_command() {
        let _guard = env_guard();
        let harness = NativeHarness::new(vec![json!({"ok":true})]);
        let app = Librewolf;

        let result = app
            .move_out(Direction::East, 0)
            .expect("move_out should succeed through native bridge");
        assert!(result.spawn_command.is_none());
        assert_eq!(harness.requests(), vec![json!({"command":"tear_out"})]);
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
        let app = Librewolf;

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
    fn explicit_browser_overrides_repeat_bridge_commands_until_target_position() {
        let _guard = env_guard();
        let old = install_config(
            r#"
[app.browser.librewolf]
enabled = true
tab_axis = "vertical"

[app.browser.librewolf.focus]
left = "focus_first_tab"

[app.browser.librewolf.move]
right = "move_tab_to_last_position"
"#,
        );
        let harness = NativeHarness::new(vec![
            json!({
                "ok": true,
                "state": {
                    "activeTabIndex": 2,
                    "tabCount": 4,
                    "pinnedTabCount": 0,
                    "activeTabPinned": false
                }
            }),
            json!({ "ok": true }),
            json!({ "ok": true }),
            json!({
                "ok": true,
                "state": {
                    "activeTabIndex": 1,
                    "tabCount": 4,
                    "pinnedTabCount": 1,
                    "activeTabPinned": false
                }
            }),
            json!({ "ok": true }),
            json!({ "ok": true }),
        ]);
        let app = Librewolf;

        app.focus(Direction::West, 0)
            .expect("left override should focus repeatedly toward the first tab");
        app.move_internal(Direction::East, 0)
            .expect("right override should move repeatedly toward the last tab");
        assert_eq!(
            harness.requests(),
            vec![
                json!({ "command": "get_tab_state" }),
                json!({
                    "command": "focus",
                    "direction": "West",
                }),
                json!({
                    "command": "focus",
                    "direction": "West",
                }),
                json!({ "command": "get_tab_state" }),
                json!({
                    "command": "move_tab",
                    "direction": "East",
                }),
                json!({
                    "command": "move_tab",
                    "direction": "East",
                }),
            ]
        );

        drop(harness);
        restore_config(old);
    }
}
