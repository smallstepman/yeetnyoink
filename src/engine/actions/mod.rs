pub(crate) mod context;
pub(crate) mod focus;
pub(crate) mod merge;
pub(crate) mod movement;
pub(crate) mod probe;
pub(crate) mod resize;
pub(crate) mod tearout;
// Re-exported for use throughout the actions module and beyond; walk_chain
// is unused until later tasks consume it.
#[allow(unused_imports)]
pub(crate) use context::{AppContext, walk_chain};
pub(crate) use focus::*;
pub(crate) use merge::*;
pub(crate) use movement::*;
pub(crate) use probe::*;
pub(crate) use resize::*;
pub(crate) use tearout::*;

// ---------------------------------------------------------------------------
// Orchestrator — migrated from engine::orchestrator
// ---------------------------------------------------------------------------

use std::collections::BTreeMap;

use anyhow::Result;

use crate::engine::domain::ErasedDomain;
use crate::engine::domain::{domain_id_for_window, encode_native_window_ref};
use crate::engine::domain::{PayloadRegistry, TransferOutcome, TransferPipeline};
use crate::engine::topology::Direction;
use crate::engine::topology::{DomainId, GlobalLeaf, Rect};
use crate::engine::window_manager::{ConfiguredWindowManager, ResizeIntent, ResizeKind, WindowRecord};
use crate::logging;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    Focus,
    Move,
    Resize { grow: bool, step: i32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionRequest {
    pub kind: ActionKind,
    pub direction: Direction,
}

impl ActionRequest {
    pub const fn new(kind: ActionKind, direction: Direction) -> Self {
        Self { kind, direction }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingDecision {
    SameDomain,
    CrossDomain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingError {
    UnsupportedTransfer {
        source_domain: DomainId,
        target_domain: DomainId,
    },
}

pub struct Orchestrator {
    payload_registry: PayloadRegistry,
    domains: BTreeMap<DomainId, Box<dyn ErasedDomain>>,
}

impl Default for Orchestrator {
    fn default() -> Self {
        Self {
            payload_registry: PayloadRegistry::default(),
            domains: BTreeMap::new(),
        }
    }
}

impl Orchestrator {
    pub fn register_domain(&mut self, domain: Box<dyn ErasedDomain>) {
        self.domains.insert(domain.domain_id(), domain);
    }

    pub fn execute(
        &mut self,
        wm: &mut ConfiguredWindowManager,
        request: ActionRequest,
    ) -> Result<()> {
        self.execute_session(wm, request)
    }

    fn execute_session(
        &mut self,
        wm: &mut ConfiguredWindowManager,
        request: ActionRequest,
    ) -> Result<()> {
        match request.kind {
            ActionKind::Focus => self.execute_focus_session(wm, request.direction),
            ActionKind::Move => self.execute_move_session(wm, request.direction),
            ActionKind::Resize { grow, step } => {
                self.execute_resize_session(wm, request.direction, grow, step)
            }
        }
    }

    pub fn execute_focus(
        &mut self,
        wm: &mut ConfiguredWindowManager,
        dir: Direction,
    ) -> Result<()> {
        self.execute_focus_session(wm, dir)
    }

    fn execute_focus_session(
        &mut self,
        wm: &mut ConfiguredWindowManager,
        dir: Direction,
    ) -> Result<()> {
        let _span = tracing::debug_span!("orchestrator.execute_focus", ?dir).entered();
        let fallback_dir = dir.into();
        if attempt_focused_app_focus(wm, fallback_dir)? {
            return Ok(());
        }
        wm.focus_direction(fallback_dir)
    }

    pub fn execute_move(&mut self, wm: &mut ConfiguredWindowManager, dir: Direction) -> Result<()> {
        self.execute_move_session(wm, dir)
    }

    fn execute_move_session(
        &mut self,
        wm: &mut ConfiguredWindowManager,
        dir: Direction,
    ) -> Result<()> {
        let fallback_dir = dir.into();
        if attempt_focused_app_move(wm, fallback_dir)? {
            return Ok(());
        }

        let focused = focused_window_record(wm)?;
        let Some(target_window) = probe_directional_target(
            wm,
            dir,
            focused.id,
            DirectionalProbeFocusMode::RestoreSource,
        )?
        else {
            return wm.move_direction(fallback_dir);
        };
        let focused_leaf = Self::leaf_from_window(&focused, 1);
        let target_leaf = Self::leaf_from_window(&target_window, 2);

        match self.route(&focused_leaf, &target_leaf) {
            RoutingDecision::SameDomain => {
                if self
                    .attempt_same_domain_transfer(&focused_leaf, &target_leaf, dir)
                    .unwrap_or(false)
                {
                    Ok(())
                } else {
                    wm.move_direction(fallback_dir)
                }
            }
            RoutingDecision::CrossDomain => {
                if self
                    .attempt_cross_domain_transfer(&focused_leaf, &target_leaf, dir)
                    .unwrap_or(false)
                {
                    Ok(())
                } else {
                    let err = RoutingError::UnsupportedTransfer {
                        source_domain: focused_leaf.domain,
                        target_domain: target_leaf.domain,
                    };
                    logging::debug(format!("orchestrator: {:?}", err));
                    wm.move_direction(fallback_dir)
                }
            }
        }
    }

    fn leaf_from_window(window: &WindowRecord, leaf_id: u64) -> GlobalLeaf {
        let domain = domain_id_for_window(
            window.app_id.as_deref(),
            window.pid,
            window.title.as_deref(),
        );
        GlobalLeaf {
            id: leaf_id,
            domain,
            native_id: encode_native_window_ref(window.id, window.pid),
            rect: Rect {
                x: leaf_id as i32,
                y: 0,
                w: 1,
                h: 1,
            },
        }
    }

    pub fn execute_resize(
        &mut self,
        wm: &mut ConfiguredWindowManager,
        dir: Direction,
        grow: bool,
        step: i32,
    ) -> Result<()> {
        self.execute_resize_session(wm, dir, grow, step)
    }

    fn execute_resize_session(
        &mut self,
        wm: &mut ConfiguredWindowManager,
        dir: Direction,
        grow: bool,
        step: i32,
    ) -> Result<()> {
        if attempt_focused_app_resize(wm, dir, grow, step.max(1))? {
            return Ok(());
        }
        let intent = ResizeIntent::new(
            dir.into(),
            if grow {
                ResizeKind::Grow
            } else {
                ResizeKind::Shrink
            },
            step.max(1),
        );
        wm.resize_with_intent(intent)
    }

    pub fn route(&self, source: &GlobalLeaf, target: &GlobalLeaf) -> RoutingDecision {
        if source.domain == target.domain {
            RoutingDecision::SameDomain
        } else {
            RoutingDecision::CrossDomain
        }
    }

    fn attempt_cross_domain_transfer(
        &mut self,
        source: &GlobalLeaf,
        target: &GlobalLeaf,
        dir: Direction,
    ) -> Result<bool> {
        let Some(mut source_domain) = self.domains.remove(&source.domain) else {
            return Ok(false);
        };
        let Some(target_domain) = self.domains.get_mut(&target.domain) else {
            self.domains.insert(source.domain, source_domain);
            return Ok(false);
        };

        let pipeline = TransferPipeline::new(&self.payload_registry);
        let outcome = pipeline.transfer_between(
            source_domain.as_mut(),
            &source.native_id,
            target_domain.as_mut(),
            &target.native_id,
            dir,
        );
        self.domains.insert(source.domain, source_domain);

        match outcome {
            Ok(TransferOutcome::Applied { merged_native_id }) => {
                logging::debug(format!(
                    "orchestrator: cross-domain transfer applied source_domain={} target_domain={} merged_native_id_len={}",
                    source.domain,
                    target.domain,
                    merged_native_id.len()
                ));
                Ok(true)
            }
            Ok(TransferOutcome::Fallback { reason }) => {
                logging::debug(format!(
                    "orchestrator: cross-domain transfer fallback source_domain={} target_domain={} reason={}",
                    source.domain, target.domain, reason
                ));
                Ok(false)
            }
            Err(err) => {
                logging::debug(format!(
                    "orchestrator: cross-domain transfer error source_domain={} target_domain={} err={:#}",
                    source.domain, target.domain, err
                ));
                Ok(false)
            }
        }
    }

    fn attempt_same_domain_transfer(
        &mut self,
        source: &GlobalLeaf,
        target: &GlobalLeaf,
        dir: Direction,
    ) -> Result<bool> {
        let Some(domain) = self.domains.get_mut(&source.domain) else {
            return Ok(false);
        };
        if domain.supported_payload_types().is_empty() {
            return Ok(false);
        }

        let pipeline = TransferPipeline::new(&self.payload_registry);
        let outcome =
            pipeline.transfer_within(domain.as_mut(), &source.native_id, &target.native_id, dir);

        match outcome {
            Ok(TransferOutcome::Applied { merged_native_id }) => {
                logging::debug(format!(
                    "orchestrator: same-domain transfer applied domain={} merged_native_id_len={}",
                    source.domain,
                    merged_native_id.len()
                ));
                Ok(true)
            }
            Ok(TransferOutcome::Fallback { reason }) => {
                logging::debug(format!(
                    "orchestrator: same-domain transfer fallback domain={} reason={}",
                    source.domain, reason
                ));
                Ok(false)
            }
            Err(err) => {
                logging::debug(format!(
                    "orchestrator: same-domain transfer error domain={} err={:#}",
                    source.domain, err
                ));
                Ok(false)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::any::TypeId;
    use std::collections::BTreeSet;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::sync::Mutex;

    use anyhow::{anyhow, Result};

    use super::{ActionKind, ActionRequest, Orchestrator};
    use crate::engine::actions::{
        cleanup_merged_source_window, focus_tearout_window, place_tearout_window,
        select_tearout_window_id,
    };
    use crate::engine::domain::PaneState;
    use crate::engine::domain::{DomainLeafSnapshot, DomainSnapshot, ErasedDomain};
    use crate::engine::domain::{EDITOR_DOMAIN_ID, TERMINAL_DOMAIN_ID};
    use crate::engine::runtime::ProcessId;
    use crate::engine::topology::Direction;
    use crate::engine::topology::{GlobalLeaf, Rect};
    use crate::engine::window_manager::{
        CapabilitySupport, ConfiguredWindowManager, FocusedWindowRecord, ResizeIntent,
        WindowManagerCapabilities, WindowManagerFeatures, WindowManagerSession, WindowRecord,
        WindowTearOutComposer,
    };

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "yeet-and-yoink-orchestrator-{prefix}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be monotonic")
                .as_nanos()
        ))
    }

    fn load_config(path: &std::path::Path) -> crate::config::Config {
        let old = crate::config::snapshot();
        crate::config::prepare_with_path(Some(path)).expect("config should load");
        old
    }

    fn restore_config(old: crate::config::Config) {
        crate::config::install(old);
    }

    struct SessionState {
        focus_calls: Vec<Direction>,
    }

    struct RecordingSession {
        state: Arc<Mutex<SessionState>>,
    }

    impl WindowManagerSession for RecordingSession {
        fn adapter_name(&self) -> &'static str {
            "fake"
        }

        fn capabilities(&self) -> WindowManagerCapabilities {
            WindowManagerCapabilities::none()
        }

        fn focused_window(&mut self) -> Result<FocusedWindowRecord> {
            Ok(FocusedWindowRecord {
                id: 77,
                app_id: Some("fake-app".into()),
                title: Some("fake-title".into()),
                pid: None,
                original_tile_index: 1,
            })
        }

        fn windows(&mut self) -> Result<Vec<WindowRecord>> {
            Ok(Vec::new())
        }

        fn focus_direction(&mut self, direction: Direction) -> Result<()> {
            self.state
                .lock()
                .expect("session state mutex should not be poisoned")
                .focus_calls
                .push(direction);
            Ok(())
        }

        fn move_direction(&mut self, _direction: Direction) -> Result<()> {
            Ok(())
        }

        fn resize_with_intent(&mut self, _intent: ResizeIntent) -> Result<()> {
            Ok(())
        }

        fn spawn(&mut self, _command: Vec<String>) -> Result<()> {
            Ok(())
        }

        fn focus_window_by_id(&mut self, _id: u64) -> Result<()> {
            Ok(())
        }

        fn close_window_by_id(&mut self, _id: u64) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn route_distinguishes_same_and_cross_domain_targets() {
        let orchestrator = Orchestrator::default();
        let source = GlobalLeaf {
            id: 1,
            domain: 10,
            native_id: vec![1],
            rect: Rect {
                x: 0,
                y: 0,
                w: 100,
                h: 100,
            },
        };
        let same = GlobalLeaf {
            id: 2,
            domain: 10,
            native_id: vec![2],
            rect: Rect {
                x: 200,
                y: 0,
                w: 100,
                h: 100,
            },
        };
        let cross = GlobalLeaf {
            id: 3,
            domain: 11,
            native_id: vec![3],
            rect: Rect {
                x: 200,
                y: 0,
                w: 100,
                h: 100,
            },
        };
        assert_eq!(
            orchestrator.route(&source, &same),
            super::RoutingDecision::SameDomain
        );
        assert_eq!(
            orchestrator.route(&source, &cross),
            super::RoutingDecision::CrossDomain
        );
    }

    fn fake_configured_wm(state: Arc<Mutex<SessionState>>) -> ConfiguredWindowManager {
        ConfiguredWindowManager::new(
            Box::new(RecordingSession { state }),
            WindowManagerFeatures::default(),
        )
    }

    #[test]
    fn orchestrator_uses_object_safe_wm_core_snapshots() {
        let state = Arc::new(Mutex::new(SessionState {
            focus_calls: Vec::new(),
        }));
        let mut wm = fake_configured_wm(state.clone());
        let mut orchestrator = Orchestrator::default();

        orchestrator
            .execute(
                &mut wm,
                ActionRequest {
                    kind: ActionKind::Focus,
                    direction: Direction::East,
                },
            )
            .expect("configured window manager should execute orchestrator actions");

        assert_eq!(
            state
                .lock()
                .expect("session state mutex should not be poisoned")
                .focus_calls,
            vec![Direction::East]
        );
    }

    #[test]
    fn orchestrator_source_targets_configured_window_manager_directly() {
        let source = include_str!("mod.rs");
        let implementation = source
            .split_once("#[cfg(test)]")
            .map(|(implementation, _)| implementation)
            .expect("actions/mod.rs source should include test module");

        assert!(!implementation.contains("trait RuntimeWindowManager"));
        assert!(!implementation.contains("W: RuntimeWindowManager"));
        assert!(implementation.contains("pub fn execute("));
        assert!(implementation.contains("wm: &mut ConfiguredWindowManager"));
    }

    #[test]
    fn cross_domain_test_fakes_do_not_depend_on_runtime_window_manager() {
        let source = include_str!("../../../tests/cross_domain_orchestrator.rs");

        assert!(!source.contains("RuntimeWindowManager"));
    }

    #[derive(Debug)]
    struct BufferPayload;

    #[derive(Debug)]
    struct TerminalPayload;

    #[derive(Clone, Default)]
    struct DomainCounters {
        tear_off_calls: Arc<AtomicUsize>,
        merge_calls: Arc<AtomicUsize>,
        snapshot_calls: Arc<AtomicUsize>,
    }

    struct FakeDomain {
        id: u64,
        name: &'static str,
        supported_payloads: Vec<TypeId>,
        tear_payload: Option<Box<dyn PaneState>>,
        counters: DomainCounters,
    }

    impl FakeDomain {
        fn new(
            id: u64,
            name: &'static str,
            supported_payloads: Vec<TypeId>,
            tear_payload: Option<Box<dyn PaneState>>,
            counters: DomainCounters,
        ) -> Self {
            Self {
                id,
                name,
                supported_payloads,
                tear_payload,
                counters,
            }
        }
    }

    impl ErasedDomain for FakeDomain {
        fn domain_id(&self) -> u64 {
            self.id
        }

        fn domain_name(&self) -> &'static str {
            self.name
        }

        fn rect(&self) -> Rect {
            Rect {
                x: 0,
                y: 0,
                w: 1000,
                h: 1000,
            }
        }

        fn fetch_snapshot(&mut self) -> Result<DomainSnapshot> {
            self.counters.snapshot_calls.fetch_add(1, Ordering::Relaxed);
            Ok(DomainSnapshot {
                domain_id: self.id,
                rect: self.rect(),
                leaves: vec![DomainLeafSnapshot {
                    id: 1,
                    native_id: vec![1],
                    rect: self.rect(),
                    focused: true,
                }],
            })
        }

        fn supported_payload_types(&self) -> Vec<TypeId> {
            self.supported_payloads.clone()
        }

        fn tear_off(&mut self, _native_id: &[u8]) -> Result<Box<dyn PaneState>> {
            self.counters.tear_off_calls.fetch_add(1, Ordering::Relaxed);
            self.tear_payload
                .take()
                .ok_or_else(|| anyhow!("no payload to tear off"))
        }

        fn merge_in(
            &mut self,
            _target_native_id: &[u8],
            _dir: crate::engine::topology::Direction,
            _payload: Box<dyn PaneState>,
        ) -> Result<Vec<u8>> {
            self.counters.merge_calls.fetch_add(1, Ordering::Relaxed);
            Ok(vec![9])
        }
    }

    #[derive(Clone)]
    struct FakeWindowManagerState {
        windows: Vec<WindowRecord>,
        window_snapshots: Vec<Vec<WindowRecord>>,
        windows_call_count: usize,
        capabilities: WindowManagerCapabilities,
        move_calls: usize,
        close_calls: usize,
        closed_window_ids: Vec<u64>,
    }

    impl FakeWindowManagerState {
        fn new(windows: Vec<WindowRecord>, capabilities: WindowManagerCapabilities) -> Self {
            Self {
                windows,
                window_snapshots: Vec::new(),
                windows_call_count: 0,
                capabilities,
                move_calls: 0,
                close_calls: 0,
                closed_window_ids: Vec::new(),
            }
        }
    }

    struct FakeWindowManager {
        state: Arc<Mutex<FakeWindowManagerState>>,
    }

    impl FakeWindowManager {
        fn new(state: Arc<Mutex<FakeWindowManagerState>>) -> Self {
            Self { state }
        }
    }

    impl WindowManagerSession for FakeWindowManager {
        fn adapter_name(&self) -> &'static str {
            "fake"
        }

        fn capabilities(&self) -> WindowManagerCapabilities {
            self.state
                .lock()
                .expect("fake wm state mutex should not be poisoned")
                .capabilities
        }

        fn focused_window(&mut self) -> Result<FocusedWindowRecord> {
            let state = self
                .state
                .lock()
                .expect("fake wm state mutex should not be poisoned");
            let focused = state
                .windows
                .iter()
                .find(|window| window.is_focused)
                .ok_or_else(|| anyhow!("no focused window"))?;
            Ok(FocusedWindowRecord {
                id: focused.id,
                app_id: focused.app_id.clone(),
                title: focused.title.clone(),
                pid: focused.pid,
                original_tile_index: focused.original_tile_index,
            })
        }

        fn windows(&mut self) -> Result<Vec<WindowRecord>> {
            let mut state = self
                .state
                .lock()
                .expect("fake wm state mutex should not be poisoned");
            if let Some(snapshot) = state
                .window_snapshots
                .get(state.windows_call_count)
                .cloned()
            {
                state.windows = snapshot;
            }
            state.windows_call_count += 1;
            Ok(state.windows.clone())
        }

        fn focus_direction(&mut self, _direction: Direction) -> Result<()> {
            let mut state = self
                .state
                .lock()
                .expect("fake wm state mutex should not be poisoned");
            if state.windows.len() < 2 {
                return Ok(());
            }
            let focused_idx = state
                .windows
                .iter()
                .position(|window| window.is_focused)
                .ok_or_else(|| anyhow!("no focused window"))?;
            let target_idx = if focused_idx + 1 < state.windows.len() {
                focused_idx + 1
            } else {
                focused_idx.saturating_sub(1)
            };
            for (idx, window) in state.windows.iter_mut().enumerate() {
                window.is_focused = idx == target_idx;
            }
            Ok(())
        }

        fn move_direction(&mut self, _direction: Direction) -> Result<()> {
            self.state
                .lock()
                .expect("fake wm state mutex should not be poisoned")
                .move_calls += 1;
            Ok(())
        }

        fn resize_with_intent(&mut self, _intent: ResizeIntent) -> Result<()> {
            Ok(())
        }

        fn spawn(&mut self, _command: Vec<String>) -> Result<()> {
            Ok(())
        }

        fn focus_window_by_id(&mut self, id: u64) -> Result<()> {
            let mut state = self
                .state
                .lock()
                .expect("fake wm state mutex should not be poisoned");
            let mut matched = false;
            for window in &mut state.windows {
                if window.id == id {
                    window.is_focused = true;
                    matched = true;
                } else {
                    window.is_focused = false;
                }
            }
            if !matched {
                return Err(anyhow!("window id {id} not found"));
            }
            Ok(())
        }

        fn close_window_by_id(&mut self, id: u64) -> Result<()> {
            let mut state = self
                .state
                .lock()
                .expect("fake wm state mutex should not be poisoned");
            let original_len = state.windows.len();
            state.windows.retain(|window| window.id != id);
            if state.windows.len() == original_len {
                return Err(anyhow!("window id {id} not found"));
            }
            state.close_calls += 1;
            state.closed_window_ids.push(id);
            Ok(())
        }
    }

    #[derive(Default)]
    struct ComposerState {
        calls: Vec<(String, usize)>,
    }

    struct FakeTearOutComposer {
        state: Arc<Mutex<ComposerState>>,
    }

    impl WindowTearOutComposer for FakeTearOutComposer {
        fn compose_tear_out(
            &mut self,
            direction: Direction,
            source_tile_index: usize,
        ) -> Result<()> {
            self.state
                .lock()
                .expect("composer state mutex should not be poisoned")
                .calls
                .push((direction.to_string(), source_tile_index));
            Ok(())
        }
    }

    struct TestConfiguredWindowManager {
        wm: ConfiguredWindowManager,
        state: Arc<Mutex<FakeWindowManagerState>>,
        composer_state: Option<Arc<Mutex<ComposerState>>>,
    }

    impl TestConfiguredWindowManager {
        fn new(
            state: FakeWindowManagerState,
            features: WindowManagerFeatures,
            composer_state: Option<Arc<Mutex<ComposerState>>>,
        ) -> Self {
            let state = Arc::new(Mutex::new(state));
            let wm = ConfiguredWindowManager::new(
                Box::new(FakeWindowManager::new(state.clone())),
                features,
            );
            Self {
                wm,
                state,
                composer_state,
            }
        }

        fn snapshot(&self) -> FakeWindowManagerState {
            self.state
                .lock()
                .expect("fake wm state mutex should not be poisoned")
                .clone()
        }

        fn take_composer_calls(&mut self) -> Vec<(String, usize)> {
            let mut state = self
                .composer_state
                .as_ref()
                .expect("composer state should be present")
                .lock()
                .expect("composer state mutex should not be poisoned");
            std::mem::take(&mut state.calls)
        }
    }

    fn fake_wm(state: FakeWindowManagerState) -> TestConfiguredWindowManager {
        TestConfiguredWindowManager::new(state, WindowManagerFeatures::default(), None)
    }

    fn fake_wm_with_tearout_composer(direction: Direction) -> TestConfiguredWindowManager {
        let composer_state = Arc::new(Mutex::new(ComposerState::default()));
        let caps = composed_tearout_capabilities_for(direction);

        let mut features = WindowManagerFeatures::default();
        features.tear_out_composer = Some(Box::new(FakeTearOutComposer {
            state: composer_state.clone(),
        }));

        TestConfiguredWindowManager::new(
            FakeWindowManagerState::new(
                vec![
                    WindowRecord {
                        id: 11,
                        app_id: Some("org.wezfurlong.wezterm".into()),
                        title: Some("source".into()),
                        pid: ProcessId::new(1),
                        is_focused: false,
                        original_tile_index: 1,
                    },
                    WindowRecord {
                        id: 12,
                        app_id: Some("org.wezfurlong.wezterm".into()),
                        title: Some("tearout".into()),
                        pid: ProcessId::new(2),
                        is_focused: true,
                        original_tile_index: 1,
                    },
                ],
                caps,
            ),
            features,
            Some(composer_state),
        )
    }

    #[test]
    fn move_prefers_cross_domain_transfer_when_payloads_are_compatible() {
        let _guard = crate::utils::env_guard();
        let root = unique_temp_dir("cross-domain-transfer");
        let config_dir = root.join("yeet-and-yoink");
        std::fs::create_dir_all(&config_dir).expect("config dir should be created");
        std::fs::write(
            config_dir.join("config.toml"),
            r#"
[app.terminal.wezterm]
enabled = true

[app.editor.emacs]
enabled = true
"#,
        )
        .expect("config file should be writable");
        let old_config = load_config(&config_dir.join("config.toml"));
        let mut orchestrator = Orchestrator::default();

        let source_counters = DomainCounters::default();
        let target_counters = DomainCounters::default();
        orchestrator.register_domain(Box::new(FakeDomain::new(
            TERMINAL_DOMAIN_ID,
            "source",
            vec![TypeId::of::<BufferPayload>()],
            Some(Box::new(BufferPayload)),
            source_counters.clone(),
        )));
        orchestrator.register_domain(Box::new(FakeDomain::new(
            EDITOR_DOMAIN_ID,
            "target",
            vec![TypeId::of::<BufferPayload>()],
            None,
            target_counters.clone(),
        )));

        let mut wm = fake_wm(FakeWindowManagerState {
            windows: vec![
                WindowRecord {
                    id: 101,
                    app_id: Some("org.wezfurlong.wezterm".into()),
                    title: Some("source".into()),
                    pid: None,
                    is_focused: true,
                    original_tile_index: 1,
                },
                WindowRecord {
                    id: 202,
                    app_id: Some("emacs".into()),
                    title: Some("target".into()),
                    pid: None,
                    is_focused: false,
                    original_tile_index: 2,
                },
            ],
            window_snapshots: Vec::new(),
            windows_call_count: 0,
            capabilities: WindowManagerCapabilities::none(),
            move_calls: 0,
            close_calls: 0,
            closed_window_ids: Vec::new(),
        });

        orchestrator
            .execute(
                &mut wm.wm,
                ActionRequest {
                    kind: ActionKind::Move,
                    direction: crate::engine::topology::Direction::East,
                },
            )
            .expect("move should succeed");

        assert_eq!(
            wm.snapshot().move_calls,
            0,
            "wm fallback should not run when transfer applies"
        );
        assert_eq!(source_counters.tear_off_calls.load(Ordering::Relaxed), 1);
        assert_eq!(target_counters.merge_calls.load(Ordering::Relaxed), 1);
        assert!(
            source_counters.snapshot_calls.load(Ordering::Relaxed) > 0,
            "source domain should resync after mutation"
        );
        assert!(
            target_counters.snapshot_calls.load(Ordering::Relaxed) > 0,
            "target domain should resync after mutation"
        );

        restore_config(old_config);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn move_falls_back_to_wm_when_transfer_has_no_compatible_payload() {
        let _guard = crate::utils::env_guard();
        let root = unique_temp_dir("wm-fallback");
        let config_dir = root.join("yeet-and-yoink");
        std::fs::create_dir_all(&config_dir).expect("config dir should be created");
        std::fs::write(
            config_dir.join("config.toml"),
            r#"
[app.terminal.wezterm]
enabled = true

[app.editor.emacs]
enabled = true
"#,
        )
        .expect("config file should be writable");
        let old_config = load_config(&config_dir.join("config.toml"));
        let mut orchestrator = Orchestrator::default();

        let source_counters = DomainCounters::default();
        let target_counters = DomainCounters::default();
        orchestrator.register_domain(Box::new(FakeDomain::new(
            TERMINAL_DOMAIN_ID,
            "source",
            vec![TypeId::of::<BufferPayload>()],
            Some(Box::new(BufferPayload)),
            source_counters.clone(),
        )));
        orchestrator.register_domain(Box::new(FakeDomain::new(
            EDITOR_DOMAIN_ID,
            "target",
            vec![TypeId::of::<TerminalPayload>()],
            None,
            target_counters.clone(),
        )));

        let mut wm = fake_wm(FakeWindowManagerState {
            windows: vec![
                WindowRecord {
                    id: 101,
                    app_id: Some("org.wezfurlong.wezterm".into()),
                    title: Some("source".into()),
                    pid: None,
                    is_focused: true,
                    original_tile_index: 1,
                },
                WindowRecord {
                    id: 202,
                    app_id: Some("emacs".into()),
                    title: Some("target".into()),
                    pid: None,
                    is_focused: false,
                    original_tile_index: 2,
                },
            ],
            window_snapshots: Vec::new(),
            windows_call_count: 0,
            capabilities: WindowManagerCapabilities::none(),
            move_calls: 0,
            close_calls: 0,
            closed_window_ids: Vec::new(),
        });

        orchestrator
            .execute(
                &mut wm.wm,
                ActionRequest {
                    kind: ActionKind::Move,
                    direction: crate::engine::topology::Direction::East,
                },
            )
            .expect("move should still succeed via fallback");

        assert_eq!(
            wm.snapshot().move_calls,
            1,
            "wm fallback should run when transfer is incompatible"
        );
        assert_eq!(source_counters.tear_off_calls.load(Ordering::Relaxed), 1);
        assert_eq!(target_counters.merge_calls.load(Ordering::Relaxed), 0);

        restore_config(old_config);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn move_merges_within_same_domain_when_supported() {
        let _guard = crate::utils::env_guard();
        let root = unique_temp_dir("same-domain-merge");
        let config_dir = root.join("yeet-and-yoink");
        std::fs::create_dir_all(&config_dir).expect("config dir should be created");
        std::fs::write(
            config_dir.join("config.toml"),
            r#"
[app.terminal.wezterm]
enabled = true
"#,
        )
        .expect("config file should be writable");
        let old_config = load_config(&config_dir.join("config.toml"));
        let mut orchestrator = Orchestrator::default();

        let counters = DomainCounters::default();
        orchestrator.register_domain(Box::new(FakeDomain::new(
            TERMINAL_DOMAIN_ID,
            "terminal",
            vec![TypeId::of::<BufferPayload>()],
            Some(Box::new(BufferPayload)),
            counters.clone(),
        )));

        let mut wm = fake_wm(FakeWindowManagerState {
            windows: vec![
                WindowRecord {
                    id: 101,
                    app_id: Some("org.wezfurlong.wezterm".into()),
                    title: Some("source".into()),
                    pid: None,
                    is_focused: true,
                    original_tile_index: 1,
                },
                WindowRecord {
                    id: 202,
                    app_id: Some("org.wezfurlong.wezterm".into()),
                    title: Some("target".into()),
                    pid: None,
                    is_focused: false,
                    original_tile_index: 2,
                },
            ],
            window_snapshots: Vec::new(),
            windows_call_count: 0,
            capabilities: WindowManagerCapabilities::none(),
            move_calls: 0,
            close_calls: 0,
            closed_window_ids: Vec::new(),
        });

        orchestrator
            .execute(
                &mut wm.wm,
                ActionRequest {
                    kind: ActionKind::Move,
                    direction: crate::engine::topology::Direction::East,
                },
            )
            .expect("move should merge within same domain");

        assert_eq!(wm.snapshot().move_calls, 0, "wm fallback should not run");
        assert_eq!(counters.tear_off_calls.load(Ordering::Relaxed), 1);
        assert_eq!(counters.merge_calls.load(Ordering::Relaxed), 1);
        assert!(
            counters.snapshot_calls.load(Ordering::Relaxed) > 0,
            "domain should resync after within-domain transfer"
        );

        restore_config(old_config);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn cleanup_merged_source_window_closes_source_and_keeps_target_focused() {
        let mut wm = fake_wm(FakeWindowManagerState {
            windows: vec![
                WindowRecord {
                    id: 301,
                    app_id: Some("kitty".into()),
                    title: Some("source".into()),
                    pid: ProcessId::new(1),
                    is_focused: true,
                    original_tile_index: 1,
                },
                WindowRecord {
                    id: 302,
                    app_id: Some("kitty".into()),
                    title: Some("target".into()),
                    pid: ProcessId::new(2),
                    is_focused: false,
                    original_tile_index: 2,
                },
            ],
            window_snapshots: Vec::new(),
            windows_call_count: 0,
            capabilities: WindowManagerCapabilities::none(),
            move_calls: 0,
            close_calls: 0,
            closed_window_ids: Vec::new(),
        });

        cleanup_merged_source_window(&mut wm.wm, 301, 302, "terminal");

        let state = wm.snapshot();
        assert_eq!(state.close_calls, 1);
        assert_eq!(state.closed_window_ids, vec![301]);
        assert_eq!(state.windows.len(), 1);
        assert_eq!(state.windows[0].id, 302);
        assert!(state.windows[0].is_focused);
    }

    fn composed_tearout_capabilities_for(direction: Direction) -> WindowManagerCapabilities {
        let mut caps = WindowManagerCapabilities::none();
        caps.primitives.tear_out_right = true;
        caps.primitives.move_column = true;
        caps.primitives.consume_into_column_and_move = true;
        match direction {
            Direction::West => caps.tear_out.west = CapabilitySupport::Composed,
            Direction::East => caps.tear_out.east = CapabilitySupport::Composed,
            Direction::North => caps.tear_out.north = CapabilitySupport::Composed,
            Direction::South => caps.tear_out.south = CapabilitySupport::Composed,
        }
        caps
    }

    #[test]
    fn place_tearout_window_moves_column_for_composed_west() {
        let mut wm = fake_wm_with_tearout_composer(Direction::West);

        place_tearout_window(&mut wm.wm, Direction::West, 11, 4, None)
            .expect("tearout placement should succeed");
        assert_eq!(wm.take_composer_calls(), vec![("west".into(), 4)]);
    }

    #[test]
    fn composed_tearout_routes_through_wm_specific_composer() {
        let mut wm = fake_wm_with_tearout_composer(Direction::North);

        place_tearout_window(&mut wm.wm, Direction::North, 11, 3, None)
            .expect("tearout placement should succeed");

        assert_eq!(wm.take_composer_calls(), vec![("north".into(), 3)]);
    }

    #[test]
    fn place_tearout_window_consumes_for_composed_north() {
        let mut wm = fake_wm_with_tearout_composer(Direction::North);

        place_tearout_window(&mut wm.wm, Direction::North, 11, 7, None)
            .expect("tearout placement should succeed");
        assert_eq!(wm.take_composer_calls(), vec![("north".into(), 7)]);
    }

    #[test]
    fn focus_tearout_window_retries_until_new_window_appears() {
        let source_pid = ProcessId::new(5151);
        let mut pre_window_ids = BTreeSet::new();
        pre_window_ids.insert(31);

        let source_window = WindowRecord {
            id: 31,
            app_id: Some("com.mitchellh.ghostty".into()),
            title: Some("source".into()),
            pid: source_pid,
            is_focused: true,
            original_tile_index: 0,
        };
        let tearout_window = WindowRecord {
            id: 32,
            app_id: Some("com.mitchellh.ghostty".into()),
            title: Some("tearout".into()),
            pid: ProcessId::new(6161),
            is_focused: false,
            original_tile_index: 0,
        };
        let mut wm = fake_wm(FakeWindowManagerState {
            windows: vec![source_window.clone()],
            window_snapshots: vec![
                vec![source_window.clone()],
                vec![source_window.clone(), tearout_window],
            ],
            windows_call_count: 0,
            capabilities: WindowManagerCapabilities::none(),
            move_calls: 0,
            close_calls: 0,
            closed_window_ids: Vec::new(),
        });

        let focused = focus_tearout_window(
            &mut wm.wm,
            &pre_window_ids,
            31,
            source_pid,
            "com.mitchellh.ghostty",
        )
        .expect("tearout focus should succeed");

        let state = wm.snapshot();
        assert_eq!(focused, Some(32));
        assert_eq!(state.windows_call_count, 2);
        assert_eq!(
            state
                .windows
                .iter()
                .find(|window| window.is_focused)
                .map(|window| window.id),
            Some(32)
        );
    }

    #[test]
    fn place_tearout_window_focuses_known_target_before_composed_north() {
        let mut wm = fake_wm_with_tearout_composer(Direction::North);
        place_tearout_window(&mut wm.wm, Direction::North, 11, 9, Some(12))
            .expect("tearout placement should succeed");

        let state = wm.snapshot();
        assert_eq!(wm.take_composer_calls(), vec![("north".into(), 9)]);
        assert_eq!(
            state
                .windows
                .iter()
                .find(|window| window.is_focused)
                .map(|window| window.id),
            Some(12)
        );
    }

    #[test]
    fn select_tearout_window_prefers_new_same_app_and_pid() {
        let mut pre_window_ids = BTreeSet::new();
        pre_window_ids.insert(10);
        let source_pid = ProcessId::new(4242);
        let windows = vec![
            WindowRecord {
                id: 10,
                app_id: Some("org.wezfurlong.wezterm".into()),
                title: Some("source".into()),
                pid: source_pid,
                is_focused: false,
                original_tile_index: 1,
            },
            WindowRecord {
                id: 11,
                app_id: Some("org.wezfurlong.wezterm".into()),
                title: Some("tearout".into()),
                pid: source_pid,
                is_focused: false,
                original_tile_index: 1,
            },
            WindowRecord {
                id: 12,
                app_id: Some("emacs".into()),
                title: Some("other".into()),
                pid: ProcessId::new(1111),
                is_focused: true,
                original_tile_index: 1,
            },
        ];

        let selected = select_tearout_window_id(
            &pre_window_ids,
            &windows,
            10,
            source_pid,
            "org.wezfurlong.wezterm",
        );
        assert_eq!(selected, Some(11));
    }

    #[test]
    fn select_tearout_window_falls_back_to_focused_when_no_new_window_detected() {
        let mut pre_window_ids = BTreeSet::new();
        pre_window_ids.insert(20);
        pre_window_ids.insert(21);
        let windows = vec![
            WindowRecord {
                id: 20,
                app_id: Some("org.wezfurlong.wezterm".into()),
                title: Some("source".into()),
                pid: ProcessId::new(4242),
                is_focused: false,
                original_tile_index: 1,
            },
            WindowRecord {
                id: 21,
                app_id: Some("org.wezfurlong.wezterm".into()),
                title: Some("target".into()),
                pid: ProcessId::new(4242),
                is_focused: true,
                original_tile_index: 2,
            },
        ];

        let selected = select_tearout_window_id(
            &pre_window_ids,
            &windows,
            20,
            ProcessId::new(4242),
            "org.wezfurlong.wezterm",
        );
        assert_eq!(selected, Some(21));
    }
}
