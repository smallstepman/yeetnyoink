use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use anyhow::{Context, Result};

use crate::adapters::apps::{self, DeepApp, MergeExecutionMode, MoveDecision};
use crate::adapters::window_managers::{
    plan_tear_out, CapabilitySupport, FocusedWindowView, ResizeIntent, ResizeKind,
    WindowManagerAdapter, WindowRecord,
};
use crate::engine::direction::Direction;
use crate::engine::domain::ErasedDomain;
use crate::engine::domain_plugins::{
    decode_native_window_ref, domain_id_for_window, domain_name_for_id, encode_native_window_ref,
};
use crate::engine::pane_state::PayloadRegistry;
use crate::engine::runtime::ProcessId;
use crate::engine::topology::{
    find_neighbor, Cardinal, DomainId, DomainNode, GlobalDomainTree, GlobalLeaf, GlobalTopology,
    Rect,
};
use crate::engine::transfer::{TransferOutcome, TransferPipeline};
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
    pub direction: Cardinal,
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

    pub fn execute<W>(&mut self, wm: &mut W, request: ActionRequest) -> Result<()>
    where
        W: WindowManagerAdapter,
    {
        match request.kind {
            ActionKind::Focus => self.execute_focus(wm, request.direction),
            ActionKind::Move => self.execute_move(wm, request.direction),
            ActionKind::Resize { grow, step } => {
                self.execute_resize(wm, request.direction, grow, step)
            }
        }
    }

    pub fn execute_focus<W>(&mut self, wm: &mut W, dir: Cardinal) -> Result<()>
    where
        W: WindowManagerAdapter,
    {
        let fallback_dir = dir.into();
        if self.attempt_focused_app_focus(wm, fallback_dir)? {
            return Ok(());
        }
        let topology = self.snapshot_from_wm(wm)?;
        let Some((focused, target)) = self.focused_and_target(&topology, dir) else {
            return wm.focus_direction(fallback_dir);
        };

        match self.route(focused, target) {
            RoutingDecision::SameDomain => {
                if let Some(target_ref) = decode_native_window_ref(&target.native_id) {
                    wm.focus_window_by_id(target_ref.window_id)?;
                    return Ok(());
                }
                wm.focus_direction(fallback_dir)
            }
            RoutingDecision::CrossDomain => wm.focus_direction(fallback_dir),
        }
    }

    fn attempt_focused_app_focus<W>(&mut self, wm: &mut W, dir: Direction) -> Result<bool>
    where
        W: WindowManagerAdapter,
    {
        let (app_id, title, source_pid) = wm.with_focused_window(|window| {
            Ok((
                window.app_id().unwrap_or("").to_string(),
                window.title().unwrap_or("").to_string(),
                window.pid(),
            ))
        })?;
        let owner_pid = source_pid.map(ProcessId::get);
        let Some(owner_pid) = owner_pid else {
            return Ok(false);
        };

        for app in apps::resolve_chain(&app_id, owner_pid, &title) {
            if !app.capabilities().focus {
                continue;
            }
            let adapter_name = app.adapter_name();
            if app
                .can_focus(dir, owner_pid)
                .with_context(|| format!("{adapter_name} can_focus failed"))?
            {
                app.focus(dir, owner_pid)
                    .with_context(|| format!("{adapter_name} focus failed"))?;
                logging::debug(format!("orchestrator: app focus handled by {adapter_name}"));
                return Ok(true);
            }
        }

        Ok(false)
    }

    pub fn execute_move<W>(&mut self, wm: &mut W, dir: Cardinal) -> Result<()>
    where
        W: WindowManagerAdapter,
    {
        let fallback_dir = dir.into();
        if self.attempt_focused_app_move(wm, fallback_dir)? {
            return Ok(());
        }
        let topology = self.snapshot_from_wm(wm)?;
        let Some((focused, target)) = self.focused_and_target(&topology, dir) else {
            return wm.move_direction(fallback_dir);
        };

        match self.route(focused, target) {
            RoutingDecision::SameDomain => {
                if self
                    .attempt_same_domain_transfer(focused, target, dir)
                    .unwrap_or(false)
                {
                    Ok(())
                } else {
                    wm.move_direction(fallback_dir)
                }
            }
            RoutingDecision::CrossDomain => {
                if self
                    .attempt_cross_domain_transfer(focused, target, dir)
                    .unwrap_or(false)
                {
                    Ok(())
                } else {
                    let err = RoutingError::UnsupportedTransfer {
                        source_domain: focused.domain,
                        target_domain: target.domain,
                    };
                    logging::debug(format!("orchestrator: {:?}", err));
                    wm.move_direction(fallback_dir)
                }
            }
        }
    }

    fn attempt_focused_app_move<W>(&mut self, wm: &mut W, dir: Direction) -> Result<bool>
    where
        W: WindowManagerAdapter,
    {
        let (source_window_id, source_tile_index, app_id, title, source_pid) = wm
            .with_focused_window(|window| {
                Ok((
                    window.id(),
                    window.original_tile_index(),
                    window.app_id().unwrap_or("").to_string(),
                    window.title().unwrap_or("").to_string(),
                    window.pid(),
                ))
            })?;
        let owner_pid = source_pid.map(ProcessId::get);
        let Some(owner_pid) = owner_pid else {
            return Ok(false);
        };

        for app in apps::resolve_chain(&app_id, owner_pid, &title) {
            let adapter_name = app.adapter_name();
            let decision = app
                .move_decision(dir, owner_pid)
                .with_context(|| format!("{adapter_name} move_decision failed"))?;
            match decision {
                MoveDecision::Passthrough => {
                    if self.attempt_passthrough_merge(
                        wm,
                        app.as_ref(),
                        dir,
                        source_window_id,
                        source_pid,
                    )? {
                        return Ok(true);
                    }
                }
                MoveDecision::Internal => {
                    app.move_internal(dir, owner_pid)
                        .with_context(|| format!("{adapter_name} move_internal failed"))?;
                    logging::debug(format!(
                        "orchestrator: app move handled by {adapter_name} decision=Internal"
                    ));
                    return Ok(true);
                }
                MoveDecision::Rearrange => {
                    app.rearrange(dir, owner_pid)
                        .with_context(|| format!("{adapter_name} rearrange failed"))?;
                    logging::debug(format!(
                        "orchestrator: app move handled by {adapter_name} decision=Rearrange"
                    ));
                    return Ok(true);
                }
                MoveDecision::TearOut => {
                    let pre_window_ids: BTreeSet<u64> = match wm.windows() {
                        Ok(windows) => windows.into_iter().map(|window| window.id).collect(),
                        Err(err) => {
                            logging::debug(format!(
                                "orchestrator: unable to snapshot pre-tearout windows err={:#}",
                                err
                            ));
                            BTreeSet::new()
                        }
                    };
                    let tear = app
                        .move_out(dir, owner_pid)
                        .with_context(|| format!("{adapter_name} move_out failed"))?;
                    let has_spawn_command = tear.spawn_command.is_some();
                    if let Some(command) = tear.spawn_command {
                        wm.spawn(command).with_context(|| {
                            format!("{adapter_name} tear-out spawn via wm failed")
                        })?;
                    }
                    if !has_spawn_command {
                        if let Err(err) = self.focus_tearout_window(
                            wm,
                            &pre_window_ids,
                            source_window_id,
                            source_pid,
                            &app_id,
                        ) {
                            logging::debug(format!(
                                "orchestrator: unable to focus tear-out window adapter={} err={:#}",
                                adapter_name, err
                            ));
                        }
                        if let Err(err) =
                            self.place_tearout_window(wm, dir, source_window_id, source_tile_index)
                        {
                            logging::debug(format!(
                                "orchestrator: tear-out placement fallback failed adapter={} err={:#}",
                                adapter_name, err
                            ));
                        }
                    }
                    logging::debug(format!(
                        "orchestrator: app move handled by {adapter_name} decision=TearOut"
                    ));
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    fn focus_tearout_window<W>(
        &self,
        wm: &mut W,
        pre_window_ids: &BTreeSet<u64>,
        source_window_id: u64,
        source_pid: Option<ProcessId>,
        source_app_id: &str,
    ) -> Result<()>
    where
        W: WindowManagerAdapter,
    {
        const ATTEMPTS: usize = 8;
        const DELAY: Duration = Duration::from_millis(20);

        for attempt in 0..ATTEMPTS {
            match wm.windows() {
                Ok(windows) => {
                    if let Some(target_window_id) = Self::select_tearout_window_id(
                        pre_window_ids,
                        &windows,
                        source_window_id,
                        source_pid,
                        source_app_id,
                    ) {
                        if target_window_id != source_window_id {
                            wm.focus_window_by_id(target_window_id)?;
                            return Ok(());
                        }
                    }
                }
                Err(err) => {
                    logging::debug(format!(
                        "orchestrator: tear-out post-window snapshot failed attempt={} err={:#}",
                        attempt + 1,
                        err
                    ));
                }
            }

            if attempt + 1 < ATTEMPTS {
                std::thread::sleep(DELAY);
            }
        }

        Ok(())
    }

    fn select_tearout_window_id(
        pre_window_ids: &BTreeSet<u64>,
        windows: &[WindowRecord],
        source_window_id: u64,
        source_pid: Option<ProcessId>,
        source_app_id: &str,
    ) -> Option<u64> {
        let mut new_windows: Vec<&WindowRecord> = windows
            .iter()
            .filter(|window| !pre_window_ids.contains(&window.id))
            .collect();
        if new_windows.is_empty() {
            return windows
                .iter()
                .find(|window| window.is_focused && window.id != source_window_id)
                .map(|window| window.id);
        }
        new_windows.sort_by_key(|window| window.id);

        new_windows
            .iter()
            .find(|window| {
                window.pid == source_pid && window.app_id.as_deref() == Some(source_app_id)
            })
            .map(|window| window.id)
            .or_else(|| {
                new_windows
                    .iter()
                    .find(|window| window.pid == source_pid)
                    .map(|window| window.id)
            })
            .or_else(|| {
                new_windows
                    .iter()
                    .find(|window| window.app_id.as_deref() == Some(source_app_id))
                    .map(|window| window.id)
            })
            .or_else(|| {
                new_windows
                    .iter()
                    .find(|window| window.is_focused)
                    .map(|window| window.id)
            })
            .or_else(|| new_windows.first().map(|window| window.id))
    }

    fn place_tearout_window<W>(
        &self,
        wm: &mut W,
        dir: Direction,
        source_window_id: u64,
        source_tile_index: usize,
    ) -> Result<()>
    where
        W: WindowManagerAdapter,
    {
        let focused_window_id = wm.with_focused_window(|window| Ok(window.id()))?;
        if focused_window_id == source_window_id {
            return Ok(());
        }

        match plan_tear_out(wm.capabilities(), dir) {
            CapabilitySupport::Native | CapabilitySupport::Unsupported => Ok(()),
            CapabilitySupport::Composed => match dir {
                Direction::West | Direction::East => wm.move_column(dir),
                Direction::North | Direction::South => {
                    wm.consume_into_column_and_move(dir, source_tile_index)
                }
            },
        }
    }

    fn attempt_passthrough_merge<W>(
        &mut self,
        wm: &mut W,
        app: &dyn DeepApp,
        dir: Direction,
        source_window_id: u64,
        source_pid: Option<ProcessId>,
    ) -> Result<bool>
    where
        W: WindowManagerAdapter,
    {
        if !app.capabilities().merge {
            return Ok(false);
        }
        let adapter_name = app.adapter_name();
        let preparation = match app.prepare_merge(source_pid) {
            Ok(value) => value,
            Err(err) => {
                logging::debug(format!(
                    "orchestrator: app passthrough merge prepare failed adapter={} err={:#}",
                    adapter_name, err
                ));
                return Ok(false);
            }
        };

        match app.merge_execution_mode() {
            MergeExecutionMode::SourceFocused => {
                match app.merge_into_target(dir, source_pid, None, preparation) {
                    Ok(()) => {
                        logging::debug(format!(
                            "orchestrator: app move handled by {adapter_name} decision=MergeSourceFocused"
                        ));
                        Ok(true)
                    }
                    Err(err) => {
                        logging::debug(format!(
                            "orchestrator: app passthrough merge failed adapter={} err={:#}",
                            adapter_name, err
                        ));
                        Ok(false)
                    }
                }
            }
            MergeExecutionMode::TargetFocused => {
                if let Err(err) = wm.focus_direction(dir) {
                    logging::debug(format!(
                        "orchestrator: app passthrough merge focus probe failed adapter={} err={:#}",
                        adapter_name, err
                    ));
                    return Ok(false);
                }

                let (target_window_id, target_pid) =
                    wm.with_focused_window(|window| Ok((window.id(), window.pid())))?;
                if target_window_id == source_window_id {
                    return Ok(false);
                }

                match app.merge_into_target(dir, source_pid, target_pid, preparation) {
                    Ok(()) => {
                        logging::debug(format!(
                            "orchestrator: app move handled by {adapter_name} decision=MergeTargetFocused"
                        ));
                        Ok(true)
                    }
                    Err(err) => {
                        let _ = wm.focus_window_by_id(source_window_id);
                        logging::debug(format!(
                            "orchestrator: app passthrough merge failed adapter={} err={:#}",
                            adapter_name, err
                        ));
                        Ok(false)
                    }
                }
            }
        }
    }

    pub fn execute_resize<W>(
        &mut self,
        wm: &mut W,
        dir: Cardinal,
        grow: bool,
        step: i32,
    ) -> Result<()>
    where
        W: WindowManagerAdapter,
    {
        let intent = ResizeIntent::new(
            dir.into(),
            if grow {
                ResizeKind::Grow
            } else {
                ResizeKind::Shrink
            },
            step.max(1),
        );
        let result = wm.resize_with_intent(intent);
        let _ = self.snapshot_from_wm(wm);
        result
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
        dir: Cardinal,
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
        dir: Cardinal,
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

    fn focused_and_target<'a>(
        &self,
        topology: &'a GlobalTopology,
        dir: Cardinal,
    ) -> Option<(&'a GlobalLeaf, &'a GlobalLeaf)> {
        let focused_id = topology.focused_leaf?;
        let focused = topology.leaves.iter().find(|leaf| leaf.id == focused_id)?;
        let target = find_neighbor(&topology.leaves, focused, dir)?;
        Some((focused, target))
    }

    fn snapshot_from_wm<W>(&self, wm: &mut W) -> Result<GlobalTopology>
    where
        W: WindowManagerAdapter,
    {
        let windows = wm.windows()?;
        let mut domain_labels = BTreeMap::<DomainId, String>::new();
        let mut leaves = Vec::<GlobalLeaf>::new();
        let mut focused = None;

        for (idx, window) in windows.iter().enumerate() {
            let domain = domain_id_for_window(
                window.app_id.as_deref(),
                window.pid,
                window.title.as_deref(),
            );
            domain_labels
                .entry(domain)
                .or_insert_with(|| domain_name_for_id(domain).to_string());

            let leaf_id = (idx as u64) + 1;
            if window.is_focused {
                focused = Some(leaf_id);
            }
            let x = (idx as i32) * 1000;
            leaves.push(GlobalLeaf {
                id: leaf_id,
                domain,
                native_id: encode_native_window_ref(window.id, window.pid),
                rect: Rect {
                    x,
                    y: 0,
                    w: 900,
                    h: 900,
                },
            });
        }

        if focused.is_none() {
            let focused_window_id = wm.with_focused_window(|window| Ok(window.id()))?;
            focused = leaves
                .iter()
                .find(|leaf| {
                    decode_native_window_ref(&leaf.native_id)
                        .map(|window| window.window_id == focused_window_id)
                        .unwrap_or(false)
                })
                .map(|leaf| leaf.id);
        }

        let mut domains = Vec::new();
        for (id, name) in domain_labels {
            domains.push(DomainNode {
                id,
                parent: None,
                rect: Rect {
                    x: 0,
                    y: 0,
                    w: 10000,
                    h: 10000,
                },
                name,
            });
        }

        Ok(GlobalTopology {
            tree: GlobalDomainTree { domains },
            leaves,
            focused_leaf: focused,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::any::TypeId;
    use std::collections::BTreeSet;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use anyhow::{anyhow, Result};

    use super::{ActionKind, ActionRequest, Orchestrator};
    use crate::adapters::window_managers::{
        FocusedWindowView, WindowManagerCapabilities, WindowManagerExecution,
        WindowManagerIntrospection, WindowManagerMetadata, WindowRecord,
    };
    use crate::engine::direction::Direction;
    use crate::engine::domain::{DomainLeafSnapshot, DomainSnapshot, ErasedDomain};
    use crate::engine::domain_plugins::{EDITOR_DOMAIN_ID, TERMINAL_DOMAIN_ID};
    use crate::engine::pane_state::PaneState;
    use crate::engine::runtime::ProcessId;
    use crate::engine::topology::{GlobalLeaf, Rect};

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
            _dir: crate::engine::topology::Cardinal,
            _payload: Box<dyn PaneState>,
        ) -> Result<Vec<u8>> {
            self.counters.merge_calls.fetch_add(1, Ordering::Relaxed);
            Ok(vec![9])
        }
    }

    #[derive(Clone, Copy)]
    struct FakeFocusedWindow<'a> {
        inner: &'a WindowRecord,
    }

    impl FocusedWindowView for FakeFocusedWindow<'_> {
        fn id(&self) -> u64 {
            self.inner.id
        }

        fn app_id(&self) -> Option<&str> {
            self.inner.app_id.as_deref()
        }

        fn title(&self) -> Option<&str> {
            None
        }

        fn pid(&self) -> Option<ProcessId> {
            self.inner.pid
        }

        fn original_tile_index(&self) -> usize {
            self.inner.original_tile_index
        }
    }

    struct FakeWindowManager {
        windows: Vec<WindowRecord>,
        capabilities: WindowManagerCapabilities,
        move_calls: usize,
        move_column_calls: usize,
        consume_calls: usize,
        consume_last_tile_index: Option<usize>,
    }

    impl WindowManagerMetadata for FakeWindowManager {
        fn adapter_name(&self) -> &'static str {
            "fake"
        }

        fn capabilities(&self) -> WindowManagerCapabilities {
            self.capabilities
        }
    }

    impl WindowManagerIntrospection for FakeWindowManager {
        type FocusedWindow<'a>
            = FakeFocusedWindow<'a>
        where
            Self: 'a;

        fn with_focused_window<R>(
            &mut self,
            visit: impl for<'a> FnOnce(Self::FocusedWindow<'a>) -> Result<R>,
        ) -> Result<R> {
            let focused = self
                .windows
                .iter()
                .find(|window| window.is_focused)
                .ok_or_else(|| anyhow!("no focused window"))?;
            visit(FakeFocusedWindow { inner: focused })
        }

        fn windows(&mut self) -> Result<Vec<WindowRecord>> {
            Ok(self.windows.clone())
        }
    }

    impl WindowManagerExecution for FakeWindowManager {
        fn focus_direction(&mut self, _direction: Direction) -> Result<()> {
            Ok(())
        }

        fn move_direction(&mut self, _direction: Direction) -> Result<()> {
            self.move_calls += 1;
            Ok(())
        }

        fn move_column(&mut self, _direction: Direction) -> Result<()> {
            self.move_column_calls += 1;
            Ok(())
        }

        fn consume_into_column_and_move(
            &mut self,
            _direction: Direction,
            original_tile_index: usize,
        ) -> Result<()> {
            self.consume_calls += 1;
            self.consume_last_tile_index = Some(original_tile_index);
            Ok(())
        }

        fn resize_with_intent(
            &mut self,
            _intent: crate::adapters::window_managers::ResizeIntent,
        ) -> Result<()> {
            Ok(())
        }

        fn spawn(&mut self, _command: Vec<String>) -> Result<()> {
            Ok(())
        }

        fn focus_window_by_id(&mut self, _id: u64) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn move_prefers_cross_domain_transfer_when_payloads_are_compatible() {
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

        let mut wm = FakeWindowManager {
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
            capabilities: WindowManagerCapabilities::none(),
            move_calls: 0,
            move_column_calls: 0,
            consume_calls: 0,
            consume_last_tile_index: None,
        };

        orchestrator
            .execute(
                &mut wm,
                ActionRequest {
                    kind: ActionKind::Move,
                    direction: crate::engine::topology::Cardinal::East,
                },
            )
            .expect("move should succeed");

        assert_eq!(
            wm.move_calls, 0,
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
    }

    #[test]
    fn move_falls_back_to_wm_when_transfer_has_no_compatible_payload() {
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

        let mut wm = FakeWindowManager {
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
            capabilities: WindowManagerCapabilities::none(),
            move_calls: 0,
            move_column_calls: 0,
            consume_calls: 0,
            consume_last_tile_index: None,
        };

        orchestrator
            .execute(
                &mut wm,
                ActionRequest {
                    kind: ActionKind::Move,
                    direction: crate::engine::topology::Cardinal::East,
                },
            )
            .expect("move should still succeed via fallback");

        assert_eq!(
            wm.move_calls, 1,
            "wm fallback should run when transfer is incompatible"
        );
        assert_eq!(source_counters.tear_off_calls.load(Ordering::Relaxed), 1);
        assert_eq!(target_counters.merge_calls.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn move_merges_within_same_domain_when_supported() {
        let mut orchestrator = Orchestrator::default();

        let counters = DomainCounters::default();
        orchestrator.register_domain(Box::new(FakeDomain::new(
            TERMINAL_DOMAIN_ID,
            "terminal",
            vec![TypeId::of::<BufferPayload>()],
            Some(Box::new(BufferPayload)),
            counters.clone(),
        )));

        let mut wm = FakeWindowManager {
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
            capabilities: WindowManagerCapabilities::none(),
            move_calls: 0,
            move_column_calls: 0,
            consume_calls: 0,
            consume_last_tile_index: None,
        };

        orchestrator
            .execute(
                &mut wm,
                ActionRequest {
                    kind: ActionKind::Move,
                    direction: crate::engine::topology::Cardinal::East,
                },
            )
            .expect("move should merge within same domain");

        assert_eq!(wm.move_calls, 0, "wm fallback should not run");
        assert_eq!(counters.tear_off_calls.load(Ordering::Relaxed), 1);
        assert_eq!(counters.merge_calls.load(Ordering::Relaxed), 1);
        assert!(
            counters.snapshot_calls.load(Ordering::Relaxed) > 0,
            "domain should resync after within-domain transfer"
        );
    }

    fn composed_tearout_capabilities_for(direction: Direction) -> WindowManagerCapabilities {
        let mut caps = WindowManagerCapabilities::none();
        caps.primitives.tear_out_right = true;
        caps.primitives.move_column = true;
        caps.primitives.consume_into_column_and_move = true;
        match direction {
            Direction::West => {
                caps.tear_out.west = crate::adapters::window_managers::CapabilitySupport::Composed
            }
            Direction::East => {
                caps.tear_out.east = crate::adapters::window_managers::CapabilitySupport::Composed
            }
            Direction::North => {
                caps.tear_out.north = crate::adapters::window_managers::CapabilitySupport::Composed
            }
            Direction::South => {
                caps.tear_out.south = crate::adapters::window_managers::CapabilitySupport::Composed
            }
        }
        caps
    }

    #[test]
    fn place_tearout_window_moves_column_for_composed_west() {
        let orchestrator = Orchestrator::default();
        let mut wm = FakeWindowManager {
            windows: vec![
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
            capabilities: composed_tearout_capabilities_for(Direction::West),
            move_calls: 0,
            move_column_calls: 0,
            consume_calls: 0,
            consume_last_tile_index: None,
        };

        orchestrator
            .place_tearout_window(&mut wm, Direction::West, 11, 4)
            .expect("tearout placement should succeed");
        assert_eq!(wm.move_column_calls, 1);
        assert_eq!(wm.consume_calls, 0);
    }

    #[test]
    fn place_tearout_window_consumes_for_composed_north() {
        let orchestrator = Orchestrator::default();
        let mut wm = FakeWindowManager {
            windows: vec![
                WindowRecord {
                    id: 21,
                    app_id: Some("org.wezfurlong.wezterm".into()),
                    title: Some("source".into()),
                    pid: ProcessId::new(1),
                    is_focused: false,
                    original_tile_index: 2,
                },
                WindowRecord {
                    id: 22,
                    app_id: Some("org.wezfurlong.wezterm".into()),
                    title: Some("tearout".into()),
                    pid: ProcessId::new(2),
                    is_focused: true,
                    original_tile_index: 1,
                },
            ],
            capabilities: composed_tearout_capabilities_for(Direction::North),
            move_calls: 0,
            move_column_calls: 0,
            consume_calls: 0,
            consume_last_tile_index: None,
        };

        orchestrator
            .place_tearout_window(&mut wm, Direction::North, 21, 7)
            .expect("tearout placement should succeed");
        assert_eq!(wm.move_column_calls, 0);
        assert_eq!(wm.consume_calls, 1);
        assert_eq!(wm.consume_last_tile_index, Some(7));
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

        let selected = Orchestrator::select_tearout_window_id(
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

        let selected = Orchestrator::select_tearout_window_id(
            &pre_window_ids,
            &windows,
            20,
            ProcessId::new(4242),
            "org.wezfurlong.wezterm",
        );
        assert_eq!(selected, Some(21));
    }
}
