use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use anyhow::{Context, Result};

use crate::adapters::window_managers::{
    plan_tear_out, CapabilitySupport, ResizeIntent, ResizeKind, WindowManagerSession, WindowRecord,
};
use crate::engine::contract::{
    AppAdapter, AppKind, ChainResolver, MergeExecutionMode, MoveDecision, TopologyHandler,
};
use crate::engine::domain::ErasedDomain;
use crate::engine::domain::{domain_id_for_window, encode_native_window_ref};
use crate::engine::domain::{PayloadRegistry, TransferOutcome, TransferPipeline};
use crate::engine::runtime::ProcessId;
use crate::engine::topology::Direction;
use crate::engine::topology::{DomainId, GlobalLeaf, Rect};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirectionalProbeFocusMode {
    RestoreSource,
    KeepTarget,
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
        W: WindowManagerSession + ?Sized,
    {
        self.execute_session(wm, request)
    }

    fn execute_session<W>(&mut self, wm: &mut W, request: ActionRequest) -> Result<()>
    where
        W: WindowManagerSession + ?Sized,
    {
        match request.kind {
            ActionKind::Focus => self.execute_focus_session(wm, request.direction),
            ActionKind::Move => self.execute_move_session(wm, request.direction),
            ActionKind::Resize { grow, step } => {
                self.execute_resize_session(wm, request.direction, grow, step)
            }
        }
    }

    pub fn execute_focus<W>(&mut self, wm: &mut W, dir: Direction) -> Result<()>
    where
        W: WindowManagerSession + ?Sized,
    {
        self.execute_focus_session(wm, dir)
    }

    fn execute_focus_session<W>(&mut self, wm: &mut W, dir: Direction) -> Result<()>
    where
        W: WindowManagerSession + ?Sized,
    {
        let _span = tracing::debug_span!("orchestrator.execute_focus", ?dir).entered();
        let fallback_dir = dir.into();
        if self.attempt_focused_app_focus(wm, fallback_dir)? {
            return Ok(());
        }
        wm.focus_direction(fallback_dir)
    }

    fn attempt_focused_app_focus<W>(&mut self, wm: &mut W, dir: Direction) -> Result<bool>
    where
        W: WindowManagerSession + ?Sized,
    {
        let _span = tracing::debug_span!("orchestrator.attempt_focused_app_focus", ?dir).entered();
        let focused = wm.focused_window()?;
        let app_id = focused.app_id.unwrap_or_default();
        let title = focused.title.unwrap_or_default();
        let source_pid = focused.pid;
        let owner_pid = source_pid.map(ProcessId::get);
        let Some(owner_pid) = owner_pid else {
            return Ok(false);
        };

        for app in crate::engine::chain_resolver::runtime_chain_resolver()
            .resolve_chain(&app_id, owner_pid, &title)
        {
            if !app.capabilities().focus {
                continue;
            }
            let adapter_name = app.adapter_name();
            if TopologyHandler::can_focus(app.as_ref(), dir, owner_pid)
                .with_context(|| format!("{adapter_name} can_focus failed"))?
            {
                TopologyHandler::focus(app.as_ref(), dir, owner_pid)
                    .with_context(|| format!("{adapter_name} focus failed"))?;
                logging::debug(format!("orchestrator: app focus handled by {adapter_name}"));
                return Ok(true);
            }
        }

        Ok(false)
    }

    pub fn execute_move<W>(&mut self, wm: &mut W, dir: Direction) -> Result<()>
    where
        W: WindowManagerSession + ?Sized,
    {
        self.execute_move_session(wm, dir)
    }

    fn execute_move_session<W>(&mut self, wm: &mut W, dir: Direction) -> Result<()>
    where
        W: WindowManagerSession + ?Sized,
    {
        let fallback_dir = dir.into();
        if self.attempt_focused_app_move(wm, fallback_dir)? {
            return Ok(());
        }

        let focused = Self::focused_window_record(wm)?;
        let Some(target_window) = self.probe_directional_target(
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

    fn attempt_focused_app_move<W>(&mut self, wm: &mut W, dir: Direction) -> Result<bool>
    where
        W: WindowManagerSession + ?Sized,
    {
        let focused = wm.focused_window()?;
        let source_window_id = focused.id;
        let source_tile_index = focused.original_tile_index;
        let app_id = focused.app_id.unwrap_or_default();
        let title = focused.title.unwrap_or_default();
        let source_pid = focused.pid;
        let owner_pid = source_pid.map(ProcessId::get);
        let Some(owner_pid) = owner_pid else {
            return Ok(false);
        };

        let chain = crate::engine::chain_resolver::runtime_chain_resolver()
            .resolve_chain(&app_id, owner_pid, &title);
        for (index, app) in chain.iter().enumerate() {
            let adapter_name = app.adapter_name();
            let decision = TopologyHandler::move_decision(app.as_ref(), dir, owner_pid)
                .with_context(|| format!("{adapter_name} move_decision failed"))?;
            match decision {
                MoveDecision::Passthrough => {
                    if self.attempt_passthrough_merge(
                        wm,
                        app.as_ref(),
                        &chain[index + 1..],
                        &app_id,
                        &title,
                        dir,
                        source_window_id,
                        source_pid,
                    )? {
                        return Ok(true);
                    }
                    if matches!(app.kind(), AppKind::Terminal)
                        && app.capabilities().tear_out
                        && self
                            .probe_directional_target_for_adapter(
                                wm,
                                dir,
                                source_window_id,
                                adapter_name,
                                DirectionalProbeFocusMode::RestoreSource,
                            )?
                            .is_none()
                    {
                        self.execute_app_tear_out(
                            wm,
                            app.as_ref(),
                            dir,
                            owner_pid,
                            source_window_id,
                            source_tile_index,
                            source_pid,
                            &app_id,
                            "PassthroughTearOut",
                        )?;
                        return Ok(true);
                    }
                }
                MoveDecision::Internal => {
                    TopologyHandler::move_internal(app.as_ref(), dir, owner_pid)
                        .with_context(|| format!("{adapter_name} move_internal failed"))?;
                    logging::debug(format!(
                        "orchestrator: app move handled by {adapter_name} decision=Internal"
                    ));
                    return Ok(true);
                }
                MoveDecision::Rearrange => {
                    TopologyHandler::rearrange(app.as_ref(), dir, owner_pid)
                        .with_context(|| format!("{adapter_name} rearrange failed"))?;
                    logging::debug(format!(
                        "orchestrator: app move handled by {adapter_name} decision=Rearrange"
                    ));
                    return Ok(true);
                }
                MoveDecision::TearOut => {
                    self.execute_app_tear_out(
                        wm,
                        app.as_ref(),
                        dir,
                        owner_pid,
                        source_window_id,
                        source_tile_index,
                        source_pid,
                        &app_id,
                        "TearOut",
                    )?;
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    fn execute_app_tear_out<W>(
        &mut self,
        wm: &mut W,
        app: &dyn AppAdapter,
        dir: Direction,
        owner_pid: u32,
        source_window_id: u64,
        source_tile_index: usize,
        source_pid: Option<ProcessId>,
        app_id: &str,
        decision_label: &str,
    ) -> Result<()>
    where
        W: WindowManagerSession + ?Sized,
    {
        let adapter_name = app.adapter_name();
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
        let tear = TopologyHandler::move_out(app, dir, owner_pid)
            .with_context(|| format!("{adapter_name} move_out failed"))?;
        if let Some(command) = tear.spawn_command {
            wm.spawn(command)
                .with_context(|| format!("{adapter_name} tear-out spawn via wm failed"))?;
        }
        let tearout_window_id = match self.focus_tearout_window(
            wm,
            &pre_window_ids,
            source_window_id,
            source_pid,
            app_id,
        ) {
            Ok(window_id) => window_id,
            Err(err) => {
                logging::debug(format!(
                    "orchestrator: unable to focus tear-out window adapter={} err={:#}",
                    adapter_name, err
                ));
                None
            }
        };
        if let Err(err) = self.place_tearout_window(
            wm,
            dir,
            source_window_id,
            source_tile_index,
            tearout_window_id,
        ) {
            logging::debug(format!(
                "orchestrator: tear-out placement fallback failed adapter={} err={:#}",
                adapter_name, err
            ));
        }
        logging::debug(format!(
            "orchestrator: app move handled by {adapter_name} decision={decision_label}"
        ));
        Ok(())
    }

    fn focus_tearout_window<W>(
        &self,
        wm: &mut W,
        pre_window_ids: &BTreeSet<u64>,
        source_window_id: u64,
        source_pid: Option<ProcessId>,
        source_app_id: &str,
    ) -> Result<Option<u64>>
    where
        W: WindowManagerSession + ?Sized,
    {
        let target_window_id = self.wait_for_tearout_window_id(
            wm,
            pre_window_ids,
            source_window_id,
            source_pid,
            source_app_id,
        )?;
        if let Some(target_window_id) = target_window_id {
            if target_window_id != source_window_id {
                wm.focus_window_by_id(target_window_id)?;
                return Ok(Some(target_window_id));
            }
        }
        Ok(None)
    }

    fn wait_for_tearout_window_id<W>(
        &self,
        wm: &mut W,
        pre_window_ids: &BTreeSet<u64>,
        source_window_id: u64,
        source_pid: Option<ProcessId>,
        source_app_id: &str,
    ) -> Result<Option<u64>>
    where
        W: WindowManagerSession + ?Sized,
    {
        const ATTEMPTS: usize = 25;
        const DELAY: Duration = Duration::from_millis(40);

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
                            return Ok(Some(target_window_id));
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

        Ok(None)
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
        target_window_id: Option<u64>,
    ) -> Result<()>
    where
        W: WindowManagerSession + ?Sized,
    {
        if let Some(target_window_id) = target_window_id.filter(|id| *id != source_window_id) {
            wm.focus_window_by_id(target_window_id)?;
        }

        let focused_window_id = wm.focused_window()?.id;
        if focused_window_id == source_window_id {
            return Ok(());
        }

        match plan_tear_out(wm.capabilities(), dir) {
            CapabilitySupport::Native => wm.move_direction(dir),
            CapabilitySupport::Unsupported => Ok(()),
            CapabilitySupport::Composed => match dir {
                Direction::West | Direction::East => wm.move_column(dir),
                Direction::North | Direction::South => {
                    wm.consume_into_column_and_move(dir, source_tile_index)
                }
            },
        }
    }

    fn focused_window_record<W>(wm: &mut W) -> Result<WindowRecord>
    where
        W: WindowManagerSession + ?Sized,
    {
        let window = wm.focused_window()?;
        Ok(WindowRecord {
            id: window.id,
            app_id: window.app_id,
            title: window.title,
            pid: window.pid,
            is_focused: true,
            original_tile_index: window.original_tile_index,
        })
    }

    fn probe_directional_target<W>(
        &self,
        wm: &mut W,
        dir: Direction,
        source_window_id: u64,
        focus_mode: DirectionalProbeFocusMode,
    ) -> Result<Option<WindowRecord>>
    where
        W: WindowManagerSession + ?Sized,
    {
        if let Err(err) = wm.focus_direction(dir) {
            logging::debug(format!(
                "orchestrator: directional target probe failed dir={} err={:#}",
                dir, err
            ));
            return Ok(None);
        }

        let target = match Self::focused_window_record(wm) {
            Ok(window) => window,
            Err(err) => {
                let _ = wm.focus_window_by_id(source_window_id);
                return Err(err.context("failed to read target window during directional probe"));
            }
        };

        if target.id == source_window_id {
            return Ok(None);
        }

        if matches!(focus_mode, DirectionalProbeFocusMode::RestoreSource) {
            wm.focus_window_by_id(source_window_id).with_context(|| {
                format!("failed to restore focus to window {}", source_window_id)
            })?;
        }
        Ok(Some(target))
    }

    fn probe_directional_target_for_adapter<W>(
        &self,
        wm: &mut W,
        dir: Direction,
        source_window_id: u64,
        adapter_name: &str,
        focus_mode: DirectionalProbeFocusMode,
    ) -> Result<Option<WindowRecord>>
    where
        W: WindowManagerSession + ?Sized,
    {
        let Some(target_window) =
            self.probe_directional_target(wm, dir, source_window_id, focus_mode)?
        else {
            return Ok(None);
        };
        if Self::window_matches_adapter(adapter_name, &target_window) {
            return Ok(Some(target_window));
        }
        if matches!(focus_mode, DirectionalProbeFocusMode::KeepTarget) {
            let _ = wm.focus_window_by_id(source_window_id);
        }
        Ok(None)
    }

    fn probe_in_place_target_for_adapter<W>(
        &self,
        wm: &mut W,
        outer_chain: &[Box<dyn AppAdapter>],
        dir: Direction,
        source_window_id: u64,
        owner_pid: u32,
        app_id: &str,
        title: &str,
        adapter_name: &str,
    ) -> Result<Option<Box<dyn AppAdapter>>>
    where
        W: WindowManagerSession + ?Sized,
    {
        for outer in outer_chain {
            if !outer.capabilities().focus
                || !TopologyHandler::can_focus(outer.as_ref(), dir, owner_pid)?
            {
                continue;
            }
            TopologyHandler::focus(outer.as_ref(), dir, owner_pid)?;
            let focused_window_id = wm.focused_window()?.id;
            if focused_window_id != source_window_id {
                let _ = wm.focus_window_by_id(source_window_id);
                continue;
            }
            let target_app = crate::engine::chain_resolver::runtime_chain_resolver()
                .resolve_chain(app_id, owner_pid, title)
                .into_iter()
                .find(|candidate| candidate.adapter_name() == adapter_name);
            if target_app.is_some() {
                return Ok(target_app);
            }
            let _ = TopologyHandler::focus(outer.as_ref(), dir.opposite(), owner_pid);
        }
        Ok(None)
    }

    fn restore_in_place_target_focus(
        &self,
        outer_chain: &[Box<dyn AppAdapter>],
        dir: Direction,
        owner_pid: u32,
    ) {
        for outer in outer_chain {
            if outer.capabilities().focus
                && TopologyHandler::can_focus(outer.as_ref(), dir.opposite(), owner_pid)
                    .unwrap_or(false)
            {
                let _ = TopologyHandler::focus(outer.as_ref(), dir.opposite(), owner_pid);
                break;
            }
        }
    }

    fn resolve_adapter_for_window(
        adapter_name: &str,
        window: &WindowRecord,
    ) -> Option<Box<dyn AppAdapter>> {
        let owner_pid = window.pid.map(ProcessId::get).unwrap_or(0);
        crate::engine::chain_resolver::runtime_chain_resolver()
            .resolve_chain(
                window.app_id.as_deref().unwrap_or_default(),
                owner_pid,
                window.title.as_deref().unwrap_or_default(),
            )
            .into_iter()
            .find(|adapter| adapter.adapter_name() == adapter_name)
    }

    fn window_matches_adapter(adapter_name: &str, window: &WindowRecord) -> bool {
        Self::resolve_adapter_for_window(adapter_name, window).is_some()
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

    fn attempt_passthrough_merge<W>(
        &mut self,
        wm: &mut W,
        app: &dyn AppAdapter,
        outer_chain: &[Box<dyn AppAdapter>],
        app_id: &str,
        title: &str,
        dir: Direction,
        source_window_id: u64,
        source_pid: Option<ProcessId>,
    ) -> Result<bool>
    where
        W: WindowManagerSession + ?Sized,
    {
        if !app.capabilities().merge {
            return Ok(false);
        }
        let adapter_name = app.adapter_name();
        let preparation = match TopologyHandler::prepare_merge(app, source_pid) {
            Ok(value) => value,
            Err(err) => {
                logging::debug(format!(
                    "orchestrator: app passthrough merge prepare failed adapter={} err={:#}",
                    adapter_name, err
                ));
                return Ok(false);
            }
        };

        match TopologyHandler::merge_execution_mode(app) {
            MergeExecutionMode::SourceFocused => {
                let Some(target_window) = self.probe_directional_target_for_adapter(
                    wm,
                    dir,
                    source_window_id,
                    adapter_name,
                    DirectionalProbeFocusMode::RestoreSource,
                )?
                else {
                    return Ok(false);
                };
                let preparation = TopologyHandler::augment_merge_preparation_for_target(
                    app,
                    preparation,
                    Some(target_window.id),
                );

                match TopologyHandler::merge_into_target(
                    app,
                    dir,
                    source_pid,
                    target_window.pid,
                    preparation,
                ) {
                    Ok(()) => {
                        self.cleanup_merged_source_window(
                            wm,
                            source_window_id,
                            target_window.id,
                            adapter_name,
                        );
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
                if let Some(owner_pid) = source_pid.map(ProcessId::get) {
                    if let Some(target_app) = self.probe_in_place_target_for_adapter(
                        wm,
                        outer_chain,
                        dir,
                        source_window_id,
                        owner_pid,
                        app_id,
                        title,
                        adapter_name,
                    )? {
                        let preparation = TopologyHandler::augment_merge_preparation_for_target(
                            target_app.as_ref(),
                            preparation,
                            Some(source_window_id),
                        );

                        match TopologyHandler::merge_into_target(
                            target_app.as_ref(),
                            dir,
                            source_pid,
                            source_pid,
                            preparation,
                        ) {
                            Ok(()) => {
                                logging::debug(format!(
                                    "orchestrator: app move handled by {adapter_name} decision=MergeTargetFocusedInPlace"
                                ));
                                return Ok(true);
                            }
                            Err(err) => {
                                self.restore_in_place_target_focus(outer_chain, dir, owner_pid);
                                logging::debug(format!(
                                    "orchestrator: app passthrough merge failed adapter={} err={:#}",
                                    adapter_name, err
                                ));
                                return Ok(false);
                            }
                        }
                    }
                }

                let Some(target_window) = self.probe_directional_target_for_adapter(
                    wm,
                    dir,
                    source_window_id,
                    adapter_name,
                    DirectionalProbeFocusMode::KeepTarget,
                )?
                else {
                    return Ok(false);
                };
                let Some(target_app) =
                    Self::resolve_adapter_for_window(adapter_name, &target_window)
                else {
                    let _ = wm.focus_window_by_id(source_window_id);
                    return Ok(false);
                };
                let preparation = TopologyHandler::augment_merge_preparation_for_target(
                    target_app.as_ref(),
                    preparation,
                    Some(target_window.id),
                );

                match TopologyHandler::merge_into_target(
                    target_app.as_ref(),
                    dir,
                    source_pid,
                    target_window.pid,
                    preparation,
                ) {
                    Ok(()) => {
                        self.cleanup_merged_source_window(
                            wm,
                            source_window_id,
                            target_window.id,
                            adapter_name,
                        );
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

    fn cleanup_merged_source_window<W>(
        &self,
        wm: &mut W,
        source_window_id: u64,
        target_window_id: u64,
        adapter_name: &str,
    ) where
        W: WindowManagerSession + ?Sized,
    {
        if source_window_id == target_window_id {
            return;
        }
        if let Err(err) = wm.focus_window_by_id(target_window_id) {
            logging::debug(format!(
                "orchestrator: merge cleanup focus failed adapter={} target_window_id={} err={:#}",
                adapter_name, target_window_id, err
            ));
        }
        if let Err(err) = wm.close_window_by_id(source_window_id) {
            logging::debug(format!(
                "orchestrator: merge cleanup close failed adapter={} source_window_id={} err={:#}",
                adapter_name, source_window_id, err
            ));
        }
    }

    pub fn execute_resize<W>(
        &mut self,
        wm: &mut W,
        dir: Direction,
        grow: bool,
        step: i32,
    ) -> Result<()>
    where
        W: WindowManagerSession + ?Sized,
    {
        self.execute_resize_session(wm, dir, grow, step)
    }

    fn execute_resize_session<W>(
        &mut self,
        wm: &mut W,
        dir: Direction,
        grow: bool,
        step: i32,
    ) -> Result<()>
    where
        W: WindowManagerSession + ?Sized,
    {
        if self.attempt_focused_app_resize(wm, dir, grow, step.max(1))? {
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

    fn attempt_focused_app_resize<W>(
        &mut self,
        wm: &mut W,
        dir: Direction,
        grow: bool,
        step: i32,
    ) -> Result<bool>
    where
        W: WindowManagerSession + ?Sized,
    {
        let focused = wm.focused_window()?;
        let app_id = focused.app_id.unwrap_or_default();
        let title = focused.title.unwrap_or_default();
        let source_pid = focused.pid;
        let owner_pid = source_pid.map(ProcessId::get);
        let Some(owner_pid) = owner_pid else {
            return Ok(false);
        };

        for app in crate::engine::chain_resolver::runtime_chain_resolver()
            .resolve_chain(&app_id, owner_pid, &title)
        {
            if !app.capabilities().resize_internal {
                continue;
            }
            let adapter_name = app.adapter_name();
            if TopologyHandler::can_resize(app.as_ref(), dir, grow, owner_pid)
                .with_context(|| format!("{adapter_name} can_resize failed"))?
            {
                TopologyHandler::resize_internal(app.as_ref(), dir, grow, step, owner_pid)
                    .with_context(|| format!("{adapter_name} resize_internal failed"))?;
                logging::debug(format!(
                    "orchestrator: app resize handled by {adapter_name}"
                ));
                return Ok(true);
            }
        }

        Ok(false)
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
    use crate::adapters::window_managers::{
        ConfiguredWindowManager, FocusedWindowRecord, FocusedWindowView, WindowManagerCapabilities,
        WindowManagerExecution, WindowManagerFeatures, WindowManagerIntrospection,
        WindowManagerMetadata, WindowManagerSession, WindowRecord,
    };
    use crate::engine::domain::PaneState;
    use crate::engine::domain::{DomainLeafSnapshot, DomainSnapshot, ErasedDomain};
    use crate::engine::domain::{EDITOR_DOMAIN_ID, TERMINAL_DOMAIN_ID};
    use crate::engine::runtime::ProcessId;
    use crate::engine::topology::Direction;
    use crate::engine::topology::{GlobalLeaf, Rect};

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

        fn move_column(&mut self, _direction: Direction) -> Result<()> {
            Ok(())
        }

        fn consume_into_column_and_move(
            &mut self,
            _direction: Direction,
            _original_tile_index: usize,
        ) -> Result<()> {
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

    #[test]
    fn execute_accepts_configured_window_manager_handle() {
        let state = Arc::new(Mutex::new(SessionState {
            focus_calls: Vec::new(),
        }));
        let mut wm = ConfiguredWindowManager::new(
            Box::new(RecordingSession {
                state: state.clone(),
            }),
            WindowManagerFeatures::default(),
        );
        let mut orchestrator = Orchestrator::default();

        orchestrator
            .execute(
                &mut wm,
                ActionRequest {
                    kind: ActionKind::Focus,
                    direction: Direction::West,
                },
            )
            .expect("configured window manager should execute orchestrator actions");

        assert_eq!(
            state
                .lock()
                .expect("session state mutex should not be poisoned")
                .focus_calls,
            vec![Direction::West]
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
            _dir: crate::engine::topology::Direction,
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
        window_snapshots: Vec<Vec<WindowRecord>>,
        windows_call_count: usize,
        capabilities: WindowManagerCapabilities,
        move_calls: usize,
        move_column_calls: usize,
        consume_calls: usize,
        consume_last_tile_index: Option<usize>,
        close_calls: usize,
        closed_window_ids: Vec<u64>,
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
            if let Some(snapshot) = self.window_snapshots.get(self.windows_call_count).cloned() {
                self.windows = snapshot;
            }
            self.windows_call_count += 1;
            Ok(self.windows.clone())
        }
    }

    impl WindowManagerExecution for FakeWindowManager {
        fn focus_direction(&mut self, _direction: Direction) -> Result<()> {
            if self.windows.len() < 2 {
                return Ok(());
            }
            let focused_idx = self
                .windows
                .iter()
                .position(|window| window.is_focused)
                .ok_or_else(|| anyhow!("no focused window"))?;
            let target_idx = if focused_idx + 1 < self.windows.len() {
                focused_idx + 1
            } else {
                focused_idx.saturating_sub(1)
            };
            for (idx, window) in self.windows.iter_mut().enumerate() {
                window.is_focused = idx == target_idx;
            }
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

        fn focus_window_by_id(&mut self, id: u64) -> Result<()> {
            let mut matched = false;
            for window in &mut self.windows {
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
            let original_len = self.windows.len();
            self.windows.retain(|window| window.id != id);
            if self.windows.len() == original_len {
                return Err(anyhow!("window id {id} not found"));
            }
            self.close_calls += 1;
            self.closed_window_ids.push(id);
            Ok(())
        }
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
            window_snapshots: Vec::new(),
            windows_call_count: 0,
            capabilities: WindowManagerCapabilities::none(),
            move_calls: 0,
            move_column_calls: 0,
            consume_calls: 0,
            consume_last_tile_index: None,
            close_calls: 0,
            closed_window_ids: Vec::new(),
        };

        orchestrator
            .execute_session(
                &mut wm,
                ActionRequest {
                    kind: ActionKind::Move,
                    direction: crate::engine::topology::Direction::East,
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
            window_snapshots: Vec::new(),
            windows_call_count: 0,
            capabilities: WindowManagerCapabilities::none(),
            move_calls: 0,
            move_column_calls: 0,
            consume_calls: 0,
            consume_last_tile_index: None,
            close_calls: 0,
            closed_window_ids: Vec::new(),
        };

        orchestrator
            .execute_session(
                &mut wm,
                ActionRequest {
                    kind: ActionKind::Move,
                    direction: crate::engine::topology::Direction::East,
                },
            )
            .expect("move should still succeed via fallback");

        assert_eq!(
            wm.move_calls, 1,
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
            window_snapshots: Vec::new(),
            windows_call_count: 0,
            capabilities: WindowManagerCapabilities::none(),
            move_calls: 0,
            move_column_calls: 0,
            consume_calls: 0,
            consume_last_tile_index: None,
            close_calls: 0,
            closed_window_ids: Vec::new(),
        };

        orchestrator
            .execute_session(
                &mut wm,
                ActionRequest {
                    kind: ActionKind::Move,
                    direction: crate::engine::topology::Direction::East,
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

        restore_config(old_config);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn cleanup_merged_source_window_closes_source_and_keeps_target_focused() {
        let orchestrator = Orchestrator::default();
        let mut wm = FakeWindowManager {
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
            move_column_calls: 0,
            consume_calls: 0,
            consume_last_tile_index: None,
            close_calls: 0,
            closed_window_ids: Vec::new(),
        };

        orchestrator.cleanup_merged_source_window(&mut wm, 301, 302, "terminal");

        assert_eq!(wm.close_calls, 1);
        assert_eq!(wm.closed_window_ids, vec![301]);
        assert_eq!(wm.windows.len(), 1);
        assert_eq!(wm.windows[0].id, 302);
        assert!(wm.windows[0].is_focused);
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
            window_snapshots: Vec::new(),
            windows_call_count: 0,
            capabilities: composed_tearout_capabilities_for(Direction::West),
            move_calls: 0,
            move_column_calls: 0,
            consume_calls: 0,
            consume_last_tile_index: None,
            close_calls: 0,
            closed_window_ids: Vec::new(),
        };

        orchestrator
            .place_tearout_window(&mut wm, Direction::West, 11, 4, None)
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
            window_snapshots: Vec::new(),
            windows_call_count: 0,
            capabilities: composed_tearout_capabilities_for(Direction::North),
            move_calls: 0,
            move_column_calls: 0,
            consume_calls: 0,
            consume_last_tile_index: None,
            close_calls: 0,
            closed_window_ids: Vec::new(),
        };

        orchestrator
            .place_tearout_window(&mut wm, Direction::North, 21, 7, None)
            .expect("tearout placement should succeed");
        assert_eq!(wm.move_column_calls, 0);
        assert_eq!(wm.consume_calls, 1);
        assert_eq!(wm.consume_last_tile_index, Some(7));
    }

    #[test]
    fn focus_tearout_window_retries_until_new_window_appears() {
        let orchestrator = Orchestrator::default();
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
        let mut wm = FakeWindowManager {
            windows: vec![source_window.clone()],
            window_snapshots: vec![
                vec![source_window.clone()],
                vec![source_window.clone(), tearout_window],
            ],
            windows_call_count: 0,
            capabilities: composed_tearout_capabilities_for(Direction::North),
            move_calls: 0,
            move_column_calls: 0,
            consume_calls: 0,
            consume_last_tile_index: None,
            close_calls: 0,
            closed_window_ids: Vec::new(),
        };

        let focused = orchestrator
            .focus_tearout_window(
                &mut wm,
                &pre_window_ids,
                31,
                source_pid,
                "com.mitchellh.ghostty",
            )
            .expect("tearout focus should succeed");

        assert_eq!(focused, Some(32));
        assert_eq!(wm.windows_call_count, 2);
        assert_eq!(
            wm.windows
                .iter()
                .find(|window| window.is_focused)
                .map(|window| window.id),
            Some(32)
        );
    }

    #[test]
    fn place_tearout_window_focuses_known_target_before_composed_north() {
        let orchestrator = Orchestrator::default();
        let mut wm = FakeWindowManager {
            windows: vec![
                WindowRecord {
                    id: 41,
                    app_id: Some("com.mitchellh.ghostty".into()),
                    title: Some("source".into()),
                    pid: ProcessId::new(1),
                    is_focused: true,
                    original_tile_index: 2,
                },
                WindowRecord {
                    id: 42,
                    app_id: Some("com.mitchellh.ghostty".into()),
                    title: Some("tearout".into()),
                    pid: ProcessId::new(2),
                    is_focused: false,
                    original_tile_index: 1,
                },
            ],
            window_snapshots: Vec::new(),
            windows_call_count: 0,
            capabilities: composed_tearout_capabilities_for(Direction::North),
            move_calls: 0,
            move_column_calls: 0,
            consume_calls: 0,
            consume_last_tile_index: None,
            close_calls: 0,
            closed_window_ids: Vec::new(),
        };

        orchestrator
            .place_tearout_window(&mut wm, Direction::North, 41, 9, Some(42))
            .expect("tearout placement should succeed");

        assert_eq!(wm.consume_calls, 1);
        assert_eq!(wm.consume_last_tile_index, Some(9));
        assert_eq!(
            wm.windows
                .iter()
                .find(|window| window.is_focused)
                .map(|window| window.id),
            Some(42)
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
