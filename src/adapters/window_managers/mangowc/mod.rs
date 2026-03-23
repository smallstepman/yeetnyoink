use std::collections::HashSet;

use anyhow::{anyhow, Result};

pub mod mmsg;
#[cfg(target_os = "linux")]
pub mod toplevel;

use crate::config::WmBackend;
use crate::engine::topology::Direction;
use crate::engine::wm::{
    validate_declared_capabilities, CapabilitySupport, ConfiguredWindowManager,
    DirectionalCapability, FocusedWindowRecord, PrimitiveWindowManagerCapabilities, ResizeIntent,
    WindowManagerCapabilities, WindowManagerCapabilityDescriptor, WindowManagerFeatures,
    WindowManagerSession, WindowManagerSpec, WindowRecord,
};

pub struct MangowcAdapter {
    mmsg: Box<dyn MmsgClient>,
    toplevel: Box<dyn ToplevelClient>,
}

pub struct MangowcSpec;

pub static MANGOWC_SPEC: MangowcSpec = MangowcSpec;

trait MmsgClient: Send {
    fn focusdir(&self, direction: &str) -> Result<()>;
    fn exchange_client(&self, direction: &str) -> Result<()>;
    fn tagmon(&self, direction: &str) -> Result<()>;
    fn spawn(&self, command: &[String]) -> Result<()>;
    fn focused_snapshot(&self) -> Result<mmsg::FocusedSnapshot>;
}

trait ToplevelClient: Send {
    fn windows(&mut self) -> Result<Vec<WindowRecord>>;
    fn focus_window_by_id(&mut self, id: u64) -> Result<()>;
    fn close_window_by_id(&mut self, id: u64) -> Result<()>;
}

impl MmsgClient for mmsg::MmsgTransport {
    fn focusdir(&self, direction: &str) -> Result<()> {
        self.focusdir(direction)
    }

    fn exchange_client(&self, direction: &str) -> Result<()> {
        self.exchange_client(direction)
    }

    fn tagmon(&self, direction: &str) -> Result<()> {
        self.tagmon(direction)
    }

    fn spawn(&self, command: &[String]) -> Result<()> {
        self.spawn(command)
    }

    fn focused_snapshot(&self) -> Result<mmsg::FocusedSnapshot> {
        self.focused_snapshot()
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct RuntimeToplevelClient {
    session: toplevel::ForeignToplevelSession,
    store: toplevel::ToplevelStore,
}

#[cfg(target_os = "linux")]
impl RuntimeToplevelClient {
    fn connect() -> Result<Self> {
        Ok(Self {
            session: toplevel::ForeignToplevelSession::connect()?,
            store: toplevel::ToplevelStore::default(),
        })
    }

    fn refresh(&mut self) -> Result<()> {
        self.session.refresh_store(&mut self.store)
    }
}

#[cfg(target_os = "linux")]
impl ToplevelClient for RuntimeToplevelClient {
    fn windows(&mut self) -> Result<Vec<WindowRecord>> {
        self.refresh()?;
        Ok(self.store.windows().to_vec())
    }

    fn focus_window_by_id(&mut self, id: u64) -> Result<()> {
        self.refresh()?;
        self.session.activate_window_by_id(&self.store, id)
    }

    fn close_window_by_id(&mut self, id: u64) -> Result<()> {
        self.refresh()?;
        self.session.close_window_by_id(&self.store, id)
    }
}

impl WindowManagerSpec for MangowcSpec {
    fn backend(&self) -> WmBackend {
        WmBackend::Mangowc
    }

    fn name(&self) -> &'static str {
        MangowcAdapter::NAME
    }

    fn connect(&self) -> Result<ConfiguredWindowManager> {
        #[cfg(target_os = "linux")]
        {
            return ConfiguredWindowManager::try_new(
                Box::new(MangowcAdapter::connect()?),
                WindowManagerFeatures::default(),
            );
        }

        #[cfg(not(target_os = "linux"))]
        {
            anyhow::bail!(
                "wm backend 'mangowc' is not supported on {}",
                std::env::consts::OS
            )
        }
    }
}

impl MangowcAdapter {
    pub fn connect() -> Result<Self> {
        #[cfg(target_os = "linux")]
        {
            return Self::with_clients(
                Box::new(mmsg::MmsgTransport::connect()?),
                Box::new(RuntimeToplevelClient::connect()?),
            );
        }

        #[cfg(not(target_os = "linux"))]
        {
            anyhow::bail!(
                "mangowc adapter is not supported on {}",
                std::env::consts::OS
            )
        }
    }

    fn with_clients(mmsg: Box<dyn MmsgClient>, toplevel: Box<dyn ToplevelClient>) -> Result<Self> {
        validate_declared_capabilities::<Self>()?;
        Ok(Self { mmsg, toplevel })
    }
}

impl WindowManagerCapabilityDescriptor for MangowcAdapter {
    const NAME: &'static str = "mangowc";
    const CAPABILITIES: WindowManagerCapabilities = WindowManagerCapabilities {
        primitives: PrimitiveWindowManagerCapabilities {
            tear_out_right: false,
            move_column: false,
            consume_into_column_and_move: false,
            set_window_width: false,
            set_window_height: false,
        },
        tear_out: DirectionalCapability::uniform(CapabilitySupport::Unsupported),
        resize: DirectionalCapability::uniform(CapabilitySupport::Unsupported),
    };
}

impl WindowManagerSession for MangowcAdapter {
    fn adapter_name(&self) -> &'static str {
        Self::NAME
    }

    fn capabilities(&self) -> WindowManagerCapabilities {
        Self::CAPABILITIES
    }

    fn focused_window(&mut self) -> Result<FocusedWindowRecord> {
        let windows = self.toplevel.windows()?;
        let activated: Vec<WindowRecord> = windows
            .into_iter()
            .filter(|window| window.is_focused)
            .collect();
        match activated.as_slice() {
            [single] => Ok(to_focused_record(single)),
            [] => Err(anyhow!(
                "mangowc: no activated foreign toplevel window in snapshot"
            )),
            _ => {
                let focused_meta = self.mmsg.focused_snapshot()?;
                disambiguate_activated_windows(&activated, &focused_meta)
            }
        }
    }

    fn windows(&mut self) -> Result<Vec<WindowRecord>> {
        self.toplevel.windows()
    }

    fn focus_direction(&mut self, direction: Direction) -> Result<()> {
        self.mmsg.focusdir(direction_for_focus(direction))
    }

    fn move_direction(&mut self, direction: Direction) -> Result<()> {
        let dispatch_direction = direction_for_focus(direction);
        let before = self.mmsg.focused_snapshot()?;
        self.mmsg.focusdir(dispatch_direction)?;
        let after = self.mmsg.focused_snapshot()?;

        if focused_changed(&before, &after) {
            self.mmsg
                .focusdir(direction_for_focus(direction.opposite()))?;
            self.mmsg.exchange_client(dispatch_direction)
        } else {
            self.mmsg.tagmon(tagmon_for(direction))
        }
    }

    fn resize_with_intent(&mut self, _intent: ResizeIntent) -> Result<()> {
        anyhow::bail!("mangowc: resize is unsupported")
    }

    fn spawn(&mut self, command: Vec<String>) -> Result<()> {
        self.mmsg.spawn(&command)
    }

    fn focus_window_by_id(&mut self, id: u64) -> Result<()> {
        self.toplevel.focus_window_by_id(id)
    }

    fn close_window_by_id(&mut self, id: u64) -> Result<()> {
        self.toplevel.close_window_by_id(id)
    }
}

fn direction_for_focus(direction: Direction) -> &'static str {
    match direction {
        Direction::West => "left",
        Direction::East => "right",
        Direction::North => "up",
        Direction::South => "down",
    }
}

fn tagmon_for(direction: Direction) -> &'static str {
    match direction {
        Direction::West => "left",
        Direction::East => "right",
        Direction::North => "up",
        Direction::South => "down",
    }
}

fn to_focused_record(window: &WindowRecord) -> FocusedWindowRecord {
    FocusedWindowRecord {
        id: window.id,
        app_id: window.app_id.clone(),
        title: window.title.clone(),
        pid: window.pid,
        original_tile_index: window.original_tile_index,
    }
}

fn focused_changed(before: &mmsg::FocusedSnapshot, after: &mmsg::FocusedSnapshot) -> bool {
    before != after
}

fn disambiguate_activated_windows(
    activated: &[WindowRecord],
    focused_meta: &mmsg::FocusedSnapshot,
) -> Result<FocusedWindowRecord> {
    let mut matching_ids: Option<HashSet<u64>> = None;

    for ids in [
        matching_window_ids(activated, focused_meta.app_id.as_deref(), |window| {
            window.app_id.as_deref()
        }),
        matching_window_ids(activated, focused_meta.title.as_deref(), |window| {
            window.title.as_deref()
        }),
    ]
    .into_iter()
    .flatten()
    {
        matching_ids = Some(match matching_ids {
            Some(existing) => existing.intersection(&ids).copied().collect(),
            None => ids,
        });
    }

    let Some(matching_ids) = matching_ids else {
        return Err(anyhow!(
            "mangowc: ambiguous activated foreign toplevel windows; refusing to guess focus"
        ));
    };

    if matching_ids.len() != 1 {
        return Err(anyhow!(
            "mangowc: ambiguous activated foreign toplevel windows; refusing to guess focus"
        ));
    }
    let single_id = *matching_ids
        .iter()
        .next()
        .expect("single matching activated window id must exist");

    let single = activated
        .iter()
        .find(|window| window.id == single_id)
        .expect("matching activated window id must exist");
    Ok(to_focused_record(single))
}

fn matching_window_ids<F>(
    activated: &[WindowRecord],
    expected: Option<&str>,
    field: F,
) -> Option<HashSet<u64>>
where
    F: Fn(&WindowRecord) -> Option<&str>,
{
    let expected = expected?;
    Some(
        activated
            .iter()
            .filter(|window| field(window) == Some(expected))
            .map(|window| window.id)
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::{HashSet, VecDeque};
    use std::sync::{Arc, Mutex};

    use anyhow::bail;

    use super::*;

    #[derive(Debug, Default)]
    struct FakeMmsgState {
        calls: Vec<String>,
        snapshots: VecDeque<mmsg::FocusedSnapshot>,
    }

    #[derive(Clone, Debug)]
    struct FakeMmsg {
        state: Arc<Mutex<FakeMmsgState>>,
    }

    impl FakeMmsg {
        fn with_snapshots(
            snapshots: Vec<mmsg::FocusedSnapshot>,
        ) -> (Self, Arc<Mutex<FakeMmsgState>>) {
            let state = Arc::new(Mutex::new(FakeMmsgState {
                calls: Vec::new(),
                snapshots: snapshots.into(),
            }));
            (
                Self {
                    state: Arc::clone(&state),
                },
                state,
            )
        }
    }

    impl MmsgClient for FakeMmsg {
        fn focusdir(&self, direction: &str) -> Result<()> {
            self.state
                .lock()
                .expect("mmsg state lock poisoned")
                .calls
                .push(format!("focusdir:{direction}"));
            Ok(())
        }

        fn exchange_client(&self, direction: &str) -> Result<()> {
            self.state
                .lock()
                .expect("mmsg state lock poisoned")
                .calls
                .push(format!("exchange_client:{direction}"));
            Ok(())
        }

        fn tagmon(&self, direction: &str) -> Result<()> {
            self.state
                .lock()
                .expect("mmsg state lock poisoned")
                .calls
                .push(format!("tagmon:{direction}"));
            Ok(())
        }

        fn spawn(&self, command: &[String]) -> Result<()> {
            self.state
                .lock()
                .expect("mmsg state lock poisoned")
                .calls
                .push(format!("spawn:{}", command.join(" ")));
            Ok(())
        }

        fn focused_snapshot(&self) -> Result<mmsg::FocusedSnapshot> {
            let mut state = self.state.lock().expect("mmsg state lock poisoned");
            state.calls.push("focused_snapshot".to_string());
            Ok(state.snapshots.pop_front().unwrap_or_default())
        }
    }

    #[derive(Debug, Default)]
    struct FakeToplevelState {
        windows: Vec<WindowRecord>,
        focused_ids: Vec<u64>,
        closed_ids: Vec<u64>,
        stale_focus_ids: HashSet<u64>,
    }

    #[derive(Clone, Debug)]
    struct FakeToplevel {
        state: Arc<Mutex<FakeToplevelState>>,
    }

    impl FakeToplevel {
        fn with_windows(windows: Vec<WindowRecord>) -> (Self, Arc<Mutex<FakeToplevelState>>) {
            let state = Arc::new(Mutex::new(FakeToplevelState {
                windows,
                focused_ids: Vec::new(),
                closed_ids: Vec::new(),
                stale_focus_ids: HashSet::new(),
            }));
            (
                Self {
                    state: Arc::clone(&state),
                },
                state,
            )
        }
    }

    impl ToplevelClient for FakeToplevel {
        fn windows(&mut self) -> Result<Vec<WindowRecord>> {
            Ok(self
                .state
                .lock()
                .expect("toplevel state lock poisoned")
                .windows
                .clone())
        }

        fn focus_window_by_id(&mut self, id: u64) -> Result<()> {
            let mut state = self.state.lock().expect("toplevel state lock poisoned");
            if state.stale_focus_ids.contains(&id) {
                bail!("mangowc: stale window id {id}")
            }
            state.focused_ids.push(id);
            Ok(())
        }

        fn close_window_by_id(&mut self, id: u64) -> Result<()> {
            self.state
                .lock()
                .expect("toplevel state lock poisoned")
                .closed_ids
                .push(id);
            Ok(())
        }
    }

    fn test_adapter(
        windows: Vec<WindowRecord>,
        focused_snapshots: Vec<mmsg::FocusedSnapshot>,
    ) -> (
        MangowcAdapter,
        Arc<Mutex<FakeMmsgState>>,
        Arc<Mutex<FakeToplevelState>>,
    ) {
        let (mmsg, mmsg_state) = FakeMmsg::with_snapshots(focused_snapshots);
        let (toplevel, toplevel_state) = FakeToplevel::with_windows(windows);
        let adapter = MangowcAdapter::with_clients(Box::new(mmsg), Box::new(toplevel))
            .expect("test adapter should construct");
        (adapter, mmsg_state, toplevel_state)
    }

    fn window(id: u64, title: &str, app_id: &str, is_focused: bool, index: usize) -> WindowRecord {
        WindowRecord {
            id,
            app_id: Some(app_id.to_string()),
            title: Some(title.to_string()),
            pid: None,
            is_focused,
            original_tile_index: index,
        }
    }

    #[test]
    fn mangowc_adapter_windows_uses_foreign_toplevel_snapshot_ids() {
        let windows = vec![
            window(11, "left", "foot", false, 0),
            window(77, "right", "kitty", true, 1),
        ];
        let (mut adapter, _, _) = test_adapter(windows, vec![]);

        let actual = adapter.windows().expect("windows should load");
        assert_eq!(
            actual.iter().map(|w| w.id).collect::<Vec<_>>(),
            vec![11, 77]
        );
    }

    #[test]
    fn mangowc_adapter_focused_window_prefers_single_activated_handle() {
        let windows = vec![
            window(1, "one", "foot", false, 0),
            window(2, "two", "kitty", true, 1),
        ];
        let (mut adapter, mmsg_state, _) = test_adapter(windows, vec![]);

        let focused = adapter
            .focused_window()
            .expect("focused window should resolve");
        assert_eq!(focused.id, 2);
        let calls = &mmsg_state.lock().expect("mmsg state lock poisoned").calls;
        assert!(!calls.iter().any(|c| c == "focused_snapshot"));
    }

    #[test]
    fn mangowc_adapter_focused_window_uses_mmsg_metadata_to_break_activation_ties() {
        let windows = vec![
            window(10, "shell-a", "foot", true, 0),
            window(20, "shell-b", "kitty", true, 1),
        ];
        let snapshots = vec![mmsg::FocusedSnapshot {
            app_id: Some("kitty".to_string()),
            title: Some("shell-b".to_string()),
            x: Some(100),
            y: Some(100),
            width: Some(800),
            height: Some(600),
        }];
        let (mut adapter, _, _) = test_adapter(windows, snapshots);

        let focused = adapter
            .focused_window()
            .expect("focus tie should be broken safely");
        assert_eq!(focused.id, 20);
    }

    #[test]
    fn mangowc_adapter_focused_window_errors_when_focus_cannot_be_safely_disambiguated() {
        let windows = vec![
            window(10, "same", "foot", true, 0),
            window(20, "same", "foot", true, 1),
        ];
        let snapshots = vec![mmsg::FocusedSnapshot {
            app_id: Some("foot".to_string()),
            title: Some("same".to_string()),
            x: Some(0),
            y: Some(0),
            width: Some(1000),
            height: Some(700),
        }];
        let (mut adapter, _, _) = test_adapter(windows, snapshots);

        let err = adapter
            .focused_window()
            .expect_err("ambiguous focus should error");
        assert!(err
            .to_string()
            .contains("ambiguous activated foreign toplevel windows"));
    }

    #[test]
    fn mangowc_adapter_focused_window_errors_when_mmsg_metadata_conflicts() {
        let windows = vec![
            window(10, "shell-a", "foot", true, 0),
            window(20, "shell-b", "kitty", true, 1),
        ];
        let snapshots = vec![mmsg::FocusedSnapshot {
            app_id: Some("foot".to_string()),
            title: Some("shell-b".to_string()),
            x: Some(0),
            y: Some(0),
            width: Some(1000),
            height: Some(700),
        }];
        let (mut adapter, _, _) = test_adapter(windows, snapshots);

        let err = adapter
            .focused_window()
            .expect_err("conflicting metadata should error");
        assert!(err
            .to_string()
            .contains("ambiguous activated foreign toplevel windows"));
    }

    #[test]
    fn mangowc_adapter_move_direction_uses_exchange_when_probe_finds_target() {
        let windows = vec![window(1, "only", "foot", true, 0)];
        let snapshots = vec![
            mmsg::FocusedSnapshot {
                app_id: Some("foot".to_string()),
                title: Some("before".to_string()),
                x: Some(0),
                y: Some(0),
                width: Some(100),
                height: Some(100),
            },
            mmsg::FocusedSnapshot {
                app_id: Some("foot".to_string()),
                title: Some("after".to_string()),
                x: Some(10),
                y: Some(0),
                width: Some(100),
                height: Some(100),
            },
        ];
        let (mut adapter, mmsg_state, _) = test_adapter(windows, snapshots);

        adapter
            .move_direction(Direction::East)
            .expect("move should succeed");

        let calls = mmsg_state
            .lock()
            .expect("mmsg state lock poisoned")
            .calls
            .clone();
        assert_eq!(
            calls,
            vec![
                "focused_snapshot",
                "focusdir:right",
                "focused_snapshot",
                "focusdir:left",
                "exchange_client:right",
            ]
        );
    }

    #[test]
    fn mangowc_adapter_move_direction_uses_tagmon_when_probe_finds_no_target() {
        let windows = vec![window(1, "only", "foot", true, 0)];
        let snapshots = vec![
            mmsg::FocusedSnapshot {
                app_id: Some("foot".to_string()),
                title: Some("same".to_string()),
                x: Some(0),
                y: Some(0),
                width: Some(100),
                height: Some(100),
            },
            mmsg::FocusedSnapshot {
                app_id: Some("foot".to_string()),
                title: Some("same".to_string()),
                x: Some(0),
                y: Some(0),
                width: Some(100),
                height: Some(100),
            },
        ];
        let (mut adapter, mmsg_state, _) = test_adapter(windows, snapshots);

        adapter
            .move_direction(Direction::East)
            .expect("move should succeed");

        let calls = mmsg_state
            .lock()
            .expect("mmsg state lock poisoned")
            .calls
            .clone();
        assert_eq!(
            calls,
            vec![
                "focused_snapshot",
                "focusdir:right",
                "focused_snapshot",
                "tagmon:right",
            ]
        );
    }

    #[test]
    fn mangowc_adapter_focus_window_by_id_delegates_to_foreign_toplevel_activate() {
        let windows = vec![window(9, "x", "foot", true, 0)];
        let (mut adapter, _, toplevel_state) = test_adapter(windows, vec![]);

        adapter
            .focus_window_by_id(9)
            .expect("focus_window_by_id should succeed");

        let focused_ids = toplevel_state
            .lock()
            .expect("toplevel state lock poisoned")
            .focused_ids
            .clone();
        assert_eq!(focused_ids, vec![9]);
    }

    #[test]
    fn mangowc_adapter_close_window_by_id_delegates_to_foreign_toplevel_close() {
        let windows = vec![window(9, "x", "foot", true, 0)];
        let (mut adapter, _, toplevel_state) = test_adapter(windows, vec![]);

        adapter
            .close_window_by_id(9)
            .expect("close_window_by_id should succeed");

        let closed_ids = toplevel_state
            .lock()
            .expect("toplevel state lock poisoned")
            .closed_ids
            .clone();
        assert_eq!(closed_ids, vec![9]);
    }

    #[test]
    fn mangowc_adapter_focus_window_by_id_surfaces_stale_handle_errors() {
        let windows = vec![window(42, "x", "foot", true, 0)];
        let (mut adapter, _, toplevel_state) = test_adapter(windows, vec![]);
        toplevel_state
            .lock()
            .expect("toplevel state lock poisoned")
            .stale_focus_ids
            .insert(42);

        let err = adapter
            .focus_window_by_id(42)
            .expect_err("stale handle should error");
        assert!(err.to_string().contains("stale window id 42"));
    }
}
