use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, bail, Context, Result};
use niri_ipc::socket::Socket;
use niri_ipc::{Action, Request, Response, SizeChange, Window, Workspace, WorkspaceReferenceArg};
use serde::{Deserialize, Serialize};
use std::any::TypeId;

use crate::config::WmBackend;
use crate::engine::runtime::ProcessId;
use crate::engine::topology::Direction;
use crate::engine::topology::{DomainId, LeafId, Rect};
use crate::engine::transfer::PaneState;
use crate::engine::transfer::{decode_native_window_ref, encode_native_window_ref};
use crate::engine::transfer::{
    DomainLeafSnapshot, DomainSnapshot, ErasedDomain, TilingDomain, TopologyModifierImpl,
    TopologyProvider,
};
use crate::engine::wm::{
    validate_declared_capabilities, CapabilitySupport, ConfiguredWindowManager,
    DirectionalCapability, FocusedWindowRecord, PrimitiveWindowManagerCapabilities, ResizeIntent,
    WindowCycleProvider, WindowCycleRequest, WindowManagerCapabilities,
    WindowManagerCapabilityDescriptor, WindowManagerDomainFactory, WindowManagerFeatures,
    WindowManagerSession, WindowManagerSpec, WindowRecord, WindowTearOutComposer,
};
use crate::logging;

pub struct NiriAdapter {
    pub(crate) inner: Arc<Mutex<Niri>>,
}

pub struct Niri {
    socket: Socket,
}

pub struct NiriSpec;

pub static NIRI_SPEC: NiriSpec = NiriSpec;

struct NiriDomainFactory;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SummonOrigin {
    workspace_id: u64,
    output: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SummonState {
    windows: HashMap<u64, SummonOrigin>,
}

impl NiriAdapter {
    pub fn connect() -> Result<Self> {
        validate_declared_capabilities::<Self>()?;
        Ok(Self::from_shared(Arc::new(Mutex::new(Niri::connect()?))))
    }

    pub(crate) fn from_shared(inner: Arc<Mutex<Niri>>) -> Self {
        Self { inner }
    }

    fn with_inner<R>(&self, f: impl FnOnce(&mut Niri) -> Result<R>) -> Result<R> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| anyhow!("niri adapter mutex should not be poisoned"))?;
        f(&mut inner)
    }
}

impl WindowManagerCapabilityDescriptor for NiriAdapter {
    const NAME: &'static str = "niri";
    const CAPABILITIES: WindowManagerCapabilities = WindowManagerCapabilities {
        primitives: PrimitiveWindowManagerCapabilities {
            tear_out_right: true,
            move_column: true,
            consume_into_column_and_move: true,
            set_window_width: true,
            set_window_height: true,
        },
        tear_out: DirectionalCapability {
            west: CapabilitySupport::Composed,
            east: CapabilitySupport::Native,
            north: CapabilitySupport::Composed,
            south: CapabilitySupport::Composed,
        },
        resize: DirectionalCapability {
            west: CapabilitySupport::Native,
            east: CapabilitySupport::Native,
            north: CapabilitySupport::Native,
            south: CapabilitySupport::Native,
        },
    };
}

impl WindowManagerSession for NiriAdapter {
    fn adapter_name(&self) -> &'static str {
        Self::NAME
    }

    fn capabilities(&self) -> WindowManagerCapabilities {
        Self::CAPABILITIES
    }

    fn focused_window(&mut self) -> Result<FocusedWindowRecord> {
        let window = self.with_inner(|inner| inner.focused_window())?;
        Ok(FocusedWindowRecord {
            id: window.id,
            app_id: window.app_id,
            title: window.title,
            pid: window
                .pid
                .and_then(|raw| u32::try_from(raw).ok())
                .and_then(ProcessId::new),
            original_tile_index: window
                .layout
                .pos_in_scrolling_layout
                .map(|(_, tile_idx)| tile_idx)
                .unwrap_or(1),
        })
    }

    fn windows(&mut self) -> Result<Vec<WindowRecord>> {
        Ok(self
            .with_inner(|inner| inner.windows())?
            .into_iter()
            .map(|window| WindowRecord {
                id: window.id,
                app_id: window.app_id,
                title: window.title,
                pid: window
                    .pid
                    .and_then(|raw| u32::try_from(raw).ok())
                    .and_then(ProcessId::new),
                is_focused: window.is_focused,
                original_tile_index: window
                    .layout
                    .pos_in_scrolling_layout
                    .map(|(_, tile_idx)| tile_idx)
                    .unwrap_or(1),
            })
            .collect())
    }

    fn focus_direction(&mut self, direction: Direction) -> Result<()> {
        self.with_inner(|inner| inner.focus_direction(direction))
    }

    fn move_direction(&mut self, direction: Direction) -> Result<()> {
        self.with_inner(|inner| inner.move_direction(direction))
    }

    fn resize_with_intent(&mut self, intent: ResizeIntent) -> Result<()> {
        self.with_inner(|inner| {
            inner.resize_window(intent.direction, intent.grow(), intent.step.abs().max(1))
        })
    }

    fn spawn(&mut self, command: Vec<String>) -> Result<()> {
        self.with_inner(|inner| inner.spawn(command))
    }

    fn focus_window_by_id(&mut self, id: u64) -> Result<()> {
        self.with_inner(|inner| inner.focus_window_by_id(id))
    }

    fn close_window_by_id(&mut self, id: u64) -> Result<()> {
        self.with_inner(|inner| inner.close_window_by_id(id))
    }
}

impl WindowTearOutComposer for NiriAdapter {
    fn compose_tear_out(&mut self, direction: Direction, source_tile_index: usize) -> Result<()> {
        self.with_inner(|inner| match direction {
            Direction::West | Direction::East => inner.move_column(direction),
            Direction::North | Direction::South => {
                inner.consume_into_column_and_move(direction, source_tile_index)
            }
        })
    }
}

impl WindowManagerSpec for NiriSpec {
    fn backend(&self) -> WmBackend {
        WmBackend::Niri
    }

    fn name(&self) -> &'static str {
        NiriAdapter::NAME
    }

    fn connect(&self) -> Result<ConfiguredWindowManager> {
        let shared = Arc::new(Mutex::new(Niri::connect()?));
        let mut features = WindowManagerFeatures::default();
        features.domain_factory = Some(Box::new(NiriDomainFactory));
        features.window_cycle = Some(Box::new(NiriAdapter::from_shared(shared.clone())));
        features.tear_out_composer = Some(Box::new(NiriAdapter::from_shared(shared.clone())));
        ConfiguredWindowManager::try_new(Box::new(NiriAdapter::from_shared(shared)), features)
    }
}

impl WindowManagerDomainFactory for NiriDomainFactory {
    fn create_domain(&self, domain_id: DomainId) -> Result<Box<dyn ErasedDomain>> {
        Ok(Box::new(NiriDomainPlugin::connect(domain_id)?))
    }
}

impl Niri {
    pub fn connect() -> Result<Self> {
        let _span = tracing::debug_span!("niri.connect").entered();
        logging::debug("niri: connecting to IPC socket");
        let socket = Socket::connect().context("failed to connect to niri IPC socket")?;
        logging::debug("niri: IPC socket connected");
        Ok(Self { socket })
    }

    fn send_action(&mut self, action: Action) -> Result<()> {
        let _span = tracing::debug_span!("niri.send_action", action = ?action).entered();
        logging::debug(format!("niri: action request = {:?}", action));
        let reply = self
            .socket
            .send(Request::Action(action))
            .context("failed to send niri action")?;
        match reply {
            Ok(Response::Handled) => {
                logging::debug("niri: action handled");
                Ok(())
            }
            Ok(other) => bail!("unexpected response: {:?}", other),
            Err(e) => bail!("niri error: {e}"),
        }
    }

    pub fn focused_window(&mut self) -> Result<Window> {
        let _span = tracing::debug_span!("niri.focused_window").entered();
        logging::debug("niri: requesting focused window");
        let reply = self
            .socket
            .send(Request::FocusedWindow)
            .context("failed to send FocusedWindow request")?;
        match reply {
            Ok(Response::FocusedWindow(Some(w))) => {
                logging::debug(format!(
                    "niri: focused window id={} app_id={:?} pid={:?}",
                    w.id, w.app_id, w.pid
                ));
                Ok(w)
            }
            Ok(Response::FocusedWindow(None)) => bail!("no focused window"),
            Ok(other) => bail!("unexpected response: {:?}", other),
            Err(e) => bail!("niri error: {e}"),
        }
    }

    pub fn windows(&mut self) -> Result<Vec<Window>> {
        let reply = self
            .socket
            .send(Request::Windows)
            .context("failed to send Windows request")?;
        match reply {
            Ok(Response::Windows(windows)) => Ok(windows),
            Ok(other) => bail!("unexpected response: {:?}", other),
            Err(e) => bail!("niri error: {e}"),
        }
    }

    pub fn workspaces(&mut self) -> Result<Vec<Workspace>> {
        let reply = self
            .socket
            .send(Request::Workspaces)
            .context("failed to send Workspaces request")?;
        match reply {
            Ok(Response::Workspaces(workspaces)) => Ok(workspaces),
            Ok(other) => bail!("unexpected response: {:?}", other),
            Err(e) => bail!("niri error: {e}"),
        }
    }

    pub fn focus_direction(&mut self, dir: Direction) -> Result<()> {
        let _span = tracing::debug_span!("niri.focus_direction", ?dir).entered();
        let action = match dir {
            Direction::West => Action::FocusColumnLeft {},
            Direction::East => Action::FocusColumnRight {},
            Direction::North => Action::FocusWindowOrWorkspaceUp {},
            Direction::South => Action::FocusWindowOrWorkspaceDown {},
        };
        self.send_action(action)
    }

    pub fn move_column(&mut self, dir: Direction) -> Result<()> {
        let action = match dir {
            Direction::West => Action::MoveColumnLeft {},
            Direction::East => Action::MoveColumnRight {},
            _ => return Ok(()),
        };
        self.send_action(action)
    }

    pub fn move_direction(&mut self, dir: Direction) -> Result<()> {
        let action = match dir {
            Direction::West => Action::ConsumeOrExpelWindowLeft { id: None },
            Direction::East => Action::ConsumeOrExpelWindowRight { id: None },
            Direction::North => Action::MoveWindowUpOrToWorkspaceUp {},
            Direction::South => Action::MoveWindowDownOrToWorkspaceDown {},
        };
        self.send_action(action)
    }

    pub fn resize_window(&mut self, dir: Direction, grow: bool, step: i32) -> Result<()> {
        let magnitude = step.abs().max(1);
        let directional_delta = match dir {
            Direction::East | Direction::South => magnitude,
            Direction::West | Direction::North => -magnitude,
        };
        let delta = if grow {
            directional_delta
        } else {
            -directional_delta
        };
        let change = SizeChange::AdjustFixed(delta);
        let action = match dir {
            Direction::West | Direction::East => Action::SetWindowWidth { id: None, change },
            Direction::North | Direction::South => Action::SetWindowHeight { id: None, change },
        };
        self.send_action(action)
    }

    pub fn spawn(&mut self, command: Vec<String>) -> Result<()> {
        self.send_action(Action::Spawn { command })
    }

    pub fn spawn_sh(&mut self, command: String) -> Result<()> {
        self.send_action(Action::SpawnSh { command })
    }

    pub fn focus_window_by_id(&mut self, id: u64) -> Result<()> {
        self.send_action(Action::FocusWindow { id })
    }

    pub fn close_window_by_id(&mut self, id: u64) -> Result<()> {
        self.send_action(Action::CloseWindow { id: Some(id) })
    }

    pub fn move_window_to_workspace(
        &mut self,
        window_id: u64,
        reference: WorkspaceReferenceArg,
        focus: bool,
    ) -> Result<()> {
        self.send_action(Action::MoveWindowToWorkspace {
            window_id: Some(window_id),
            reference,
            focus,
        })
    }

    pub fn move_window_to_monitor(&mut self, id: u64, output: String) -> Result<()> {
        self.send_action(Action::MoveWindowToMonitor {
            id: Some(id),
            output,
        })
    }

    /// After a new tile is created (and focused) to the right of the original,
    /// consume it into the original's column and position it right next to
    /// the original tile (below for south, above for north).
    ///
    /// `original_tile_idx` is the 1-based tile index of the original window
    /// in its column (from `layout.pos_in_scrolling_layout`).
    pub fn consume_into_column_and_move(
        &mut self,
        dir: Direction,
        original_tile_idx: usize,
    ) -> Result<()> {
        // The new tile is in its own column to the right of the original.
        // Consume it leftward into the original's column (goes to bottom).
        self.send_action(Action::ConsumeOrExpelWindowLeft { id: None })?;

        // B is now at the bottom of the column. Query its position.
        let new_window = self.focused_window()?;
        let new_tile_idx = new_window
            .layout
            .pos_in_scrolling_layout
            .map(|(_, t)| t)
            .unwrap_or(1);

        // Target position: right below the original for south, at the
        // original's position (pushing it down) for north.
        let target_idx = match dir {
            Direction::South => original_tile_idx + 1,
            Direction::North => original_tile_idx,
            _ => new_tile_idx, // no-op
        };

        // Move up from bottom to target.
        for _ in 0..new_tile_idx.saturating_sub(target_idx) {
            self.send_action(Action::MoveWindowUp {})?;
        }
        Ok(())
    }
}

impl WindowCycleProvider for NiriAdapter {
    fn focus_or_cycle(&mut self, request: &WindowCycleRequest) -> Result<()> {
        if request.new {
            let spawn = request
                .spawn
                .as_ref()
                .context("--new requires --spawn '<command>'")?;
            return self.with_inner(|inner| inner.spawn_sh(spawn.clone()));
        }

        let windows = self.with_inner(|inner| inner.windows())?;
        let focused_id = windows
            .iter()
            .find(|window| window.is_focused)
            .map(|window| window.id);

        let app_id = request.app_id.as_deref();
        let title = request.title.as_deref();
        let mut matches: Vec<Window> = windows
            .iter()
            .filter(|window| window_matches(window, app_id, title))
            .cloned()
            .collect();

        if matches.is_empty() {
            if let Some(spawn) = request.spawn.as_ref() {
                return self.with_inner(|inner| inner.spawn_sh(spawn.clone()));
            }
            bail!("no matching windows found and no --spawn provided");
        }

        matches.sort_by(|a, b| focus_sort_key(b).cmp(&focus_sort_key(a)));
        let target_idx = focused_id
            .and_then(|id| matches.iter().position(|window| window.id == id))
            .map(|idx| (idx + 1) % matches.len())
            .unwrap_or(0);
        let target = matches[target_idx].clone();

        if request.summon {
            self.with_inner(|inner| summon_or_return(inner, &target, &windows))?;
            return Ok(());
        }

        self.with_inner(|inner| inner.focus_window_by_id(target.id))
    }
}

fn window_matches(window: &Window, app_id: Option<&str>, title: Option<&str>) -> bool {
    if let Some(app_id) = app_id {
        if window.app_id.as_deref() != Some(app_id) {
            return false;
        }
    }
    if let Some(title) = title {
        let Some(window_title) = window.title.as_deref() else {
            return false;
        };
        if !window_title.to_lowercase().contains(&title.to_lowercase()) {
            return false;
        }
    }
    true
}

fn focus_sort_key(window: &Window) -> (u64, u32, u64) {
    let (secs, nanos) = window
        .focus_timestamp
        .map(|ts| (ts.secs, ts.nanos))
        .unwrap_or((0, 0));
    (secs, nanos, window.id)
}

trait NiriSession {
    fn workspaces(&mut self) -> Result<Vec<Workspace>>;
    fn move_window_to_workspace(
        &mut self,
        window_id: u64,
        reference: WorkspaceReferenceArg,
        focus: bool,
    ) -> Result<()>;
    fn move_window_to_monitor(&mut self, id: u64, output: String) -> Result<()>;
    fn focus_window_by_id(&mut self, id: u64) -> Result<()>;
}

impl NiriSession for Niri {
    fn workspaces(&mut self) -> Result<Vec<Workspace>> {
        Self::workspaces(self)
    }

    fn move_window_to_workspace(
        &mut self,
        window_id: u64,
        reference: WorkspaceReferenceArg,
        focus: bool,
    ) -> Result<()> {
        Self::move_window_to_workspace(self, window_id, reference, focus)
    }

    fn move_window_to_monitor(&mut self, id: u64, output: String) -> Result<()> {
        Self::move_window_to_monitor(self, id, output)
    }

    fn focus_window_by_id(&mut self, id: u64) -> Result<()> {
        Self::focus_window_by_id(self, id)
    }
}

fn retain_live_summon_windows(state: &mut SummonState, all_windows: &[Window]) -> bool {
    let live_window_ids: HashSet<u64> = all_windows.iter().map(|window| window.id).collect();
    let before = state.windows.len();
    state
        .windows
        .retain(|window_id, _| live_window_ids.contains(window_id));
    state.windows.len() != before
}

fn summon_or_return(
    niri: &mut impl NiriSession,
    target: &Window,
    all_windows: &[Window],
) -> Result<()> {
    let workspaces = niri.workspaces()?;
    let focused_workspace = workspaces
        .iter()
        .find(|workspace| workspace.is_focused)
        .cloned()
        .context("no focused workspace found")?;

    let workspaces_by_id: HashMap<u64, _> = workspaces
        .iter()
        .map(|workspace| (workspace.id, workspace))
        .collect();
    let mut state = load_summon_state()?;
    let mut state_dirty = retain_live_summon_windows(&mut state, all_windows);

    if target.is_focused {
        if let Some(origin) = state.windows.remove(&target.id) {
            niri.move_window_to_workspace(
                target.id,
                WorkspaceReferenceArg::Id(origin.workspace_id),
                false,
            )?;
            if let Some(output) = origin.output {
                niri.move_window_to_monitor(target.id, output)?;
            }
            save_summon_state(&state)?;
            return Ok(());
        }
    }

    if target.workspace_id != Some(focused_workspace.id) {
        let origin_output = target
            .workspace_id
            .and_then(|workspace_id| workspaces_by_id.get(&workspace_id))
            .and_then(|workspace| workspace.output.clone());
        if let std::collections::hash_map::Entry::Vacant(entry) = state.windows.entry(target.id) {
            entry.insert(SummonOrigin {
                workspace_id: target.workspace_id.unwrap_or(focused_workspace.id),
                output: origin_output,
            });
            state_dirty = true;
        }

        niri.move_window_to_workspace(
            target.id,
            WorkspaceReferenceArg::Id(focused_workspace.id),
            false,
        )?;
        if let Some(output) = focused_workspace.output.clone() {
            niri.move_window_to_monitor(target.id, output)?;
        }
    }

    if state_dirty {
        save_summon_state(&state)?;
    }

    niri.focus_window_by_id(target.id)
}

fn summon_state_path() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("yeetnyoink").join("summon-state.json")
}

fn load_summon_state() -> Result<SummonState> {
    let path = summon_state_path();
    if !path.exists() {
        return Ok(SummonState::default());
    }

    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read summon state file: {}", path.display()))?;
    let state: SummonState = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse summon state file: {}", path.display()))?;
    Ok(state)
}

fn save_summon_state(state: &SummonState) -> Result<()> {
    let path = summon_state_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create state directory: {}", parent.display()))?;
    }

    let serialized = serde_json::to_string(state).context("failed to serialize summon state")?;
    fs::write(&path, serialized)
        .with_context(|| format!("failed to write summon state file: {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Default)]
    struct FakeNiri {
        workspaces: Vec<Workspace>,
        actions: Vec<String>,
    }

    impl NiriSession for FakeNiri {
        fn workspaces(&mut self) -> Result<Vec<Workspace>> {
            Ok(self.workspaces.clone())
        }

        fn move_window_to_workspace(
            &mut self,
            window_id: u64,
            reference: WorkspaceReferenceArg,
            focus: bool,
        ) -> Result<()> {
            self.actions.push(format!(
                "move_window_to_workspace:{window_id}:{reference:?}:{focus}"
            ));
            Ok(())
        }

        fn move_window_to_monitor(&mut self, id: u64, output: String) -> Result<()> {
            self.actions
                .push(format!("move_window_to_monitor:{id}:{output}"));
            Ok(())
        }

        fn focus_window_by_id(&mut self, id: u64) -> Result<()> {
            self.actions.push(format!("focus_window_by_id:{id}"));
            Ok(())
        }
    }

    #[test]
    fn summon_or_return_persists_dead_entry_cleanup_when_target_has_no_origin() {
        let temp_dir = unique_temp_dir();
        let original_runtime_dir = std::env::var_os("XDG_RUNTIME_DIR");
        std::env::set_var("XDG_RUNTIME_DIR", &temp_dir);

        let result = (|| -> Result<()> {
            save_summon_state(&SummonState {
                windows: HashMap::from([(
                    99,
                    SummonOrigin {
                        workspace_id: 7,
                        output: Some("HDMI-A-1".to_string()),
                    },
                )]),
            })?;

            let mut fake = FakeNiri {
                workspaces: vec![workspace(1, true, Some("eDP-1"))],
                actions: Vec::new(),
            };
            let target = window(42, Some(1), true);

            summon_or_return(&mut fake, &target, std::slice::from_ref(&target))?;

            let state = load_summon_state()?;
            assert!(state.windows.is_empty());
            assert_eq!(fake.actions, vec!["focus_window_by_id:42"]);
            Ok(())
        })();

        match original_runtime_dir {
            Some(value) => std::env::set_var("XDG_RUNTIME_DIR", value),
            None => std::env::remove_var("XDG_RUNTIME_DIR"),
        }

        let _ = fs::remove_dir_all(&temp_dir);

        result.expect("summon state cleanup should be persisted");
    }

    fn unique_temp_dir() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("yeetnyoink-niri-test-{unique}"));
        fs::create_dir_all(&path).expect("temp dir should be created");
        path
    }

    fn window(id: u64, workspace_id: Option<u64>, is_focused: bool) -> Window {
        Window {
            id,
            title: Some(format!("window-{id}")),
            app_id: Some("test.app".to_string()),
            pid: Some(1234),
            workspace_id,
            is_focused,
            is_floating: false,
            is_urgent: false,
            layout: niri_ipc::WindowLayout {
                pos_in_scrolling_layout: Some((1, 1)),
                tile_size: (100.0, 100.0),
                window_size: (100, 100),
                tile_pos_in_workspace_view: Some((0.0, 0.0)),
                window_offset_in_tile: (0.0, 0.0),
            },
            focus_timestamp: None,
        }
    }

    fn workspace(id: u64, is_focused: bool, output: Option<&str>) -> Workspace {
        Workspace {
            id,
            idx: 1,
            name: None,
            output: output.map(str::to_string),
            is_urgent: false,
            is_active: is_focused,
            is_focused,
            active_window_id: None,
        }
    }
}

pub struct NiriDomainPlugin {
    domain_id: DomainId,
    inner: NiriAdapter,
}

impl NiriDomainPlugin {
    pub fn connect(domain_id: DomainId) -> Result<Self> {
        Ok(Self {
            domain_id,
            inner: NiriAdapter::connect()?,
        })
    }

    fn snapshot_leaves(&mut self) -> Result<Vec<DomainLeafSnapshot>> {
        let windows = self.inner.windows()?;
        Ok(windows
            .iter()
            .enumerate()
            .map(|(index, window)| {
                let x = (index as i32) * 1000;
                DomainLeafSnapshot {
                    id: (index as LeafId) + 1,
                    native_id: encode_native_window_ref(window.id, window.pid),
                    rect: Rect {
                        x,
                        y: 0,
                        w: 900,
                        h: 900,
                    },
                    focused: window.is_focused,
                }
            })
            .collect())
    }
}

impl TopologyProvider for NiriDomainPlugin {
    type NativeId = Vec<u8>;
    type Error = anyhow::Error;

    fn domain_name(&self) -> &'static str {
        "niri"
    }

    fn rect(&self) -> Rect {
        Rect {
            x: 0,
            y: 0,
            w: 10000,
            h: 10000,
        }
    }

    fn fetch_layout(&mut self) -> Result<(), Self::Error> {
        let _ = self.inner.windows()?;
        Ok(())
    }
}

impl TopologyModifierImpl for NiriDomainPlugin {
    fn focus_impl(&mut self, native_id: &Self::NativeId) -> Result<(), Self::Error> {
        let target = decode_native_window_ref(native_id).context("invalid niri native id")?;
        self.inner.focus_window_by_id(target.window_id)
    }

    fn move_impl(&mut self, native_id: &Self::NativeId, dir: Direction) -> Result<(), Self::Error> {
        let target = decode_native_window_ref(native_id).context("invalid niri native id")?;
        self.inner.focus_window_by_id(target.window_id)?;
        self.inner.move_direction(dir)
    }

    fn tear_off_impl(&mut self, _id: &Self::NativeId) -> Result<Box<dyn PaneState>, Self::Error> {
        Err(anyhow!("niri domain does not support payload tear-off"))
    }

    fn merge_in_impl(
        &mut self,
        _target: &Self::NativeId,
        _dir: Direction,
        _payload: Box<dyn PaneState>,
    ) -> Result<Self::NativeId, Self::Error> {
        Err(anyhow!("niri domain does not support payload merge-in"))
    }
}

impl TilingDomain for NiriDomainPlugin {
    fn supported_payload_types(&self) -> &'static [TypeId] {
        &[]
    }
}

impl ErasedDomain for NiriDomainPlugin {
    fn domain_id(&self) -> DomainId {
        self.domain_id
    }

    fn domain_name(&self) -> &'static str {
        "niri"
    }

    fn rect(&self) -> Rect {
        TopologyProvider::rect(self)
    }

    fn fetch_snapshot(&mut self) -> Result<DomainSnapshot> {
        Ok(DomainSnapshot {
            domain_id: self.domain_id,
            rect: TopologyProvider::rect(self),
            leaves: self.snapshot_leaves()?,
        })
    }

    fn supported_payload_types(&self) -> Vec<TypeId> {
        vec![]
    }

    fn tear_off(&mut self, native_id: &[u8]) -> Result<Box<dyn PaneState>> {
        self.tear_off_impl(&native_id.to_vec())
    }

    fn merge_in(
        &mut self,
        target_native_id: &[u8],
        dir: Direction,
        payload: Box<dyn PaneState>,
    ) -> Result<Vec<u8>> {
        self.merge_in_impl(&target_native_id.to_vec(), dir, payload)
    }
}
