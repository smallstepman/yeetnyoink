use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};

use crate::adapters::apps::AppAdapter;
use crate::engine::contract::{
    AdapterCapabilities, AppKind, MoveDecision, TearResult, TopologyHandler,
};
use crate::engine::runtime;
use crate::engine::topology::Direction;
use crate::logging;

/// LibreWolf / Firefox integration via an external Native Messaging bridge.
pub struct Librefox;

#[derive(Debug, Serialize)]
#[serde(tag = "cmd", rename_all = "camelCase")]
enum FirefoxRequest {
    GetTabState,
    Focus { direction: String },
    MoveTab { direction: String },
    TearOut,
    MergeIntoAdjacentWindow { direction: String },
    TriggerVimium { direction: String },
}

#[derive(Debug, Clone, Copy)]
struct TabState {
    active_tab_index: usize,
    tab_count: usize,
    active_group_index: Option<usize>,
    group_count: usize,
    vimium_available: bool,
}

impl Librefox {
    fn bridge_binary() -> String {
        std::env::var("NIRI_DEEP_FIREFOX_BRIDGE")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "yeet-and-yoink-firefox-bridge".to_string())
    }

    fn send_request(request: &FirefoxRequest) -> Result<Value> {
        let bridge = Self::bridge_binary();
        let payload =
            serde_json::to_string(request).context("failed to serialize firefox request")?;
        logging::debug(format!("librefox: bridge={} request={}", bridge, payload));

        let mut child = Command::new(&bridge)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to launch firefox bridge: {bridge}"))?;

        {
            let stdin = child
                .stdin
                .as_mut()
                .context("failed to acquire firefox bridge stdin")?;
            stdin
                .write_all(payload.as_bytes())
                .context("failed to write firefox bridge request")?;
            stdin
                .write_all(b"\n")
                .context("failed to terminate firefox bridge request")?;
        }

        let output = child
            .wait_with_output()
            .context("failed to read firefox bridge response")?;

        if !output.status.success() {
            let stderr = runtime::stderr_text(&output);
            if stderr.is_empty() {
                bail!("firefox bridge exited with status {}", output.status);
            }
            bail!("firefox bridge failed: {stderr}");
        }

        let stdout = runtime::stdout_text(&output);
        if stdout.is_empty() {
            return Ok(Value::Null);
        }

        let parsed: Value = serde_json::from_str(&stdout)
            .with_context(|| format!("failed to parse firefox bridge response json: {stdout}"))?;
        Self::unwrap_bridge_response(parsed)
    }

    fn unwrap_bridge_response(value: Value) -> Result<Value> {
        let Some(obj) = value.as_object() else {
            return Ok(value);
        };

        let Some(ok) = obj.get("ok").and_then(Value::as_bool) else {
            return Ok(value);
        };

        if ok {
            return Ok(obj.get("data").cloned().unwrap_or(Value::Null));
        }

        let error = obj
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("unknown firefox bridge error");
        bail!("firefox bridge error: {error}");
    }

    fn usize_field(value: &Value, keys: &[&str]) -> Option<usize> {
        keys.iter()
            .find_map(|key| value.get(*key))
            .and_then(Value::as_u64)
            .map(|number| number as usize)
    }

    fn bool_field(value: &Value, keys: &[&str]) -> Option<bool> {
        keys.iter()
            .find_map(|key| value.get(*key))
            .and_then(Value::as_bool)
    }

    fn tab_state(&self) -> Result<TabState> {
        let raw = Self::send_request(&FirefoxRequest::GetTabState)?;
        let active_tab_index = Self::usize_field(&raw, &["activeTabIndex", "active_tab_index"])
            .or_else(|| {
                raw.get("activeTab")
                    .or_else(|| raw.get("active_tab"))
                    .and_then(|tab| Self::usize_field(tab, &["index"]))
            })
            .context("firefox bridge response missing active tab index")?;

        let tab_count = Self::usize_field(&raw, &["tabCount", "tab_count"]).or_else(|| {
            raw.get("tabs")
                .and_then(Value::as_array)
                .map(std::vec::Vec::len)
        });
        let tab_count = tab_count.context("firefox bridge response missing tab count")?;

        let group_count = Self::usize_field(&raw, &["groupCount", "group_count"]).or_else(|| {
            raw.get("groups")
                .and_then(Value::as_array)
                .map(std::vec::Vec::len)
        });
        let group_count = group_count.unwrap_or(0);

        let active_group_index =
            Self::usize_field(&raw, &["activeGroupIndex", "active_group_index"]);
        let vimium_available =
            Self::bool_field(&raw, &["vimiumAvailable", "vimium_available"]).unwrap_or(false);

        Ok(TabState {
            active_tab_index,
            tab_count,
            active_group_index,
            group_count,
            vimium_available,
        })
    }

    fn command(&self, request: FirefoxRequest) -> Result<()> {
        Self::send_request(&request).map(|_| ())
    }

    fn at_tab_edge(state: TabState, dir: Direction) -> bool {
        match dir {
            Direction::West => state.active_tab_index == 0,
            Direction::East => state.active_tab_index + 1 >= state.tab_count,
            Direction::North | Direction::South => false,
        }
    }
}

impl AppAdapter for Librefox {
    fn adapter_name(&self) -> &'static str {
        "librefox"
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

impl TopologyHandler for Librefox {
    fn can_focus(&self, dir: Direction, _pid: u32) -> Result<bool> {
        let state = self.tab_state()?;
        let can = match dir {
            Direction::West => state.active_tab_index > 0,
            Direction::East => state.active_tab_index + 1 < state.tab_count,
            Direction::North => state
                .active_group_index
                .map(|index| index > 0)
                .unwrap_or(state.vimium_available),
            Direction::South => state
                .active_group_index
                .map(|index| index + 1 < state.group_count)
                .unwrap_or(state.vimium_available),
        };
        Ok(can)
    }

    fn move_decision(&self, dir: Direction, _pid: u32) -> Result<MoveDecision> {
        let state = self.tab_state()?;
        if state.tab_count <= 1 {
            return Ok(MoveDecision::Passthrough);
        }

        match dir {
            Direction::West | Direction::East => {
                if Self::at_tab_edge(state, dir) {
                    Ok(MoveDecision::TearOut)
                } else {
                    Ok(MoveDecision::Internal)
                }
            }
            Direction::North | Direction::South => Ok(MoveDecision::Passthrough),
        }
    }

    fn focus(&self, dir: Direction, _pid: u32) -> Result<()> {
        let primary = FirefoxRequest::Focus {
            direction: dir.to_string(),
        };
        if let Err(primary_err) = self.command(primary) {
            if matches!(dir, Direction::North | Direction::South) {
                logging::debug(format!(
                    "librefox: focus {} failed, trying vimium fallback: {:#}",
                    dir, primary_err
                ));
                return self
                    .command(FirefoxRequest::TriggerVimium {
                        direction: dir.to_string(),
                    })
                    .context("firefox focus failed and vimium fallback also failed");
            }
            return Err(primary_err);
        }
        Ok(())
    }

    fn move_internal(&self, dir: Direction, _pid: u32) -> Result<()> {
        match dir {
            Direction::West | Direction::East => self.command(FirefoxRequest::MoveTab {
                direction: dir.to_string(),
            }),
            Direction::North | Direction::South => {
                bail!("firefox internal move only supports west/east")
            }
        }
    }

    fn move_out(&self, _dir: Direction, _pid: u32) -> Result<TearResult> {
        self.command(FirefoxRequest::TearOut)?;
        Ok(TearResult {
            spawn_command: None,
        })
    }

    fn merge_into(&self, dir: Direction, _source_pid: u32) -> Result<()> {
        self.command(FirefoxRequest::MergeIntoAdjacentWindow {
            direction: dir.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{FirefoxRequest, Librefox};
    use crate::engine::contract::{AppAdapter, MoveDecision, TopologyHandler};
    use crate::engine::topology::Direction;

    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::utils::env_guard()
    }

    #[test]
    fn declares_explicit_capability_contract() {
        let app = Librefox;
        let caps = AppAdapter::capabilities(&app);
        assert!(caps.probe);
        assert!(caps.focus);
        assert!(caps.move_internal);
        assert!(!caps.resize_internal);
        assert!(caps.tear_out);
        assert!(caps.merge);
        assert!(!caps.rearrange);
    }

    struct FirefoxBridgeHarness {
        base: PathBuf,
        responses_dir: PathBuf,
        log_file: PathBuf,
        old_bridge: Option<OsString>,
        old_responses_dir: Option<OsString>,
        old_log_file: Option<OsString>,
    }

    impl FirefoxBridgeHarness {
        fn new() -> Self {
            let unique = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let base = std::env::temp_dir().join(format!(
                "yeet-and-yoink-firefox-bridge-test-{}-{unique}",
                std::process::id()
            ));
            let responses_dir = base.join("responses");
            let log_file = base.join("bridge.log");
            let bridge_bin = base.join("bridge");

            fs::create_dir_all(&responses_dir).expect("failed to create responses dir");
            fs::write(
                &bridge_bin,
                r#"#!/bin/sh
set -eu

payload="$(cat)"
printf '%s\n' "$payload" >> "${FIREFOX_BRIDGE_TEST_LOG}"

safe_payload="$(printf '%s' "$payload" | tr -c 'A-Za-z0-9._-' '_')"
status_file="${FIREFOX_BRIDGE_TEST_RESPONSES}/${safe_payload}.status"
stdout_file="${FIREFOX_BRIDGE_TEST_RESPONSES}/${safe_payload}.stdout"
stderr_file="${FIREFOX_BRIDGE_TEST_RESPONSES}/${safe_payload}.stderr"

status=0
if [ -f "$status_file" ]; then
  status="$(cat "$status_file")"
fi
if [ -f "$stdout_file" ]; then
  cat "$stdout_file"
fi
if [ -f "$stderr_file" ]; then
  cat "$stderr_file" >&2
fi
exit "$status"
"#,
            )
            .expect("failed to write fake bridge");

            let mut perms = fs::metadata(&bridge_bin)
                .expect("failed to stat fake bridge")
                .permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&bridge_bin, perms).expect("failed to chmod fake bridge");

            let old_bridge = std::env::var_os("NIRI_DEEP_FIREFOX_BRIDGE");
            let old_responses_dir = std::env::var_os("FIREFOX_BRIDGE_TEST_RESPONSES");
            let old_log_file = std::env::var_os("FIREFOX_BRIDGE_TEST_LOG");

            std::env::set_var("NIRI_DEEP_FIREFOX_BRIDGE", &bridge_bin);
            std::env::set_var("FIREFOX_BRIDGE_TEST_RESPONSES", &responses_dir);
            std::env::set_var("FIREFOX_BRIDGE_TEST_LOG", &log_file);

            Self {
                base,
                responses_dir,
                log_file,
                old_bridge,
                old_responses_dir,
                old_log_file,
            }
        }

        fn set_response(&self, request: FirefoxRequest, status: i32, stdout: &str, stderr: &str) {
            let payload =
                serde_json::to_string(&request).expect("failed to serialize fake bridge payload");
            let safe_payload: String = payload
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                        c
                    } else {
                        '_'
                    }
                })
                .collect();

            fs::write(
                self.responses_dir.join(format!("{safe_payload}.status")),
                status.to_string(),
            )
            .expect("failed to write fake status");
            fs::write(
                self.responses_dir.join(format!("{safe_payload}.stdout")),
                stdout,
            )
            .expect("failed to write fake stdout");
            fs::write(
                self.responses_dir.join(format!("{safe_payload}.stderr")),
                stderr,
            )
            .expect("failed to write fake stderr");
        }

        fn command_log(&self) -> String {
            fs::read_to_string(&self.log_file).unwrap_or_default()
        }
    }

    impl Drop for FirefoxBridgeHarness {
        fn drop(&mut self) {
            if let Some(value) = &self.old_bridge {
                std::env::set_var("NIRI_DEEP_FIREFOX_BRIDGE", value);
            } else {
                std::env::remove_var("NIRI_DEEP_FIREFOX_BRIDGE");
            }

            if let Some(value) = &self.old_responses_dir {
                std::env::set_var("FIREFOX_BRIDGE_TEST_RESPONSES", value);
            } else {
                std::env::remove_var("FIREFOX_BRIDGE_TEST_RESPONSES");
            }

            if let Some(value) = &self.old_log_file {
                std::env::set_var("FIREFOX_BRIDGE_TEST_LOG", value);
            } else {
                std::env::remove_var("FIREFOX_BRIDGE_TEST_LOG");
            }

            let _ = fs::remove_dir_all(&self.base);
        }
    }

    #[test]
    fn can_focus_uses_tab_edge_state() {
        let _env_guard = env_guard();
        let harness = FirefoxBridgeHarness::new();
        let app = Librefox;

        harness.set_response(
            FirefoxRequest::GetTabState,
            0,
            r#"{"ok":true,"data":{"activeTabIndex":0,"tabCount":3}}"#,
            "",
        );
        assert!(!app
            .can_focus(Direction::West, 0)
            .expect("can_focus west should succeed"));

        harness.set_response(
            FirefoxRequest::GetTabState,
            0,
            r#"{"ok":true,"data":{"activeTabIndex":1,"tabCount":3}}"#,
            "",
        );
        assert!(app
            .can_focus(Direction::East, 0)
            .expect("can_focus east should succeed"));
    }

    #[test]
    fn move_decision_tears_out_at_tab_strip_edge() {
        let _env_guard = env_guard();
        let harness = FirefoxBridgeHarness::new();
        let app = Librefox;

        harness.set_response(
            FirefoxRequest::GetTabState,
            0,
            r#"{"ok":true,"data":{"activeTabIndex":2,"tabCount":3}}"#,
            "",
        );

        let decision = app
            .move_decision(Direction::East, 0)
            .expect("move_decision should succeed");
        assert!(matches!(decision, MoveDecision::TearOut));
    }

    #[test]
    fn focus_north_falls_back_to_vimium_when_needed() {
        let _env_guard = env_guard();
        let harness = FirefoxBridgeHarness::new();
        let app = Librefox;

        harness.set_response(
            FirefoxRequest::Focus {
                direction: "north".to_string(),
            },
            1,
            "",
            "north focus unsupported",
        );
        harness.set_response(
            FirefoxRequest::TriggerVimium {
                direction: "north".to_string(),
            },
            0,
            r#"{"ok":true}"#,
            "",
        );

        app.focus(Direction::North, 0)
            .expect("focus north should fall back to vimium");

        let log = harness.command_log();
        assert!(log.contains(r#"{"cmd":"focus","direction":"north"}"#));
        assert!(log.contains(r#"{"cmd":"triggerVimium","direction":"north"}"#));
    }

    #[test]
    fn merge_into_sends_merge_command() {
        let _env_guard = env_guard();
        let harness = FirefoxBridgeHarness::new();
        let app = Librefox;

        harness.set_response(
            FirefoxRequest::MergeIntoAdjacentWindow {
                direction: "west".to_string(),
            },
            0,
            r#"{"ok":true}"#,
            "",
        );

        app.merge_into(Direction::West, 0)
            .expect("merge_into should succeed");

        let log = harness.command_log();
        assert!(log.contains(r#"{"cmd":"mergeIntoAdjacentWindow","direction":"west"}"#));
    }
}
