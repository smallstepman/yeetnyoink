use crate::config::{self, WmBackend};
use crate::engine::runtime::{self, CommandContext, ProcessId};
use crate::engine::topology::{DirectedRect, Direction, Rect};
use crate::engine::wm::{
    CapabilitySupport, ConfiguredWindowManager, DirectionalCapability, FloatingFocusMode,
    FocusedAppRecord, FocusedWindowRecord, PrimitiveWindowManagerCapabilities, ResizeIntent,
    WindowManagerCapabilities, WindowManagerCapabilityDescriptor, WindowManagerFeatures,
    WindowManagerSession, WindowManagerSpec, WindowRecord, validate_declared_capabilities,
};
use crate::logging;
use anyhow::{Context, bail};

use macos_window_manager::{
    ActiveSpaceFocusTargetHint, MacosNativeApi, MacosNativeConnectError, MacosNativeOperationError,
    MacosNativeProbeError, MissionControlHotkey, MissionControlModifiers, NativeBackendOptions,
    NativeBounds, NativeDesktopSnapshot, NativeDiagnostics, NativeDirection, NativeSpaceSnapshot,
    NativeWindowSnapshot, RealNativeApi, SpaceKind,
};

#[cfg(test)]
#[path = "macos_window_manager_test_support.rs"]
#[allow(dead_code, unused_imports)]
mod macos_window_manager_test_support;

pub(crate) struct MacosNativeAdapter<A = RealNativeApi> {
    ctx: MacosNativeContext<A>,
}

trait MacosNativeApiFactory {
    type Api: MacosNativeApi;

    fn create(&self) -> Self::Api;
}

#[derive(Clone, Copy)]
pub(crate) struct RealNativeApiFactory;

impl MacosNativeApiFactory for RealNativeApiFactory {
    type Api = RealNativeApi;

    fn create(&self) -> Self::Api {
        RealNativeApi::new(native_backend_options_from_config())
    }
}

pub(crate) struct MacosNativeSpec<F = RealNativeApiFactory> {
    api_factory: F,
}

pub(crate) static MACOS_NATIVE_SPEC: MacosNativeSpec = MacosNativeSpec {
    api_factory: RealNativeApiFactory,
};

impl<F> WindowManagerSpec for MacosNativeSpec<F>
where
    F: MacosNativeApiFactory + Sync,
    F::Api: Send + 'static,
{
    fn backend(&self) -> WmBackend {
        WmBackend::MacosNative
    }

    fn name(&self) -> &'static str {
        MacosNativeAdapter::<F::Api>::NAME
    }

    fn connect(&self) -> anyhow::Result<ConfiguredWindowManager> {
        {
            let _span =
                tracing::debug_span!("macos_native.connect.validate_capabilities").entered();
            validate_declared_capabilities::<MacosNativeAdapter<F::Api>>()?;
        }
        let api = {
            let _span = tracing::debug_span!("macos_native.connect.real_api_new").entered();
            self.api_factory.create()
        };
        ConfiguredWindowManager::try_new(
            Box::new(MacosNativeAdapter::connect_with_api(api)?),
            WindowManagerFeatures::default(),
        )
    }

    fn floating_focus_mode(&self) -> FloatingFocusMode {
        MacosNativeAdapter::<F::Api>::FLOATING_FOCUS_MODE
    }

    fn focused_app_record(&self) -> anyhow::Result<Option<FocusedAppRecord>> {
        let api = {
            let _span = tracing::debug_span!("macos_native.fast_focus.real_api_new").entered();
            self.api_factory.create()
        };
        focused_app_record_with_api(&api)
    }
}

impl<A> MacosNativeAdapter<A>
where
    A: MacosNativeApi,
{
    pub(crate) fn connect_with_api(api: A) -> Result<Self, MacosNativeConnectError> {
        Ok(Self {
            ctx: MacosNativeContext::connect_with_api(api)?,
        })
    }
}

impl<A> WindowManagerCapabilityDescriptor for MacosNativeAdapter<A> {
    const NAME: &'static str = "macos_native";
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
    const FLOATING_FOCUS_MODE: FloatingFocusMode = FloatingFocusMode::FloatingOnly;
}

impl<A> WindowManagerSession for MacosNativeAdapter<A>
where
    A: MacosNativeApi + Send,
{
    fn adapter_name(&self) -> &'static str {
        Self::NAME
    }

    fn capabilities(&self) -> WindowManagerCapabilities {
        Self::CAPABILITIES
    }

    fn focused_window(&mut self) -> anyhow::Result<FocusedWindowRecord> {
        let snapshot = self.ctx.api.desktop_snapshot().map_err(map_probe_error)?;
        focused_window_record_from_native(&snapshot)
    }

    fn windows(&mut self) -> anyhow::Result<Vec<WindowRecord>> {
        let snapshot = self.ctx.api.desktop_snapshot().map_err(map_probe_error)?;
        Ok(window_records_from_native(&snapshot))
    }

    fn focus_direction(&mut self, direction: Direction) -> anyhow::Result<()> {
        let _span = tracing::debug_span!("macos_native.focus_direction", ?direction).entered();
        self.focus_direction_inner(direction)
    }
    fn move_direction(&mut self, direction: Direction) -> anyhow::Result<()> {
        let snapshot = self.ctx.api.desktop_snapshot().map_err(map_probe_error)?;
        let topology = outer_topology_from_native_snapshot(&snapshot)?;

        match select_move_target_from_outer_topology(&topology, direction)? {
            MoveTarget::NeighborSwap {
                source_window_id,
                source_frame,
                target_window_id,
                target_frame,
            } => self
                .ctx
                .api
                .swap_window_frames(
                    source_window_id,
                    native_bounds_from_outer(source_frame),
                    target_window_id,
                    native_bounds_from_outer(target_frame),
                )
                .map_err(anyhow::Error::new),
            MoveTarget::CrossSpace {
                window_id,
                target_space_id,
            } => self
                .ctx
                .api
                .move_window_to_space(window_id, target_space_id)
                .map_err(anyhow::Error::new),
        }
    }

    fn resize_with_intent(&mut self, intent: ResizeIntent) -> anyhow::Result<()> {
        bail!(
            "macos_native: resize {} is not implemented",
            intent.direction
        )
    }

    fn spawn(&mut self, command: Vec<String>) -> anyhow::Result<()> {
        if command.is_empty() {
            bail!("spawn: empty command");
        }
        let (program, args) = command.split_first().context("spawn: empty command")?;
        let args_refs: Vec<&str> = args.iter().map(|arg| arg.as_str()).collect();
        runtime::run_command_status(
            program,
            &args_refs,
            &CommandContext::new(Self::NAME, "spawn"),
        )
    }

    fn focus_window_by_id(&mut self, id: u64) -> anyhow::Result<()> {
        self.ctx
            .api
            .focus_window_by_id(id)
            .map_err(anyhow::Error::new)
    }

    fn close_window_by_id(&mut self, id: u64) -> anyhow::Result<()> {
        bail!("macos_native: close_window_by_id({id}) is not implemented")
    }
}

#[derive(Debug)]
pub(crate) struct MacosNativeContext<A = RealNativeApi> {
    api: A,
}

impl<A> MacosNativeContext<A>
where
    A: MacosNativeApi,
{
    pub(crate) fn connect_with_api(api: A) -> Result<Self, MacosNativeConnectError> {
        api.validate_environment()?;

        Ok(Self { api })
    }
}

#[derive(Debug, Clone, Copy)]
struct TracingDiagnostics;

impl NativeDiagnostics for TracingDiagnostics {
    fn debug(&self, message: &str) {
        logging::debug(message.to_owned());
    }
}

fn mission_control_hotkey_from_config(direction: Direction) -> MissionControlHotkey {
    let shortcut = config::macos_native_mission_control_shortcut(direction)
        .expect("macos_native mission control shortcuts should be validated at config load");
    MissionControlHotkey {
        key_code: shortcut.parse_keycode().expect(
            "macos_native mission control shortcut keycodes should be validated at config load",
        ),
        mission_control: MissionControlModifiers {
            control: shortcut.ctrl,
            option: shortcut.option,
            command: shortcut.command,
            shift: shortcut.shift,
            function: shortcut.r#fn,
        },
    }
}

fn native_direction_from_outer(direction: Direction) -> NativeDirection {
    match direction {
        Direction::West => NativeDirection::West,
        Direction::East => NativeDirection::East,
        Direction::North => NativeDirection::North,
        Direction::South => NativeDirection::South,
    }
}

fn native_backend_options_from_config() -> NativeBackendOptions {
    NativeBackendOptions {
        west_space_hotkey: mission_control_hotkey_from_config(Direction::West),
        east_space_hotkey: mission_control_hotkey_from_config(Direction::East),
        diagnostics: Some(std::sync::Arc::new(TracingDiagnostics)),
    }
}

fn map_probe_error(err: MacosNativeProbeError) -> anyhow::Error {
    match err {
        MacosNativeProbeError::MissingFocusedWindow => anyhow::anyhow!("no focused window"),
        other => anyhow::Error::new(other),
    }
}

fn focused_app_record_with_api<A: MacosNativeApi + ?Sized>(
    api: &A,
) -> anyhow::Result<Option<FocusedAppRecord>> {
    if {
        let _span = tracing::debug_span!("macos_native.fast_focus.ax_is_trusted").entered();
        !MacosNativeApi::ax_is_trusted(api)
    } {
        return Err(anyhow::anyhow!(
            "Accessibility permission is required for macOS native support"
        ));
    }
    if {
        let _span =
            tracing::debug_span!("macos_native.fast_focus.minimal_topology_ready").entered();
        !MacosNativeApi::minimal_topology_ready(api)
    } {
        return Err(anyhow::anyhow!(
            "macOS native topology precondition is unavailable: main SkyLight connection"
        ));
    }
    let snapshot = {
        let _span = tracing::debug_span!("macos_native.fast_focus.desktop_snapshot").entered();
        api.desktop_snapshot().map_err(map_probe_error)?
    };
    focused_app_record_from_native(&snapshot)
}

fn process_id_from_native(pid: Option<u32>) -> Option<ProcessId> {
    pid.and_then(ProcessId::new)
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OuterMacosTopology {
    spaces: Vec<OuterMacosSpace>,
    windows: Vec<OuterMacosWindow>,
    focused_window_id: Option<u64>,
    rects: Vec<DirectedRect<u64>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OuterMacosSpace {
    id: u64,
    display_index: usize,
    active: bool,
    kind: SpaceKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OuterMacosWindow {
    id: u64,
    pid: Option<u32>,
    space_id: u64,
    bounds: Option<Rect>,
    level: i32,
    order_index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FocusTarget {
    SameSpace { window_id: u64 },
    CrossSpace { target_space_id: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MoveTarget {
    NeighborSwap {
        source_window_id: u64,
        source_frame: Rect,
        target_window_id: u64,
        target_frame: Rect,
    },
    CrossSpace {
        window_id: u64,
        target_space_id: u64,
    },
}

fn native_bounds_from_outer(rect: Rect) -> NativeBounds {
    NativeBounds {
        x: rect.x,
        y: rect.y,
        width: rect.w,
        height: rect.h,
    }
}

#[allow(dead_code)]
fn rect_from_native(bounds: NativeBounds) -> Rect {
    Rect {
        x: bounds.x,
        y: bounds.y,
        w: bounds.width,
        h: bounds.height,
    }
}

#[allow(dead_code)]
fn outer_topology_from_native_snapshot(
    snapshot: &NativeDesktopSnapshot,
) -> anyhow::Result<OuterMacosTopology> {
    Ok(OuterMacosTopology {
        spaces: snapshot
            .spaces
            .iter()
            .map(|space| OuterMacosSpace {
                id: space.id,
                display_index: space.display_index,
                active: space.active,
                kind: space.kind,
            })
            .collect(),
        windows: snapshot
            .windows
            .iter()
            .map(|window| OuterMacosWindow {
                id: window.id,
                pid: window.pid,
                space_id: window.space_id,
                bounds: window.bounds.map(rect_from_native),
                level: window.level,
                order_index: window.order_index,
            })
            .collect(),
        focused_window_id: snapshot.focused_window_id,
        rects: snapshot
            .windows
            .iter()
            .filter(|window| snapshot.active_space_ids.contains(&window.space_id))
            .filter(|window| window.level == 0)
            .filter_map(|window| {
                window.bounds.map(|bounds| DirectedRect {
                    id: window.id,
                    rect: rect_from_native(bounds),
                })
            })
            .collect(),
    })
}

fn compare_outer_active_windows(
    left: &OuterMacosWindow,
    right: &OuterMacosWindow,
) -> std::cmp::Ordering {
    match (left.order_index, right.order_index) {
        (Some(left_index), Some(right_index)) => left_index.cmp(&right_index),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
    .then_with(|| left.id.cmp(&right.id))
}

fn resolved_outer_focused_window(
    topology: &OuterMacosTopology,
) -> anyhow::Result<&OuterMacosWindow> {
    if let Some(focused_window_id) = topology.focused_window_id {
        if let Some(window) = topology
            .windows
            .iter()
            .find(|window| window.id == focused_window_id)
        {
            return Ok(window);
        }
    }

    topology
        .windows
        .iter()
        .filter(|window| {
            topology
                .spaces
                .iter()
                .find(|space| space.id == window.space_id)
                .is_some_and(|space| space.active)
        })
        .min_by(|left, right| compare_outer_active_windows(left, right))
        .ok_or(MacosNativeProbeError::MissingFocusedWindow)
        .map_err(map_probe_error)
}

fn outer_space(topology: &OuterMacosTopology, space_id: u64) -> Option<&OuterMacosSpace> {
    topology.spaces.iter().find(|space| space.id == space_id)
}

fn outer_display_index_for_space(topology: &OuterMacosTopology, space_id: u64) -> Option<usize> {
    outer_space(topology, space_id).map(|space| space.display_index)
}

fn outer_windows_in_space<'a>(
    topology: &'a OuterMacosTopology,
    space_id: u64,
) -> Vec<&'a OuterMacosWindow> {
    topology
        .windows
        .iter()
        .filter(|window| window.space_id == space_id)
        .collect()
}

fn outer_focusable_windows_in_space<'a>(
    topology: &'a OuterMacosTopology,
    space_id: u64,
) -> Vec<&'a OuterMacosWindow> {
    outer_windows_in_space(topology, space_id)
        .into_iter()
        .filter(|window| window.level == 0)
        .collect()
}

fn outer_candidate_extends_in_direction(
    source: Rect,
    candidate: Rect,
    direction: Direction,
) -> bool {
    match direction {
        Direction::West => candidate.x < source.x,
        Direction::East => candidate.x + candidate.w > source.x + source.w,
        Direction::North => candidate.y < source.y,
        Direction::South => candidate.y + candidate.h > source.y + source.h,
    }
}

fn compare_outer_windows_for_edge(
    left: &OuterMacosWindow,
    right: &OuterMacosWindow,
    direction: Direction,
) -> std::cmp::Ordering {
    let left_bounds = left.bounds.expect("bounds should be present");
    let right_bounds = right.bounds.expect("bounds should be present");

    match direction {
        Direction::East => (left_bounds.x + left_bounds.w).cmp(&(right_bounds.x + right_bounds.w)),
        Direction::West => right_bounds.x.cmp(&left_bounds.x),
        Direction::North => right_bounds.y.cmp(&left_bounds.y),
        Direction::South => (left_bounds.y + left_bounds.h).cmp(&(right_bounds.y + right_bounds.h)),
    }
    .then_with(|| compare_outer_active_windows(right, left))
}

fn outer_same_space_focus_target(
    topology: &OuterMacosTopology,
    direction: Direction,
    strategy: crate::engine::topology::FloatingFocusStrategy,
) -> Option<u64> {
    let focused = resolved_outer_focused_window(topology).ok()?;

    let mut rects = outer_display_index_for_space(topology, focused.space_id)
        .map(|display_index| {
            topology
                .rects
                .iter()
                .filter(|rect| {
                    topology
                        .windows
                        .iter()
                        .find(|window| window.id == rect.id)
                        .is_some_and(|window| {
                            outer_display_index_for_space(topology, window.space_id)
                                == Some(display_index)
                        })
                })
                .cloned()
                .collect::<Vec<_>>()
        })
        .filter(|rects| !rects.is_empty())
        .unwrap_or_else(|| topology.rects.clone());
    if rects.iter().all(|rect| rect.id != focused.id) {
        if let Some(bounds) = focused.bounds {
            rects.push(DirectedRect {
                id: focused.id,
                rect: bounds,
            });
        }
    }
    let target_id = crate::engine::topology::select_closest_in_direction_with_strategy(
        &rects,
        focused.id,
        direction,
        Some(strategy),
    )?;

    if outer_should_escape_to_adjacent_space(topology, focused, direction, target_id) {
        return None;
    }

    Some(target_id)
}

fn outer_same_space_move_target(
    topology: &OuterMacosTopology,
    direction: Direction,
) -> anyhow::Result<Option<MoveTarget>> {
    let focused = resolved_outer_focused_window(topology)?;
    let rects = outer_display_index_for_space(topology, focused.space_id)
        .map(|display_index| {
            topology
                .rects
                .iter()
                .filter(|rect| {
                    topology
                        .windows
                        .iter()
                        .find(|window| window.id == rect.id)
                        .is_some_and(|window| {
                            outer_display_index_for_space(topology, window.space_id)
                                == Some(display_index)
                        })
                })
                .cloned()
                .collect::<Vec<_>>()
        })
        .filter(|rects| !rects.is_empty())
        .unwrap_or_else(|| topology.rects.clone());
    let Some(target_window_id) = crate::engine::topology::select_closest_in_direction_with_strategy(
        &rects, focused.id, direction, None,
    ) else {
        return Ok(None);
    };

    if outer_should_escape_to_adjacent_space(topology, focused, direction, target_window_id) {
        return Ok(None);
    }

    let source_frame = focused.bounds.ok_or_else(|| {
        anyhow::Error::new(MacosNativeOperationError::MissingWindowFrame(focused.id))
    })?;
    let target_frame = topology
        .windows
        .iter()
        .find(|window| window.id == target_window_id)
        .and_then(|window| window.bounds)
        .ok_or_else(|| {
            anyhow::Error::new(MacosNativeOperationError::MissingWindowFrame(
                target_window_id,
            ))
        })?;

    Ok(Some(MoveTarget::NeighborSwap {
        source_window_id: focused.id,
        source_frame,
        target_window_id,
        target_frame,
    }))
}

fn outer_focused_window_is_on_outer_edge(
    topology: &OuterMacosTopology,
    focused: &OuterMacosWindow,
    direction: Direction,
) -> bool {
    let Some(focused_bounds) = focused.bounds else {
        return false;
    };
    let mut bounds = outer_focusable_windows_in_space(topology, focused.space_id)
        .into_iter()
        .filter_map(|window| window.bounds);

    let Some(extreme_edge) = bounds.next().map(|bounds| match direction {
        Direction::West => bounds.x,
        Direction::East => bounds.x + bounds.w,
        Direction::North => bounds.y,
        Direction::South => bounds.y + bounds.h,
    }) else {
        return false;
    };

    let extreme_edge = bounds.fold(extreme_edge, |current, bounds| {
        let candidate = match direction {
            Direction::West => bounds.x,
            Direction::East => bounds.x + bounds.w,
            Direction::North => bounds.y,
            Direction::South => bounds.y + bounds.h,
        };
        match direction {
            Direction::West | Direction::North => current.min(candidate),
            Direction::East | Direction::South => current.max(candidate),
        }
    });

    match direction {
        Direction::West => focused_bounds.x == extreme_edge,
        Direction::East => focused_bounds.x + focused_bounds.w == extreme_edge,
        Direction::North => focused_bounds.y == extreme_edge,
        Direction::South => focused_bounds.y + focused_bounds.h == extreme_edge,
    }
}

fn outer_should_escape_to_adjacent_space(
    topology: &OuterMacosTopology,
    focused: &OuterMacosWindow,
    direction: Direction,
    target_id: u64,
) -> bool {
    if outer_adjacent_space_in_direction(topology, focused.space_id, direction).is_none() {
        return false;
    }
    if !outer_focused_window_is_on_outer_edge(topology, focused, direction) {
        return false;
    }

    let Some(source_bounds) = focused.bounds else {
        return false;
    };
    let Some(target_bounds) = topology
        .windows
        .iter()
        .find(|window| window.id == target_id)
        .and_then(|window| window.bounds)
    else {
        return false;
    };

    !outer_candidate_extends_in_direction(source_bounds, target_bounds, direction)
}

fn outer_adjacent_space_in_direction(
    topology: &OuterMacosTopology,
    source_space_id: u64,
    direction: Direction,
) -> Option<u64> {
    let source_space = outer_space(topology, source_space_id)?;
    let display_spaces = topology
        .spaces
        .iter()
        .filter(|space| space.display_index == source_space.display_index)
        .collect::<Vec<_>>();
    let source_index = display_spaces
        .iter()
        .position(|space| space.id == source_space_id)?;

    match direction {
        Direction::West => display_spaces[..source_index]
            .iter()
            .rev()
            .find(|space| space.kind != SpaceKind::StageManagerOpaque)
            .map(|space| space.id),
        Direction::East => display_spaces[source_index + 1..]
            .iter()
            .find(|space| space.kind != SpaceKind::StageManagerOpaque)
            .map(|space| space.id),
        Direction::North | Direction::South => None,
    }
}

fn select_focus_target_from_outer_topology(
    topology: &OuterMacosTopology,
    direction: Direction,
    strategy: crate::engine::topology::FloatingFocusStrategy,
) -> anyhow::Result<FocusTarget> {
    let native_direction = native_direction_from_outer(direction);
    let focused = resolved_outer_focused_window(topology)?;
    let target_window_id = outer_same_space_focus_target(topology, direction, strategy);

    if let Some(window_id) = target_window_id {
        return Ok(FocusTarget::SameSpace { window_id });
    }

    let target_space_id = outer_adjacent_space_in_direction(topology, focused.space_id, direction)
        .ok_or_else(|| {
            anyhow::Error::new(MacosNativeOperationError::NoDirectionalFocusTarget(
                native_direction,
            ))
        })?;
    let target_space = outer_space(topology, target_space_id).ok_or_else(|| {
        anyhow::Error::new(MacosNativeOperationError::MissingSpace(target_space_id))
    })?;
    if target_space.kind == SpaceKind::StageManagerOpaque {
        return Err(anyhow::Error::new(
            MacosNativeOperationError::UnsupportedStageManagerSpace(target_space_id),
        ));
    }

    Ok(FocusTarget::CrossSpace { target_space_id })
}

fn select_move_target_from_outer_topology(
    topology: &OuterMacosTopology,
    direction: Direction,
) -> anyhow::Result<MoveTarget> {
    let native_direction = native_direction_from_outer(direction);
    let focused = resolved_outer_focused_window(topology)?;

    if let Some(target) = outer_same_space_move_target(topology, direction)? {
        return Ok(target);
    }

    let target_space_id = outer_adjacent_space_in_direction(topology, focused.space_id, direction)
        .ok_or_else(|| {
            anyhow::Error::new(MacosNativeOperationError::NoDirectionalMoveTarget(
                native_direction,
            ))
        })?;
    let target_space = outer_space(topology, target_space_id).ok_or_else(|| {
        anyhow::Error::new(MacosNativeOperationError::MissingSpace(target_space_id))
    })?;
    if target_space.kind == SpaceKind::StageManagerOpaque {
        return Err(anyhow::Error::new(
            MacosNativeOperationError::UnsupportedStageManagerSpace(target_space_id),
        ));
    }

    Ok(MoveTarget::CrossSpace {
        window_id: focused.id,
        target_space_id,
    })
}

fn outer_best_window_in_space(
    topology: &OuterMacosTopology,
    space_id: u64,
    direction: Direction,
) -> Option<&OuterMacosWindow> {
    let windows = outer_focusable_windows_in_space(topology, space_id);
    windows
        .iter()
        .copied()
        .filter(|window| window.bounds.is_some())
        .max_by(|left, right| compare_outer_windows_for_edge(left, right, direction))
        .or_else(|| {
            windows
                .iter()
                .copied()
                .min_by(|left, right| compare_outer_active_windows(left, right))
        })
}

fn outer_space_transition_window_ids(
    snapshot: &NativeDesktopSnapshot,
    target_space_id: u64,
) -> (Option<u64>, std::collections::HashSet<u64>) {
    let target_display_index = snapshot
        .spaces
        .iter()
        .find(|space| space.id == target_space_id)
        .map(|space| space.display_index);
    let source_space_id = target_display_index.and_then(|display_index| {
        snapshot
            .spaces
            .iter()
            .find(|space| {
                space.active && space.display_index == display_index && space.id != target_space_id
            })
            .map(|space| space.id)
    });
    let source_focus_window_id = snapshot.focused_window_id.filter(|window_id| {
        snapshot
            .windows
            .iter()
            .find(|window| window.id == *window_id)
            .map(|window| window.space_id)
            == source_space_id
    });
    let target_window_ids = snapshot
        .windows
        .iter()
        .filter(|window| window.space_id == target_space_id)
        .map(|window| window.id)
        .collect();

    (source_focus_window_id, target_window_ids)
}

fn compare_native_active_windows(
    left: &NativeWindowSnapshot,
    right: &NativeWindowSnapshot,
) -> std::cmp::Ordering {
    match (left.order_index, right.order_index) {
        (Some(left_index), Some(right_index)) => left_index.cmp(&right_index),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
    .then_with(|| left.id.cmp(&right.id))
}

fn resolved_focused_native_window(
    snapshot: &NativeDesktopSnapshot,
) -> anyhow::Result<&NativeWindowSnapshot> {
    let is_active_window =
        |window: &&NativeWindowSnapshot| snapshot.active_space_ids.contains(&window.space_id);

    if let Some(focused_window_id) = snapshot.focused_window_id {
        if let Some(window) = snapshot
            .windows
            .iter()
            .find(|window| window.id == focused_window_id)
        {
            return Ok(window);
        }
    }

    snapshot
        .windows
        .iter()
        .filter(is_active_window)
        .min_by(|left, right| compare_native_active_windows(left, right))
        .ok_or(MacosNativeProbeError::MissingFocusedWindow)
        .map_err(map_probe_error)
}

fn window_records_from_native(snapshot: &NativeDesktopSnapshot) -> Vec<WindowRecord> {
    let focused_window_id = resolved_focused_native_window(snapshot)
        .ok()
        .map(|window| window.id);

    snapshot
        .windows
        .iter()
        .map(|window| WindowRecord {
            id: window.id,
            app_id: window.app_id.clone(),
            title: window.title.clone(),
            pid: process_id_from_native(window.pid),
            is_focused: focused_window_id == Some(window.id),
            original_tile_index: window.order_index.unwrap_or(0),
        })
        .collect()
}

fn focused_window_record_from_native(
    snapshot: &NativeDesktopSnapshot,
) -> anyhow::Result<FocusedWindowRecord> {
    let focused = resolved_focused_native_window(snapshot)?;

    Ok(FocusedWindowRecord {
        id: focused.id,
        app_id: focused.app_id.clone(),
        title: focused.title.clone(),
        pid: process_id_from_native(focused.pid),
        original_tile_index: focused.order_index.unwrap_or(0),
    })
}

fn focused_app_record_from_native(
    snapshot: &NativeDesktopSnapshot,
) -> anyhow::Result<Option<FocusedAppRecord>> {
    let focused = focused_window_record_from_native(snapshot)?;

    Ok(Some(FocusedAppRecord {
        app_id: focused.app_id.unwrap_or_default(),
        title: focused.title.unwrap_or_default(),
        pid: focused
            .pid
            .ok_or(MacosNativeProbeError::MissingFocusedWindow)
            .map_err(map_probe_error)?,
    }))
}

impl<A> MacosNativeAdapter<A>
where
    A: MacosNativeApi,
{
    fn focus_direction_inner(&self, direction: Direction) -> anyhow::Result<()> {
        let strategy = config::macos_native_floating_focus_strategy()
            .expect("macos_native floating focus strategy should be validated at config load");
        let snapshot = self.ctx.api.desktop_snapshot().map_err(map_probe_error)?;
        let topology = outer_topology_from_native_snapshot(&snapshot)?;
        let native_direction = native_direction_from_outer(direction);

        match select_focus_target_from_outer_topology(&topology, direction, strategy)? {
            FocusTarget::SameSpace { window_id } => self
                .ctx
                .api
                .focus_same_space_target_in_snapshot(&snapshot, native_direction, window_id)
                .map_err(anyhow::Error::new),
            FocusTarget::CrossSpace { target_space_id } => {
                self.ctx
                    .api
                    .switch_space_in_snapshot(&snapshot, target_space_id, Some(native_direction))
                    .map_err(anyhow::Error::new)?;
                let switched_snapshot = self.ctx.api.desktop_snapshot().map_err(map_probe_error)?;
                let switched_topology = outer_topology_from_native_snapshot(&switched_snapshot)?;
                let Some(target) = outer_best_window_in_space(
                    &switched_topology,
                    target_space_id,
                    direction.opposite(),
                ) else {
                    logging::debug(format!(
                        "macos_native: switched to adjacent space {target_space_id} without focusable windows; treating focus as successful"
                    ));
                    return Ok(());
                };

                if let Some(pid) = target.pid {
                    let target_hint = target.bounds.map(|bounds| ActiveSpaceFocusTargetHint {
                        space_id: target.space_id,
                        bounds: native_bounds_from_outer(bounds),
                    });
                    self.ctx
                        .api
                        .focus_window_in_active_space_with_known_pid(target.id, pid, target_hint)
                        .map_err(anyhow::Error::new)
                } else {
                    self.ctx
                        .api
                        .focus_window(target.id)
                        .map_err(anyhow::Error::new)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::macos_window_manager_test_support::foundation::{
        CFArrayRef, CFDictionaryRef, CFTypeRef, K_CG_EVENT_FLAG_MASK_ALTERNATE,
        K_CG_EVENT_FLAG_MASK_COMMAND, K_CG_EVENT_FLAG_MASK_SHIFT, ProcessSerialNumber,
        cf_number_from_u64, cf_string, switch_adjacent_space_via_hotkey,
    };
    use super::macos_window_manager_test_support::tests::{
        SpaceSnapshot, dictionary_from_type_refs, focused_window_id_via_ax,
        space_snapshots_from_topology,
    };
    use super::macos_window_manager_test_support::window_server::{
        cg_window_bounds_key, cg_window_layer_key, cg_window_name_key, cg_window_number_key,
        cg_window_owner_pid_key, filter_window_descriptions_raw, parse_window_descriptions,
    };
    use super::macos_window_manager_test_support::{
        CfOwned, DESKTOP_SPACE_TYPE, FULLSCREEN_SPACE_TYPE, REQUIRED_PRIVATE_SYMBOLS,
        RawSpaceRecord, RawTopologySnapshot, RawWindow, SPACE_SWITCH_POLL_INTERVAL,
        SPACE_SWITCH_SETTLE_TIMEOUT, SPACE_SWITCH_STABLE_TARGET_POLLS, WindowSnapshot,
        array_from_type_refs, array_from_u64s, best_window_id_from_windows, classify_space,
        dictionary_i32, dictionary_string, enrich_real_window_app_ids_with,
        ensure_supported_target_space, focus_window_via_make_key_and_raise,
        focus_window_via_process_and_raise, focused_window_from_topology,
        native_desktop_snapshot_from_topology, number_from_u64, order_active_space_windows,
        parse_lsappinfo_bundle_identifier, parse_managed_spaces, parse_raw_space_record,
        snapshots_for_inactive_space, space_id_for_window, space_transition_window_ids,
        stable_app_id_from_real_window, string, validate_environment_with_api,
        window_ids_for_space, window_snapshots_from_topology,
    };
    use super::*;
    use crate::engine::topology::{Rect, select_closest_in_direction_with_strategy};
    use crate::logging;
    use core_foundation::base::TCFType;
    use std::time::Instant;
    use std::{
        cell::RefCell,
        collections::{BTreeSet, HashMap, HashSet, VecDeque},
        rc::Rc,
        sync::{Arc, Mutex},
    };

    impl<A> MacosNativeContext<A>
    where
        A: MacosNativeApi,
    {
        pub(crate) fn spaces(&self) -> Result<Vec<SpaceSnapshot>, MacosNativeProbeError> {
            let topology = self.topology_snapshot()?;
            Ok(space_snapshots_from_topology(&topology))
        }

        pub(crate) fn focused_window(&self) -> Result<WindowSnapshot, MacosNativeProbeError> {
            self.api.focused_window_snapshot()
        }

        pub(crate) fn switch_space(&self, space_id: u64) -> Result<(), MacosNativeOperationError> {
            let topology = self.topology_snapshot()?;
            self.switch_space_in_topology(&topology, space_id, None)
        }

        pub(crate) fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
            self.api.focus_window_by_id(window_id)
        }

        fn switch_space_in_topology(
            &self,
            topology: &RawTopologySnapshot,
            space_id: u64,
            adjacent_direction: Option<NativeDirection>,
        ) -> Result<(), MacosNativeOperationError> {
            ensure_supported_target_space(topology, space_id)?;

            if topology.active_space_ids.contains(&space_id) {
                return Ok(());
            }

            let (source_focus_window_id, target_window_ids) =
                space_transition_window_ids(topology, space_id);
            logging::debug(format!(
                "macos_native: switching to space {space_id} source_focus={:?} target_windows={}",
                source_focus_window_id,
                target_window_ids.len()
            ));
            if let Some(direction) = adjacent_direction {
                if target_window_ids.is_empty() {
                    logging::debug(format!(
                        "macos_native: using exact space switch for empty adjacent space {space_id}"
                    ));
                    self.api.switch_space(space_id)?;
                    return self.wait_for_space_presentation(
                        space_id,
                        source_focus_window_id,
                        &target_window_ids,
                    );
                }

                self.api.switch_adjacent_space(direction, space_id)?;
                match self.wait_for_space_presentation(
                    space_id,
                    source_focus_window_id,
                    &target_window_ids,
                ) {
                    Ok(()) => Ok(()),
                    Err(err) => {
                        let target_still_inactive = match self.api.active_space_ids() {
                            Ok(active_space_ids) => !active_space_ids.contains(&space_id),
                            Err(probe_err) => {
                                logging::debug(format!(
                                    "macos_native: failed to re-check active spaces after adjacent hotkey switch failure for space {space_id} ({probe_err}); retrying exact space switch"
                                ));
                                true
                            }
                        };

                        if !target_still_inactive {
                            return Err(err);
                        }

                        let retry_target_window_ids = match self.api.onscreen_window_ids() {
                            Ok(onscreen_window_ids)
                                if !target_window_ids.is_empty()
                                    && !target_window_ids.is_disjoint(&onscreen_window_ids) =>
                            {
                                logging::debug(format!(
                                    "macos_native: adjacent hotkey left target-space window ids visible while target space {space_id} is still inactive; treating target ids as unreliable for exact-switch retry"
                                ));
                                HashSet::new()
                            }
                            Ok(_) => target_window_ids.clone(),
                            Err(probe_err) => {
                                logging::debug(format!(
                                    "macos_native: failed to inspect onscreen windows after adjacent hotkey switch failure for space {space_id} ({probe_err}); preserving target ids for exact-switch retry"
                                ));
                                target_window_ids.clone()
                            }
                        };

                        logging::debug(format!(
                            "macos_native: adjacent hotkey did not activate target space {space_id}; retrying exact space switch"
                        ));
                        self.api.switch_space(space_id)?;
                        self.wait_for_space_presentation(
                            space_id,
                            source_focus_window_id,
                            &retry_target_window_ids,
                        )
                    }
                }
            } else {
                self.api.switch_space(space_id)?;
                self.wait_for_space_presentation(
                    space_id,
                    source_focus_window_id,
                    &target_window_ids,
                )
            }
        }

        fn wait_for_space_presentation(
            &self,
            space_id: u64,
            source_focus_window_id: Option<u64>,
            target_window_ids: &HashSet<u64>,
        ) -> Result<(), MacosNativeOperationError> {
            let _span =
                tracing::debug_span!("macos_native.wait_for_active_space", space_id).entered();
            let deadline = Instant::now() + SPACE_SWITCH_SETTLE_TIMEOUT;
            let mut polls = 0usize;
            let mut stable_target_polls = 0usize;

            loop {
                polls += 1;
                let active_space_ids = self.api.active_space_ids()?;
                let onscreen_window_ids = self.api.onscreen_window_ids()?;
                let target_active = active_space_ids.contains(&space_id);
                let source_focus_hidden = source_focus_window_id
                    .is_none_or(|window_id| !onscreen_window_ids.contains(&window_id));
                let target_visible = target_window_ids.is_empty()
                    || !target_window_ids.is_disjoint(&onscreen_window_ids);
                if target_active && target_visible {
                    stable_target_polls += 1;
                } else {
                    stable_target_polls = 0;
                }

                if target_active
                    && target_visible
                    && (source_focus_hidden
                        || stable_target_polls >= SPACE_SWITCH_STABLE_TARGET_POLLS)
                {
                    logging::debug(format!(
                        "macos_native: space {space_id} presentation settled after {polls} poll(s)"
                    ));
                    return Ok(());
                }

                if Instant::now() >= deadline {
                    logging::debug(format!(
                        "macos_native: space {space_id} did not settle after {polls} poll(s) target_active={target_active} source_focus_hidden={source_focus_hidden} target_visible={target_visible}"
                    ));
                    return Err(MacosNativeOperationError::CallFailed(
                        "wait_for_active_space",
                    ));
                }

                std::thread::sleep(SPACE_SWITCH_POLL_INTERVAL);
            }
        }

        pub(crate) fn move_window_to_space(
            &self,
            window_id: u64,
            space_id: u64,
        ) -> Result<(), MacosNativeOperationError> {
            let topology = self.topology_snapshot()?;
            space_id_for_window(&topology, window_id)
                .ok_or(MacosNativeOperationError::MissingWindow(window_id))?;
            ensure_supported_target_space(&topology, space_id)?;
            self.api.move_window_to_space(window_id, space_id)
        }

        fn topology_snapshot(&self) -> Result<RawTopologySnapshot, MacosNativeProbeError> {
            self.api.topology_snapshot()
        }
    }

    #[derive(Debug, Clone)]
    struct FakeNativeApi {
        symbols: BTreeSet<&'static str>,
        ax_trusted: bool,
        minimal_topology_ready: bool,
        validate_environment_override: Option<MacosNativeConnectError>,
        topology: RawTopologySnapshot,
        space_windows: HashMap<u64, Vec<RawWindow>>,
        calls: Rc<RefCell<Vec<String>>>,
    }

    impl Default for FakeNativeApi {
        fn default() -> Self {
            Self {
                symbols: REQUIRED_PRIVATE_SYMBOLS.iter().copied().collect(),
                ax_trusted: true,
                minimal_topology_ready: true,
                validate_environment_override: None,
                topology: Self::topology_fixture(41),
                space_windows: HashMap::new(),
                calls: Rc::new(RefCell::new(Vec::new())),
            }
        }
    }

    impl FakeNativeApi {
        fn topology_fixture(active_window_id: u64) -> RawTopologySnapshot {
            RawTopologySnapshot {
                spaces: vec![raw_desktop_space(1), raw_split_space(2, &[21, 22])],
                active_space_ids: HashSet::from([1]),
                active_space_windows: HashMap::from([(
                    1,
                    vec![
                        raw_window(active_window_id)
                            .with_visible_index(0)
                            .with_pid(4242)
                            .with_app_id("com.example.focused")
                            .with_title("Focused window"),
                    ],
                )]),
                inactive_space_window_ids: HashMap::from([(2, vec![21, 22])]),
                focused_window_id: Some(active_window_id),
            }
        }

        fn multi_display_topology_fixture() -> RawTopologySnapshot {
            RawTopologySnapshot {
                spaces: vec![
                    raw_desktop_space_on_display(1, 0),
                    raw_split_space_on_display(2, &[21, 22], 0),
                    raw_fullscreen_space_on_display(3, 1),
                ],
                active_space_ids: HashSet::from([1, 3]),
                active_space_windows: HashMap::from([
                    (
                        1,
                        vec![
                            raw_window(11)
                                .with_visible_index(2)
                                .with_pid(1111)
                                .with_app_id("com.example.left")
                                .with_title("Left display"),
                        ],
                    ),
                    (
                        3,
                        vec![
                            raw_window(31)
                                .with_visible_index(0)
                                .with_pid(3333)
                                .with_app_id("com.example.right")
                                .with_title("Right display"),
                        ],
                    ),
                ]),
                inactive_space_window_ids: HashMap::from([(2, vec![21, 22])]),
                focused_window_id: Some(31),
            }
        }

        fn without_symbol(mut self, symbol: &'static str) -> Self {
            self.symbols.remove(symbol);
            self
        }

        fn with_ax_trusted(mut self, ax_trusted: bool) -> Self {
            self.ax_trusted = ax_trusted;
            self
        }

        fn with_minimal_topology_ready(mut self, minimal_topology_ready: bool) -> Self {
            self.minimal_topology_ready = minimal_topology_ready;
            self
        }

        fn with_validate_environment_error(mut self, err: MacosNativeConnectError) -> Self {
            self.validate_environment_override = Some(err);
            self
        }

        fn with_topology(mut self, topology: RawTopologySnapshot) -> Self {
            self.topology = topology;
            self
        }

        fn with_calls(mut self, calls: Rc<RefCell<Vec<String>>>) -> Self {
            self.calls = calls;
            self
        }
    }

    #[derive(Debug, Clone)]
    struct SnapshotOverrideApi {
        topology: RawTopologySnapshot,
    }

    impl Default for SnapshotOverrideApi {
        fn default() -> Self {
            Self {
                topology: FakeNativeApi::multi_display_topology_fixture(),
            }
        }
    }

    impl MacosNativeApi for FakeNativeApi {
        fn has_symbol(&self, symbol: &'static str) -> bool {
            self.symbols.contains(symbol)
        }

        fn ax_is_trusted(&self) -> bool {
            self.ax_trusted
        }

        fn minimal_topology_ready(&self) -> bool {
            self.minimal_topology_ready
        }

        fn validate_environment(&self) -> Result<(), MacosNativeConnectError> {
            if let Some(err) = self.validate_environment_override {
                return Err(err);
            }

            validate_environment_with_api(self)
        }

        fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
            Ok(native_desktop_snapshot_from_topology(
                &self.topology_snapshot()?,
            ))
        }

        fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
            Ok(self.topology.spaces.clone())
        }

        fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            Ok(self.topology.active_space_ids.clone())
        }

        fn active_space_windows(
            &self,
            space_id: u64,
        ) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
            Ok(self
                .space_windows
                .get(&space_id)
                .cloned()
                .or_else(|| self.topology.active_space_windows.get(&space_id).cloned())
                .unwrap_or_default())
        }

        fn inactive_space_window_ids(
            &self,
        ) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
            Ok(self.topology.inactive_space_window_ids.clone())
        }

        fn switch_space(&self, space_id: u64) -> Result<(), MacosNativeOperationError> {
            self.calls
                .borrow_mut()
                .push(format!("switch_space:{space_id}"));
            Ok(())
        }

        fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
            self.calls
                .borrow_mut()
                .push(format!("focus_window:{window_id}"));
            Ok(())
        }

        fn move_window_to_space(
            &self,
            window_id: u64,
            space_id: u64,
        ) -> Result<(), MacosNativeOperationError> {
            self.calls
                .borrow_mut()
                .push(format!("move_window_to_space:{window_id}:{space_id}"));
            Ok(())
        }

        fn swap_window_frames(
            &self,
            source_window_id: u64,
            _source_frame: NativeBounds,
            target_window_id: u64,
            _target_frame: NativeBounds,
        ) -> Result<(), MacosNativeOperationError> {
            self.calls.borrow_mut().push(format!(
                "swap_window_frames:{source_window_id}:{target_window_id}"
            ));
            Ok(())
        }

        fn topology_snapshot(&self) -> Result<RawTopologySnapshot, MacosNativeProbeError> {
            Ok(self.topology.clone())
        }
    }

    impl MacosNativeApi for SnapshotOverrideApi {
        fn has_symbol(&self, symbol: &'static str) -> bool {
            REQUIRED_PRIVATE_SYMBOLS.contains(&symbol)
        }

        fn ax_is_trusted(&self) -> bool {
            true
        }

        fn minimal_topology_ready(&self) -> bool {
            true
        }

        fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
            Ok(native_desktop_snapshot_from_topology(
                &self.topology_snapshot()?,
            ))
        }

        fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
            Ok(vec![raw_stage_manager_space(99)])
        }

        fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            Ok(HashSet::from([99]))
        }

        fn active_space_windows(
            &self,
            _space_id: u64,
        ) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
            Ok(vec![raw_window(999).with_visible_index(0)])
        }

        fn inactive_space_window_ids(
            &self,
        ) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
            Ok(HashMap::new())
        }

        fn switch_space(&self, _space_id: u64) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }

        fn focus_window(&self, _window_id: u64) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }

        fn move_window_to_space(
            &self,
            _window_id: u64,
            _space_id: u64,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }

        fn swap_window_frames(
            &self,
            _source_window_id: u64,
            _source_frame: NativeBounds,
            _target_window_id: u64,
            _target_frame: NativeBounds,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }

        fn focused_window_snapshot(&self) -> Result<WindowSnapshot, MacosNativeProbeError> {
            focused_window_from_topology(&self.topology)
        }

        fn topology_snapshot(&self) -> Result<RawTopologySnapshot, MacosNativeProbeError> {
            Ok(self.topology.clone())
        }
    }

    #[derive(Debug, Clone)]
    struct SendRecordingApi {
        topology: RawTopologySnapshot,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl MacosNativeApi for SendRecordingApi {
        fn has_symbol(&self, _symbol: &'static str) -> bool {
            true
        }

        fn ax_is_trusted(&self) -> bool {
            true
        }

        fn minimal_topology_ready(&self) -> bool {
            true
        }

        fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
            Ok(native_desktop_snapshot_from_topology(
                &self.topology_snapshot()?,
            ))
        }

        fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
            Ok(self.topology.spaces.clone())
        }

        fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            Ok(self.topology.active_space_ids.clone())
        }

        fn active_space_windows(
            &self,
            space_id: u64,
        ) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
            Ok(self
                .topology
                .active_space_windows
                .get(&space_id)
                .cloned()
                .unwrap_or_default())
        }

        fn inactive_space_window_ids(
            &self,
        ) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
            Ok(self.topology.inactive_space_window_ids.clone())
        }

        fn focused_window_id(&self) -> Result<Option<u64>, MacosNativeProbeError> {
            Ok(self.topology.focused_window_id)
        }

        fn switch_space(&self, space_id: u64) -> Result<(), MacosNativeOperationError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("switch_space:{space_id}"));
            Ok(())
        }

        fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("focus_window:{window_id}"));
            Ok(())
        }

        fn move_window_to_space(
            &self,
            window_id: u64,
            space_id: u64,
        ) -> Result<(), MacosNativeOperationError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("move_window_to_space:{window_id}:{space_id}"));
            Ok(())
        }

        fn swap_window_frames(
            &self,
            source_window_id: u64,
            _source_frame: NativeBounds,
            target_window_id: u64,
            _target_frame: NativeBounds,
        ) -> Result<(), MacosNativeOperationError> {
            self.calls.lock().unwrap().push(format!(
                "swap_window_frames:{source_window_id}:{target_window_id}"
            ));
            Ok(())
        }

        fn topology_snapshot(&self) -> Result<RawTopologySnapshot, MacosNativeProbeError> {
            Ok(self.topology.clone())
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum NativeCall {
        DesktopSnapshot,
        SwitchSpaceInSnapshot(u64, Option<NativeDirection>),
        FocusSameSpaceTargetInSnapshot(NativeDirection, u64),
        FocusWindowWithPid(u64, u32),
        SwapWindowFrames { source: u64, target: u64 },
        MoveWindowToSpace { window_id: u64, space_id: u64 },
    }

    #[derive(Debug, Clone)]
    struct RecordingFocusApi {
        snapshot: NativeDesktopSnapshot,
        calls: Arc<Mutex<Vec<NativeCall>>>,
    }

    impl RecordingFocusApi {
        fn from_snapshot(snapshot: NativeDesktopSnapshot) -> Self {
            Self {
                snapshot,
                calls: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn api_calls(&self) -> Vec<NativeCall> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl MacosNativeApi for RecordingFocusApi {
        fn has_symbol(&self, _symbol: &'static str) -> bool {
            true
        }

        fn ax_is_trusted(&self) -> bool {
            true
        }

        fn minimal_topology_ready(&self) -> bool {
            true
        }

        fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
            self.calls.lock().unwrap().push(NativeCall::DesktopSnapshot);
            Ok(self.snapshot.clone())
        }

        fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
            panic!("recording focus api must not query managed_spaces")
        }

        fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            panic!("recording focus api must not query active_space_ids")
        }

        fn active_space_windows(
            &self,
            _space_id: u64,
        ) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
            panic!("recording focus api must not query active_space_windows")
        }

        fn inactive_space_window_ids(
            &self,
        ) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
            panic!("recording focus api must not query inactive_space_window_ids")
        }

        fn switch_space(&self, _space_id: u64) -> Result<(), MacosNativeOperationError> {
            panic!("recording focus api must not switch spaces in this test")
        }

        fn focus_window(&self, _window_id: u64) -> Result<(), MacosNativeOperationError> {
            panic!("recording focus api should focus with known pid in this test")
        }

        fn focus_window_with_known_pid(
            &self,
            window_id: u64,
            pid: u32,
        ) -> Result<(), MacosNativeOperationError> {
            self.calls
                .lock()
                .unwrap()
                .push(NativeCall::FocusWindowWithPid(window_id, pid));
            Ok(())
        }

        fn move_window_to_space(
            &self,
            _window_id: u64,
            _space_id: u64,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }

        fn swap_window_frames(
            &self,
            _source_window_id: u64,
            _source_frame: NativeBounds,
            _target_window_id: u64,
            _target_frame: NativeBounds,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }
    }

    #[derive(Debug, Clone)]
    struct RecordingCrossSpaceFocusApi {
        snapshots: Arc<Mutex<VecDeque<NativeDesktopSnapshot>>>,
        calls: Arc<Mutex<Vec<NativeCall>>>,
    }

    impl RecordingCrossSpaceFocusApi {
        fn from_snapshots(snapshots: impl IntoIterator<Item = NativeDesktopSnapshot>) -> Self {
            Self {
                snapshots: Arc::new(Mutex::new(snapshots.into_iter().collect())),
                calls: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn api_calls(&self) -> Vec<NativeCall> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl MacosNativeApi for RecordingCrossSpaceFocusApi {
        fn has_symbol(&self, _symbol: &'static str) -> bool {
            true
        }

        fn ax_is_trusted(&self) -> bool {
            true
        }

        fn minimal_topology_ready(&self) -> bool {
            true
        }

        fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
            self.calls.lock().unwrap().push(NativeCall::DesktopSnapshot);
            self.snapshots.lock().unwrap().pop_front().ok_or(
                MacosNativeProbeError::MissingTopology("recording cross-space focus snapshot"),
            )
        }

        fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
            panic!("cross-space focus api must not query managed_spaces")
        }

        fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            panic!("cross-space focus api must not query active_space_ids")
        }

        fn active_space_windows(
            &self,
            _space_id: u64,
        ) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
            panic!("cross-space focus api must not query active_space_windows")
        }

        fn inactive_space_window_ids(
            &self,
        ) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
            panic!("cross-space focus api must not query inactive_space_window_ids")
        }

        fn switch_space_in_snapshot(
            &self,
            _snapshot: &NativeDesktopSnapshot,
            space_id: u64,
            adjacent_direction: Option<NativeDirection>,
        ) -> Result<(), MacosNativeOperationError> {
            self.calls
                .lock()
                .unwrap()
                .push(NativeCall::SwitchSpaceInSnapshot(
                    space_id,
                    adjacent_direction,
                ));
            Ok(())
        }

        fn switch_space(&self, _space_id: u64) -> Result<(), MacosNativeOperationError> {
            panic!("outer focus routing should call switch_space_in_snapshot")
        }

        fn focus_window(&self, _window_id: u64) -> Result<(), MacosNativeOperationError> {
            panic!("empty destination space should not focus a window")
        }

        fn focus_window_with_known_pid(
            &self,
            window_id: u64,
            pid: u32,
        ) -> Result<(), MacosNativeOperationError> {
            self.calls
                .lock()
                .unwrap()
                .push(NativeCall::FocusWindowWithPid(window_id, pid));
            Ok(())
        }

        fn focus_window_in_active_space_with_known_pid(
            &self,
            window_id: u64,
            pid: u32,
            _target_hint: Option<ActiveSpaceFocusTargetHint>,
        ) -> Result<(), MacosNativeOperationError> {
            self.calls
                .lock()
                .unwrap()
                .push(NativeCall::FocusWindowWithPid(window_id, pid));
            Ok(())
        }

        fn move_window_to_space(
            &self,
            _window_id: u64,
            _space_id: u64,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }

        fn swap_window_frames(
            &self,
            _source_window_id: u64,
            _source_frame: NativeBounds,
            _target_window_id: u64,
            _target_frame: NativeBounds,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }
    }

    #[derive(Debug, Clone)]
    struct RecordingSameSpaceDelegationApi {
        snapshot: NativeDesktopSnapshot,
        calls: Arc<Mutex<Vec<NativeCall>>>,
    }

    impl RecordingSameSpaceDelegationApi {
        fn from_snapshot(snapshot: NativeDesktopSnapshot) -> Self {
            Self {
                snapshot,
                calls: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn api_calls(&self) -> Vec<NativeCall> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl MacosNativeApi for RecordingSameSpaceDelegationApi {
        fn has_symbol(&self, _symbol: &'static str) -> bool {
            true
        }

        fn ax_is_trusted(&self) -> bool {
            true
        }

        fn minimal_topology_ready(&self) -> bool {
            true
        }

        fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
            self.calls.lock().unwrap().push(NativeCall::DesktopSnapshot);
            Ok(self.snapshot.clone())
        }

        fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
            panic!("delegation api must not query managed_spaces")
        }

        fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            panic!("delegation api must not query active_space_ids")
        }

        fn active_space_windows(
            &self,
            _space_id: u64,
        ) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
            panic!("delegation api must not query active_space_windows")
        }

        fn inactive_space_window_ids(
            &self,
        ) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
            panic!("delegation api must not query inactive_space_window_ids")
        }

        fn switch_space(&self, _space_id: u64) -> Result<(), MacosNativeOperationError> {
            panic!("delegation api must not switch spaces in this test")
        }

        fn focus_window(&self, _window_id: u64) -> Result<(), MacosNativeOperationError> {
            panic!("outer focus routing should delegate same-space mechanics to backend helper")
        }

        fn focus_window_with_known_pid(
            &self,
            _window_id: u64,
            _pid: u32,
        ) -> Result<(), MacosNativeOperationError> {
            panic!("outer focus routing should not perform same-space native mechanics directly")
        }

        fn focus_same_space_target_in_snapshot(
            &self,
            _snapshot: &NativeDesktopSnapshot,
            direction: NativeDirection,
            target_window_id: u64,
        ) -> Result<(), MacosNativeOperationError> {
            self.calls
                .lock()
                .unwrap()
                .push(NativeCall::FocusSameSpaceTargetInSnapshot(
                    direction,
                    target_window_id,
                ));
            Ok(())
        }

        fn move_window_to_space(
            &self,
            _window_id: u64,
            _space_id: u64,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }

        fn swap_window_frames(
            &self,
            _source_window_id: u64,
            _source_frame: NativeBounds,
            _target_window_id: u64,
            _target_frame: NativeBounds,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }
    }

    #[derive(Debug, Clone)]
    struct SplitViewKnownPidFallbackApi {
        snapshot: NativeDesktopSnapshot,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl SplitViewKnownPidFallbackApi {
        fn from_snapshot(snapshot: NativeDesktopSnapshot) -> Self {
            Self {
                snapshot,
                calls: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn api_calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl MacosNativeApi for SplitViewKnownPidFallbackApi {
        fn has_symbol(&self, _symbol: &'static str) -> bool {
            true
        }

        fn ax_is_trusted(&self) -> bool {
            true
        }

        fn minimal_topology_ready(&self) -> bool {
            true
        }

        fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
            self.calls
                .lock()
                .unwrap()
                .push("desktop_snapshot".to_string());
            Ok(self.snapshot.clone())
        }

        fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
            panic!("split-view fallback api must not query managed_spaces")
        }

        fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            panic!("split-view fallback api must not query active_space_ids")
        }

        fn active_space_windows(
            &self,
            _space_id: u64,
        ) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
            panic!("split-view fallback api must not query active_space_windows")
        }

        fn inactive_space_window_ids(
            &self,
        ) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
            panic!("split-view fallback api must not query inactive_space_window_ids")
        }

        fn switch_space(&self, _space_id: u64) -> Result<(), MacosNativeOperationError> {
            panic!("split-view fallback api must not switch spaces")
        }

        fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("focus_window:{window_id}"));
            Ok(())
        }

        fn focus_window_with_known_pid(
            &self,
            window_id: u64,
            pid: u32,
        ) -> Result<(), MacosNativeOperationError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("focus_window_with_known_pid:{window_id}:{pid}"));
            Err(MacosNativeOperationError::MissingWindow(window_id))
        }

        fn move_window_to_space(
            &self,
            _window_id: u64,
            _space_id: u64,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }

        fn swap_window_frames(
            &self,
            _source_window_id: u64,
            _source_frame: NativeBounds,
            _target_window_id: u64,
            _target_frame: NativeBounds,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }
    }

    #[derive(Debug, Clone)]
    struct SplitViewSameAppPeerFallbackApi {
        snapshot: NativeDesktopSnapshot,
        ax_window_ids_by_pid: HashMap<u32, Vec<u64>>,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl SplitViewSameAppPeerFallbackApi {
        fn from_snapshot(
            snapshot: NativeDesktopSnapshot,
            ax_window_ids_by_pid: HashMap<u32, Vec<u64>>,
        ) -> Self {
            Self {
                snapshot,
                ax_window_ids_by_pid,
                calls: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn api_calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl MacosNativeApi for SplitViewSameAppPeerFallbackApi {
        fn has_symbol(&self, _symbol: &'static str) -> bool {
            true
        }

        fn ax_is_trusted(&self) -> bool {
            true
        }

        fn minimal_topology_ready(&self) -> bool {
            true
        }

        fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
            self.calls
                .lock()
                .unwrap()
                .push("desktop_snapshot".to_string());
            Ok(self.snapshot.clone())
        }

        fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
            panic!("same-app split-view fallback api must not query managed_spaces")
        }

        fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            panic!("same-app split-view fallback api must not query active_space_ids")
        }

        fn active_space_windows(
            &self,
            _space_id: u64,
        ) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
            panic!("same-app split-view fallback api must not query active_space_windows")
        }

        fn inactive_space_window_ids(
            &self,
        ) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
            panic!("same-app split-view fallback api must not query inactive_space_window_ids")
        }

        fn ax_window_ids_for_pid(&self, pid: u32) -> Result<Vec<u64>, MacosNativeOperationError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("ax_window_ids_for_pid:{pid}"));
            Ok(self
                .ax_window_ids_by_pid
                .get(&pid)
                .cloned()
                .unwrap_or_default())
        }

        fn switch_space(&self, _space_id: u64) -> Result<(), MacosNativeOperationError> {
            panic!("same-app split-view fallback api must not switch spaces")
        }

        fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("focus_window:{window_id}"));
            Err(MacosNativeOperationError::MissingWindow(window_id))
        }

        fn focus_window_with_known_pid(
            &self,
            window_id: u64,
            pid: u32,
        ) -> Result<(), MacosNativeOperationError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("focus_window_with_known_pid:{window_id}:{pid}"));
            if self
                .ax_window_ids_by_pid
                .get(&pid)
                .is_some_and(|ids| ids.contains(&window_id))
            {
                Ok(())
            } else {
                Err(MacosNativeOperationError::MissingWindow(window_id))
            }
        }

        fn move_window_to_space(
            &self,
            _window_id: u64,
            _space_id: u64,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }

        fn swap_window_frames(
            &self,
            _source_window_id: u64,
            _source_frame: NativeBounds,
            _target_window_id: u64,
            _target_frame: NativeBounds,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }
    }

    #[derive(Debug, Clone)]
    struct SplitViewRefreshedTargetFallbackApi {
        snapshots: Arc<Mutex<VecDeque<NativeDesktopSnapshot>>>,
        successful_focus_targets: HashSet<(u64, u32)>,
        ax_window_ids_by_pid: HashMap<u32, Vec<u64>>,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl SplitViewRefreshedTargetFallbackApi {
        fn from_snapshots(
            snapshots: Vec<NativeDesktopSnapshot>,
            successful_focus_targets: HashSet<(u64, u32)>,
        ) -> Self {
            Self::from_snapshots_with_ax_window_ids(
                snapshots,
                successful_focus_targets,
                HashMap::new(),
            )
        }

        fn from_snapshots_with_ax_window_ids(
            snapshots: Vec<NativeDesktopSnapshot>,
            successful_focus_targets: HashSet<(u64, u32)>,
            ax_window_ids_by_pid: HashMap<u32, Vec<u64>>,
        ) -> Self {
            Self {
                snapshots: Arc::new(Mutex::new(VecDeque::from(snapshots))),
                successful_focus_targets,
                ax_window_ids_by_pid,
                calls: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn api_calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl MacosNativeApi for SplitViewRefreshedTargetFallbackApi {
        fn has_symbol(&self, _symbol: &'static str) -> bool {
            true
        }

        fn ax_is_trusted(&self) -> bool {
            true
        }

        fn minimal_topology_ready(&self) -> bool {
            true
        }

        fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
            self.calls
                .lock()
                .unwrap()
                .push("desktop_snapshot".to_string());
            let mut snapshots = self.snapshots.lock().unwrap();
            let snapshot = snapshots
                .front()
                .cloned()
                .expect("refreshed-target fallback api must retain at least one snapshot");
            if snapshots.len() > 1 {
                snapshots.pop_front();
            }
            Ok(snapshot)
        }

        fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
            panic!("refreshed-target fallback api must not query managed_spaces")
        }

        fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            panic!("refreshed-target fallback api must not query active_space_ids")
        }

        fn active_space_windows(
            &self,
            _space_id: u64,
        ) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
            panic!("refreshed-target fallback api must not query active_space_windows")
        }

        fn inactive_space_window_ids(
            &self,
        ) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
            panic!("refreshed-target fallback api must not query inactive_space_window_ids")
        }

        fn switch_space(&self, _space_id: u64) -> Result<(), MacosNativeOperationError> {
            panic!("refreshed-target fallback api must not switch spaces")
        }

        fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("focus_window:{window_id}"));
            Err(MacosNativeOperationError::MissingWindow(window_id))
        }

        fn focus_window_with_known_pid(
            &self,
            window_id: u64,
            pid: u32,
        ) -> Result<(), MacosNativeOperationError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("focus_window_with_known_pid:{window_id}:{pid}"));
            if self.successful_focus_targets.contains(&(window_id, pid)) {
                Ok(())
            } else {
                Err(MacosNativeOperationError::MissingWindow(window_id))
            }
        }

        fn ax_window_ids_for_pid(&self, pid: u32) -> Result<Vec<u64>, MacosNativeOperationError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("ax_window_ids_for_pid:{pid}"));
            Ok(self
                .ax_window_ids_by_pid
                .get(&pid)
                .cloned()
                .unwrap_or_default())
        }

        fn move_window_to_space(
            &self,
            _window_id: u64,
            _space_id: u64,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }

        fn swap_window_frames(
            &self,
            _source_window_id: u64,
            _source_frame: NativeBounds,
            _target_window_id: u64,
            _target_frame: NativeBounds,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }
    }

    #[derive(Debug, Clone)]
    struct RecordingMoveApi {
        snapshot: NativeDesktopSnapshot,
        calls: Arc<Mutex<Vec<NativeCall>>>,
    }

    impl RecordingMoveApi {
        fn from_snapshot(snapshot: NativeDesktopSnapshot) -> Self {
            Self {
                snapshot,
                calls: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn api_calls(&self) -> Vec<NativeCall> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl MacosNativeApi for RecordingMoveApi {
        fn has_symbol(&self, _symbol: &'static str) -> bool {
            true
        }

        fn ax_is_trusted(&self) -> bool {
            true
        }

        fn minimal_topology_ready(&self) -> bool {
            true
        }

        fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
            self.calls.lock().unwrap().push(NativeCall::DesktopSnapshot);
            Ok(self.snapshot.clone())
        }

        fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
            panic!("recording move api must not query managed_spaces")
        }

        fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            panic!("recording move api must not query active_space_ids")
        }

        fn active_space_windows(
            &self,
            _space_id: u64,
        ) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
            panic!("recording move api must not query active_space_windows")
        }

        fn inactive_space_window_ids(
            &self,
        ) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
            panic!("recording move api must not query inactive_space_window_ids")
        }

        fn switch_space(&self, _space_id: u64) -> Result<(), MacosNativeOperationError> {
            panic!("recording move api must not switch spaces in this test")
        }

        fn focus_window(&self, _window_id: u64) -> Result<(), MacosNativeOperationError> {
            panic!("recording move api must not focus windows in this test")
        }

        fn move_window_to_space(
            &self,
            window_id: u64,
            space_id: u64,
        ) -> Result<(), MacosNativeOperationError> {
            self.calls
                .lock()
                .unwrap()
                .push(NativeCall::MoveWindowToSpace {
                    window_id,
                    space_id,
                });
            Ok(())
        }

        fn swap_window_frames(
            &self,
            source_window_id: u64,
            _source_frame: NativeBounds,
            target_window_id: u64,
            _target_frame: NativeBounds,
        ) -> Result<(), MacosNativeOperationError> {
            self.calls
                .lock()
                .unwrap()
                .push(NativeCall::SwapWindowFrames {
                    source: source_window_id,
                    target: target_window_id,
                });
            Ok(())
        }
    }

    #[derive(Debug, Clone)]
    struct DirectOperationOverrideApi {
        topology: RawTopologySnapshot,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl MacosNativeApi for DirectOperationOverrideApi {
        fn has_symbol(&self, _symbol: &'static str) -> bool {
            true
        }

        fn ax_is_trusted(&self) -> bool {
            true
        }

        fn minimal_topology_ready(&self) -> bool {
            true
        }

        fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
            Ok(native_desktop_snapshot_from_topology(
                &self.topology_snapshot()?,
            ))
        }

        fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
            Ok(self.topology.spaces.clone())
        }

        fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            Ok(self.topology.active_space_ids.clone())
        }

        fn active_space_windows(
            &self,
            space_id: u64,
        ) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
            Ok(self
                .topology
                .active_space_windows
                .get(&space_id)
                .cloned()
                .unwrap_or_default())
        }

        fn inactive_space_window_ids(
            &self,
        ) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
            Ok(self.topology.inactive_space_window_ids.clone())
        }

        fn focused_window_id(&self) -> Result<Option<u64>, MacosNativeProbeError> {
            Ok(self.topology.focused_window_id)
        }

        fn focus_window_by_id(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("focus_window_by_id:{window_id}"));
            Ok(())
        }

        fn switch_space(&self, space_id: u64) -> Result<(), MacosNativeOperationError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("switch_space:{space_id}"));
            Ok(())
        }

        fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("focus_window:{window_id}"));
            Ok(())
        }

        fn move_window_to_space(
            &self,
            window_id: u64,
            space_id: u64,
        ) -> Result<(), MacosNativeOperationError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("move_window_to_space:{window_id}:{space_id}"));
            Ok(())
        }

        fn swap_window_frames(
            &self,
            source_window_id: u64,
            _source_frame: NativeBounds,
            target_window_id: u64,
            _target_frame: NativeBounds,
        ) -> Result<(), MacosNativeOperationError> {
            self.calls.lock().unwrap().push(format!(
                "swap_window_frames:{source_window_id}:{target_window_id}"
            ));
            Ok(())
        }

        fn topology_snapshot(&self) -> Result<RawTopologySnapshot, MacosNativeProbeError> {
            Ok(self.topology.clone())
        }
    }

    #[derive(Debug, Clone)]
    struct SpaceSettlingApi {
        topology: RawTopologySnapshot,
        calls: Rc<RefCell<Vec<String>>>,
        pending_space: Rc<RefCell<Option<u64>>>,
        stale_polls_remaining: Rc<RefCell<usize>>,
    }

    impl SpaceSettlingApi {
        fn new(
            topology: RawTopologySnapshot,
            calls: Rc<RefCell<Vec<String>>>,
            stale_polls_remaining: usize,
        ) -> Self {
            Self {
                topology,
                calls,
                pending_space: Rc::new(RefCell::new(None)),
                stale_polls_remaining: Rc::new(RefCell::new(stale_polls_remaining)),
            }
        }

        fn current_active_space_ids(&self) -> HashSet<u64> {
            let pending_space = *self.pending_space.borrow();
            let stale_polls_remaining = *self.stale_polls_remaining.borrow();
            match (pending_space, stale_polls_remaining) {
                (Some(space_id), 0) => HashSet::from([space_id]),
                _ => self.topology.active_space_ids.clone(),
            }
        }
    }

    impl MacosNativeApi for SpaceSettlingApi {
        fn has_symbol(&self, _symbol: &'static str) -> bool {
            true
        }

        fn ax_is_trusted(&self) -> bool {
            true
        }

        fn minimal_topology_ready(&self) -> bool {
            true
        }

        fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
            Ok(native_desktop_snapshot_from_topology(
                &self.topology_snapshot()?,
            ))
        }

        fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
            Ok(self.topology.spaces.clone())
        }

        fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            self.calls.borrow_mut().push("active_space_ids".to_string());

            if self.pending_space.borrow().is_some() {
                let mut stale_polls_remaining = self.stale_polls_remaining.borrow_mut();
                if *stale_polls_remaining > 0 {
                    *stale_polls_remaining -= 1;
                }
            }

            Ok(self.current_active_space_ids())
        }

        fn active_space_windows(
            &self,
            space_id: u64,
        ) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
            Ok(self
                .topology
                .active_space_windows
                .get(&space_id)
                .cloned()
                .unwrap_or_default())
        }

        fn inactive_space_window_ids(
            &self,
        ) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
            Ok(self.topology.inactive_space_window_ids.clone())
        }

        fn focused_window_id(&self) -> Result<Option<u64>, MacosNativeProbeError> {
            Ok(self.topology.focused_window_id)
        }

        fn onscreen_window_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            Ok(
                match (
                    *self.pending_space.borrow(),
                    *self.stale_polls_remaining.borrow(),
                ) {
                    (Some(space_id), 0) => window_ids_for_space(&self.topology, space_id),
                    _ => self
                        .topology
                        .active_space_ids
                        .iter()
                        .copied()
                        .flat_map(|space_id| window_ids_for_space(&self.topology, space_id))
                        .collect(),
                },
            )
        }

        fn switch_space(&self, space_id: u64) -> Result<(), MacosNativeOperationError> {
            self.calls
                .borrow_mut()
                .push(format!("switch_space:{space_id}"));
            *self.pending_space.borrow_mut() = Some(space_id);
            Ok(())
        }

        fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
            if let Some(target_space_id) = *self.pending_space.borrow() {
                if !self.current_active_space_ids().contains(&target_space_id) {
                    return Err(MacosNativeOperationError::CallFailed(
                        "focus_window_before_space_settled",
                    ));
                }
            }

            self.calls
                .borrow_mut()
                .push(format!("focus_window:{window_id}"));
            Ok(())
        }

        fn move_window_to_space(
            &self,
            window_id: u64,
            space_id: u64,
        ) -> Result<(), MacosNativeOperationError> {
            self.calls
                .borrow_mut()
                .push(format!("move_window_to_space:{window_id}:{space_id}"));
            Ok(())
        }

        fn swap_window_frames(
            &self,
            source_window_id: u64,
            _source_frame: NativeBounds,
            target_window_id: u64,
            _target_frame: NativeBounds,
        ) -> Result<(), MacosNativeOperationError> {
            self.calls.borrow_mut().push(format!(
                "swap_window_frames:{source_window_id}:{target_window_id}"
            ));
            Ok(())
        }
    }

    #[derive(Debug, Clone)]
    struct SpacePresentationApi {
        topology: RawTopologySnapshot,
        calls: Rc<RefCell<Vec<String>>>,
        pending_space: Rc<RefCell<Option<u64>>>,
        onscreen_sequences: Rc<RefCell<VecDeque<HashSet<u64>>>>,
    }

    impl SpacePresentationApi {
        fn new(
            topology: RawTopologySnapshot,
            calls: Rc<RefCell<Vec<String>>>,
            onscreen_sequences: Vec<HashSet<u64>>,
        ) -> Self {
            Self {
                topology,
                calls,
                pending_space: Rc::new(RefCell::new(None)),
                onscreen_sequences: Rc::new(RefCell::new(VecDeque::from(onscreen_sequences))),
            }
        }

        fn current_active_space_ids(&self) -> HashSet<u64> {
            (*self.pending_space.borrow())
                .map(|space_id| HashSet::from([space_id]))
                .unwrap_or_else(|| self.topology.active_space_ids.clone())
        }
    }

    impl MacosNativeApi for SpacePresentationApi {
        fn has_symbol(&self, _symbol: &'static str) -> bool {
            true
        }

        fn ax_is_trusted(&self) -> bool {
            true
        }

        fn minimal_topology_ready(&self) -> bool {
            true
        }

        fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
            Ok(native_desktop_snapshot_from_topology(
                &self.topology_snapshot()?,
            ))
        }

        fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
            Ok(self.topology.spaces.clone())
        }

        fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            self.calls.borrow_mut().push("active_space_ids".to_string());
            Ok(self.current_active_space_ids())
        }

        fn active_space_windows(
            &self,
            space_id: u64,
        ) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
            Ok(self
                .topology
                .active_space_windows
                .get(&space_id)
                .cloned()
                .unwrap_or_default())
        }

        fn inactive_space_window_ids(
            &self,
        ) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
            Ok(self.topology.inactive_space_window_ids.clone())
        }

        fn focused_window_id(&self) -> Result<Option<u64>, MacosNativeProbeError> {
            Ok(self.topology.focused_window_id)
        }

        fn onscreen_window_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            self.calls
                .borrow_mut()
                .push("onscreen_window_ids".to_string());
            let mut sequences = self.onscreen_sequences.borrow_mut();
            let current = sequences.front().cloned().unwrap_or_default();
            if sequences.len() > 1 {
                sequences.pop_front();
            }
            Ok(current)
        }

        fn switch_space(&self, space_id: u64) -> Result<(), MacosNativeOperationError> {
            self.calls
                .borrow_mut()
                .push(format!("switch_space:{space_id}"));
            *self.pending_space.borrow_mut() = Some(space_id);
            Ok(())
        }

        fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
            self.calls
                .borrow_mut()
                .push(format!("focus_window:{window_id}"));
            Ok(())
        }

        fn move_window_to_space(
            &self,
            window_id: u64,
            space_id: u64,
        ) -> Result<(), MacosNativeOperationError> {
            self.calls
                .borrow_mut()
                .push(format!("move_window_to_space:{window_id}:{space_id}"));
            Ok(())
        }

        fn swap_window_frames(
            &self,
            source_window_id: u64,
            _source_frame: NativeBounds,
            target_window_id: u64,
            _target_frame: NativeBounds,
        ) -> Result<(), MacosNativeOperationError> {
            self.calls.borrow_mut().push(format!(
                "swap_window_frames:{source_window_id}:{target_window_id}"
            ));
            Ok(())
        }
    }

    #[derive(Debug, Clone)]
    struct KnownPidAfterSwitchApi {
        topology: RawTopologySnapshot,
        calls: Rc<RefCell<Vec<String>>>,
        current_space_id: Rc<RefCell<u64>>,
    }

    impl KnownPidAfterSwitchApi {
        fn new(topology: RawTopologySnapshot, calls: Rc<RefCell<Vec<String>>>) -> Self {
            let current_space_id = topology
                .active_space_ids
                .iter()
                .copied()
                .next()
                .expect("topology should expose one active space for test");
            Self {
                topology,
                calls,
                current_space_id: Rc::new(RefCell::new(current_space_id)),
            }
        }
    }

    impl MacosNativeApi for KnownPidAfterSwitchApi {
        fn has_symbol(&self, _symbol: &'static str) -> bool {
            true
        }

        fn ax_is_trusted(&self) -> bool {
            true
        }

        fn minimal_topology_ready(&self) -> bool {
            true
        }

        fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
            Ok(native_desktop_snapshot_from_topology(
                &self.topology_snapshot()?,
            ))
        }

        fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
            Ok(self.topology.spaces.clone())
        }

        fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            Ok(HashSet::from([*self.current_space_id.borrow()]))
        }

        fn active_space_windows(
            &self,
            space_id: u64,
        ) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
            if *self.current_space_id.borrow() == 9 && space_id == 9 {
                return Ok(vec![raw_window(77).with_visible_index(0).with_pid(5151)]);
            }
            Ok(self
                .topology
                .active_space_windows
                .get(&space_id)
                .cloned()
                .unwrap_or_default())
        }

        fn inactive_space_window_ids(
            &self,
        ) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
            Ok(self.topology.inactive_space_window_ids.clone())
        }

        fn focused_window_id(&self) -> Result<Option<u64>, MacosNativeProbeError> {
            Ok(self
                .active_space_windows(*self.current_space_id.borrow())?
                .first()
                .map(|window| window.id))
        }

        fn onscreen_window_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            Ok(self
                .active_space_windows(*self.current_space_id.borrow())?
                .into_iter()
                .map(|window| window.id)
                .collect())
        }

        fn switch_space(&self, space_id: u64) -> Result<(), MacosNativeOperationError> {
            self.calls
                .borrow_mut()
                .push(format!("switch_space:{space_id}"));
            *self.current_space_id.borrow_mut() = space_id;
            Ok(())
        }

        fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
            Err(MacosNativeOperationError::MissingWindow(window_id))
        }

        fn focus_window_with_known_pid(
            &self,
            window_id: u64,
            pid: u32,
        ) -> Result<(), MacosNativeOperationError> {
            self.calls
                .borrow_mut()
                .push(format!("focus_window_with_known_pid:{window_id}:{pid}"));
            Ok(())
        }

        fn move_window_to_space(
            &self,
            _window_id: u64,
            _space_id: u64,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }

        fn swap_window_frames(
            &self,
            _source_window_id: u64,
            _source_frame: NativeBounds,
            _target_window_id: u64,
            _target_frame: NativeBounds,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }
    }

    #[derive(Debug, Clone)]
    struct PostSwitchSelectionDriftApi {
        initial_topology: RawTopologySnapshot,
        switched_topology: RawTopologySnapshot,
        drifted_windows: Vec<RawWindow>,
        calls: Rc<RefCell<Vec<String>>>,
        current_space_id: Rc<RefCell<u64>>,
    }

    impl PostSwitchSelectionDriftApi {
        fn new(
            initial_topology: RawTopologySnapshot,
            switched_topology: RawTopologySnapshot,
            drifted_windows: Vec<RawWindow>,
            calls: Rc<RefCell<Vec<String>>>,
        ) -> Self {
            Self {
                initial_topology,
                switched_topology,
                drifted_windows,
                calls,
                current_space_id: Rc::new(RefCell::new(1)),
            }
        }
    }

    impl MacosNativeApi for PostSwitchSelectionDriftApi {
        fn has_symbol(&self, _symbol: &'static str) -> bool {
            true
        }

        fn ax_is_trusted(&self) -> bool {
            true
        }

        fn minimal_topology_ready(&self) -> bool {
            true
        }

        fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
            Ok(native_desktop_snapshot_from_topology(
                &self.topology_snapshot()?,
            ))
        }

        fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
            Ok(self.initial_topology.spaces.clone())
        }

        fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            Ok(HashSet::from([*self.current_space_id.borrow()]))
        }

        fn active_space_windows(
            &self,
            space_id: u64,
        ) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
            if *self.current_space_id.borrow() == 2 && space_id == 2 {
                return Ok(self.drifted_windows.clone());
            }
            Ok(self
                .initial_topology
                .active_space_windows
                .get(&space_id)
                .cloned()
                .unwrap_or_default())
        }

        fn inactive_space_window_ids(
            &self,
        ) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
            Ok(self.initial_topology.inactive_space_window_ids.clone())
        }

        fn focused_window_id(&self) -> Result<Option<u64>, MacosNativeProbeError> {
            Ok(if *self.current_space_id.borrow() == 2 {
                self.switched_topology.focused_window_id
            } else {
                self.initial_topology.focused_window_id
            })
        }

        fn onscreen_window_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            Ok(if *self.current_space_id.borrow() == 2 {
                self.switched_topology
                    .active_space_windows
                    .values()
                    .flat_map(|windows| windows.iter().map(|window| window.id))
                    .collect()
            } else {
                self.initial_topology
                    .active_space_windows
                    .values()
                    .flat_map(|windows| windows.iter().map(|window| window.id))
                    .collect()
            })
        }

        fn switch_space(&self, space_id: u64) -> Result<(), MacosNativeOperationError> {
            self.calls
                .borrow_mut()
                .push(format!("switch_space:{space_id}"));
            *self.current_space_id.borrow_mut() = space_id;
            Ok(())
        }

        fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
            self.calls
                .borrow_mut()
                .push(format!("focus_window:{window_id}"));
            Ok(())
        }

        fn move_window_to_space(
            &self,
            _window_id: u64,
            _space_id: u64,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }

        fn swap_window_frames(
            &self,
            _source_window_id: u64,
            _source_frame: NativeBounds,
            _target_window_id: u64,
            _target_frame: NativeBounds,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }

        fn topology_snapshot(&self) -> Result<RawTopologySnapshot, MacosNativeProbeError> {
            Ok(if *self.current_space_id.borrow() == 2 {
                self.switched_topology.clone()
            } else {
                self.initial_topology.clone()
            })
        }
    }

    #[derive(Debug, Clone)]
    struct DirectOffSpaceFocusApi {
        topology: RawTopologySnapshot,
        described_space_windows: HashMap<u64, Vec<RawWindow>>,
        current_space_id: Rc<RefCell<u64>>,
        calls: Rc<RefCell<Vec<String>>>,
    }

    impl MacosNativeApi for DirectOffSpaceFocusApi {
        fn has_symbol(&self, _symbol: &'static str) -> bool {
            true
        }

        fn ax_is_trusted(&self) -> bool {
            true
        }

        fn minimal_topology_ready(&self) -> bool {
            true
        }

        fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
            Ok(native_desktop_snapshot_from_topology(
                &self.topology_snapshot()?,
            ))
        }

        fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
            Ok(self.topology.spaces.clone())
        }

        fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            Ok(HashSet::from([*self.current_space_id.borrow()]))
        }

        fn active_space_windows(
            &self,
            space_id: u64,
        ) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
            if *self.current_space_id.borrow() == space_id {
                if let Some(windows) = self.described_space_windows.get(&space_id) {
                    return Ok(windows.clone());
                }
            }
            Ok(self
                .topology
                .active_space_windows
                .get(&space_id)
                .cloned()
                .unwrap_or_default())
        }

        fn inactive_space_window_ids(
            &self,
        ) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
            Ok(self.topology.inactive_space_window_ids.clone())
        }

        fn focused_window_id(&self) -> Result<Option<u64>, MacosNativeProbeError> {
            Ok(self
                .active_space_windows(*self.current_space_id.borrow())?
                .first()
                .map(|window| window.id))
        }

        fn onscreen_window_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            Ok(self
                .active_space_windows(*self.current_space_id.borrow())?
                .into_iter()
                .map(|window| window.id)
                .collect())
        }

        fn switch_space(&self, space_id: u64) -> Result<(), MacosNativeOperationError> {
            self.calls
                .borrow_mut()
                .push(format!("switch_space:{space_id}"));
            *self.current_space_id.borrow_mut() = space_id;
            Ok(())
        }

        fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
            self.calls
                .borrow_mut()
                .push(format!("focus_window:{window_id}"));
            Ok(())
        }

        fn focus_window_with_known_pid(
            &self,
            window_id: u64,
            pid: u32,
        ) -> Result<(), MacosNativeOperationError> {
            self.calls
                .borrow_mut()
                .push(format!("focus_window_with_known_pid:{window_id}:{pid}"));
            Ok(())
        }

        fn move_window_to_space(
            &self,
            _window_id: u64,
            _space_id: u64,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }

        fn swap_window_frames(
            &self,
            _source_window_id: u64,
            _source_frame: NativeBounds,
            _target_window_id: u64,
            _target_frame: NativeBounds,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }
    }

    struct FocusedIdTopologyApi;

    impl MacosNativeApi for FocusedIdTopologyApi {
        fn has_symbol(&self, _symbol: &'static str) -> bool {
            true
        }

        fn ax_is_trusted(&self) -> bool {
            true
        }

        fn minimal_topology_ready(&self) -> bool {
            true
        }

        fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
            Ok(native_desktop_snapshot_from_topology(
                &self.topology_snapshot()?,
            ))
        }

        fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
            Ok(vec![raw_desktop_space(1)])
        }

        fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            Ok(HashSet::from([1]))
        }

        fn active_space_windows(
            &self,
            _space_id: u64,
        ) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
            Ok(vec![raw_window(11).with_visible_index(0)])
        }

        fn inactive_space_window_ids(
            &self,
        ) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
            Ok(HashMap::new())
        }

        fn focused_window_id(&self) -> Result<Option<u64>, MacosNativeProbeError> {
            Ok(Some(11))
        }

        fn switch_space(&self, _space_id: u64) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }

        fn focus_window(&self, _window_id: u64) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }

        fn move_window_to_space(
            &self,
            _window_id: u64,
            _space_id: u64,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }

        fn swap_window_frames(
            &self,
            _source_window_id: u64,
            _source_frame: NativeBounds,
            _target_window_id: u64,
            _target_frame: NativeBounds,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }
    }

    #[derive(Debug, Clone)]
    struct SamePidAxFallbackApi {
        topology: RawTopologySnapshot,
        ax_backed_window_ids: Vec<u64>,
        calls: Rc<RefCell<Vec<String>>>,
    }

    impl MacosNativeApi for SamePidAxFallbackApi {
        fn has_symbol(&self, _symbol: &'static str) -> bool {
            true
        }

        fn ax_is_trusted(&self) -> bool {
            true
        }

        fn minimal_topology_ready(&self) -> bool {
            true
        }

        fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
            Ok(native_desktop_snapshot_from_topology(
                &self.topology_snapshot()?,
            ))
        }

        fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
            Ok(self.topology.spaces.clone())
        }

        fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            Ok(self.topology.active_space_ids.clone())
        }

        fn active_space_windows(
            &self,
            space_id: u64,
        ) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
            Ok(self
                .topology
                .active_space_windows
                .get(&space_id)
                .cloned()
                .unwrap_or_default())
        }

        fn inactive_space_window_ids(
            &self,
        ) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
            Ok(self.topology.inactive_space_window_ids.clone())
        }

        fn focused_window_id(&self) -> Result<Option<u64>, MacosNativeProbeError> {
            Ok(self.topology.focused_window_id)
        }

        fn ax_window_ids_for_pid(&self, _pid: u32) -> Result<Vec<u64>, MacosNativeOperationError> {
            Ok(self.ax_backed_window_ids.clone())
        }

        fn switch_space(&self, _space_id: u64) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }

        fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
            Err(MacosNativeOperationError::MissingWindow(window_id))
        }

        fn focus_window_with_known_pid(
            &self,
            window_id: u64,
            pid: u32,
        ) -> Result<(), MacosNativeOperationError> {
            self.calls
                .borrow_mut()
                .push(format!("focus_window_with_known_pid:{window_id}:{pid}"));
            if self.ax_backed_window_ids.contains(&window_id) {
                Ok(())
            } else {
                Err(MacosNativeOperationError::MissingWindow(window_id))
            }
        }

        fn move_window_to_space(
            &self,
            _window_id: u64,
            _space_id: u64,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }

        fn swap_window_frames(
            &self,
            _source_window_id: u64,
            _source_frame: NativeBounds,
            _target_window_id: u64,
            _target_frame: NativeBounds,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }
    }

    #[derive(Debug, Clone)]
    struct SequencedSamePidAxFallbackApi {
        planning_topology: RawTopologySnapshot,
        execution_topology: RawTopologySnapshot,
        ax_backed_window_ids: Vec<u64>,
        calls: Arc<Mutex<Vec<String>>>,
        topology_snapshot_calls: Arc<Mutex<usize>>,
    }

    impl SequencedSamePidAxFallbackApi {
        fn current_topology(&self) -> RawTopologySnapshot {
            if *self.topology_snapshot_calls.lock().unwrap() > 0 {
                self.execution_topology.clone()
            } else {
                self.planning_topology.clone()
            }
        }
    }

    impl MacosNativeApi for SequencedSamePidAxFallbackApi {
        fn has_symbol(&self, _symbol: &'static str) -> bool {
            true
        }

        fn ax_is_trusted(&self) -> bool {
            true
        }

        fn minimal_topology_ready(&self) -> bool {
            true
        }

        fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
            Ok(native_desktop_snapshot_from_topology(
                &self.topology_snapshot()?,
            ))
        }

        fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
            Ok(self.current_topology().spaces)
        }

        fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            Ok(self.current_topology().active_space_ids)
        }

        fn active_space_windows(
            &self,
            space_id: u64,
        ) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
            Ok(self
                .current_topology()
                .active_space_windows
                .get(&space_id)
                .cloned()
                .unwrap_or_default())
        }

        fn inactive_space_window_ids(
            &self,
        ) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
            Ok(self.current_topology().inactive_space_window_ids)
        }

        fn focused_window_id(&self) -> Result<Option<u64>, MacosNativeProbeError> {
            Ok(self.current_topology().focused_window_id)
        }

        fn ax_window_ids_for_pid(&self, _pid: u32) -> Result<Vec<u64>, MacosNativeOperationError> {
            Ok(self.ax_backed_window_ids.clone())
        }

        fn switch_space(&self, _space_id: u64) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }

        fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
            Err(MacosNativeOperationError::MissingWindow(window_id))
        }

        fn focus_window_with_known_pid(
            &self,
            window_id: u64,
            pid: u32,
        ) -> Result<(), MacosNativeOperationError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("focus_window_with_known_pid:{window_id}:{pid}"));
            if self.ax_backed_window_ids.contains(&window_id) {
                Ok(())
            } else {
                Err(MacosNativeOperationError::MissingWindow(window_id))
            }
        }

        fn move_window_to_space(
            &self,
            _window_id: u64,
            _space_id: u64,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }

        fn swap_window_frames(
            &self,
            _source_window_id: u64,
            _source_frame: NativeBounds,
            _target_window_id: u64,
            _target_frame: NativeBounds,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }

        fn topology_snapshot(&self) -> Result<RawTopologySnapshot, MacosNativeProbeError> {
            let snapshot = self.current_topology();
            *self.topology_snapshot_calls.lock().unwrap() += 1;
            Ok(snapshot)
        }
    }

    #[derive(Debug, Clone)]
    struct SwitchThenFocusApi {
        topology: RawTopologySnapshot,
        switched_space_windows: HashMap<u64, Vec<RawWindow>>,
        current_space_id: Rc<RefCell<u64>>,
        calls: Rc<RefCell<Vec<String>>>,
    }

    impl MacosNativeApi for SwitchThenFocusApi {
        fn has_symbol(&self, _symbol: &'static str) -> bool {
            true
        }

        fn ax_is_trusted(&self) -> bool {
            true
        }

        fn minimal_topology_ready(&self) -> bool {
            true
        }

        fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
            Ok(native_desktop_snapshot_from_topology(
                &self.topology_snapshot()?,
            ))
        }

        fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
            Ok(self.topology.spaces.clone())
        }

        fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            Ok(HashSet::from([*self.current_space_id.borrow()]))
        }

        fn active_space_windows(
            &self,
            space_id: u64,
        ) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
            if *self.current_space_id.borrow() == space_id {
                if let Some(windows) = self.switched_space_windows.get(&space_id) {
                    return Ok(windows.clone());
                }
            }
            Ok(self
                .topology
                .active_space_windows
                .get(&space_id)
                .cloned()
                .unwrap_or_default())
        }

        fn inactive_space_window_ids(
            &self,
        ) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
            Ok(self.topology.inactive_space_window_ids.clone())
        }

        fn focused_window_id(&self) -> Result<Option<u64>, MacosNativeProbeError> {
            Ok(self
                .active_space_windows(*self.current_space_id.borrow())?
                .first()
                .map(|window| window.id))
        }

        fn onscreen_window_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            Ok(self
                .active_space_windows(*self.current_space_id.borrow())?
                .into_iter()
                .map(|window| window.id)
                .collect())
        }

        fn switch_space(&self, space_id: u64) -> Result<(), MacosNativeOperationError> {
            self.calls
                .borrow_mut()
                .push(format!("switch_space:{space_id}"));
            *self.current_space_id.borrow_mut() = space_id;
            Ok(())
        }

        fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
            if !self
                .switched_space_windows
                .contains_key(&*self.current_space_id.borrow())
            {
                return Err(MacosNativeOperationError::MissingWindow(window_id));
            }
            self.calls
                .borrow_mut()
                .push(format!("focus_window:{window_id}"));
            Ok(())
        }

        fn move_window_to_space(
            &self,
            _window_id: u64,
            _space_id: u64,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }

        fn swap_window_frames(
            &self,
            _source_window_id: u64,
            _source_frame: NativeBounds,
            _target_window_id: u64,
            _target_frame: NativeBounds,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }
    }

    #[derive(Debug, Clone)]
    struct PostSwitchFocuslessSnapshotApi {
        topology: RawTopologySnapshot,
        switched_space_windows: HashMap<u64, Vec<RawWindow>>,
        current_space_id: Rc<RefCell<u64>>,
        calls: Rc<RefCell<Vec<String>>>,
    }

    impl MacosNativeApi for PostSwitchFocuslessSnapshotApi {
        fn has_symbol(&self, _symbol: &'static str) -> bool {
            true
        }

        fn ax_is_trusted(&self) -> bool {
            true
        }

        fn minimal_topology_ready(&self) -> bool {
            true
        }

        fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
            Ok(native_desktop_snapshot_from_topology(
                &self.topology_snapshot()?,
            ))
        }

        fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
            Ok(self.topology.spaces.clone())
        }

        fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            Ok(HashSet::from([*self.current_space_id.borrow()]))
        }

        fn active_space_windows(
            &self,
            space_id: u64,
        ) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
            if *self.current_space_id.borrow() == space_id {
                if let Some(windows) = self.switched_space_windows.get(&space_id) {
                    return Ok(windows.clone());
                }
            }
            Ok(self
                .topology
                .active_space_windows
                .get(&space_id)
                .cloned()
                .unwrap_or_default())
        }

        fn inactive_space_window_ids(
            &self,
        ) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
            Ok(if *self.current_space_id.borrow() == 2 {
                HashMap::from([(
                    1,
                    self.topology
                        .active_space_windows
                        .get(&1)
                        .into_iter()
                        .flat_map(|windows| windows.iter().map(|window| window.id))
                        .collect(),
                )])
            } else {
                self.topology.inactive_space_window_ids.clone()
            })
        }

        fn focused_window_id(&self) -> Result<Option<u64>, MacosNativeProbeError> {
            if *self.current_space_id.borrow() == 2 {
                panic!("post-switch target selection should not query focused_window_id");
            }
            Ok(self.topology.focused_window_id)
        }

        fn onscreen_window_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            Ok(self
                .active_space_windows(*self.current_space_id.borrow())?
                .into_iter()
                .map(|window| window.id)
                .collect())
        }

        fn switch_space(&self, space_id: u64) -> Result<(), MacosNativeOperationError> {
            self.calls
                .borrow_mut()
                .push(format!("switch_space:{space_id}"));
            *self.current_space_id.borrow_mut() = space_id;
            Ok(())
        }

        fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
            self.calls
                .borrow_mut()
                .push(format!("focus_window:{window_id}"));
            Ok(())
        }

        fn move_window_to_space(
            &self,
            _window_id: u64,
            _space_id: u64,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }

        fn swap_window_frames(
            &self,
            _source_window_id: u64,
            _source_frame: NativeBounds,
            _target_window_id: u64,
            _target_frame: NativeBounds,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }

        fn topology_snapshot_without_focus(
            &self,
        ) -> Result<RawTopologySnapshot, MacosNativeProbeError> {
            let active_space_ids = self.active_space_ids()?;
            let active_space_windows = active_space_ids
                .iter()
                .copied()
                .map(|space_id| {
                    self.active_space_windows(space_id)
                        .map(|windows| (space_id, windows))
                })
                .collect::<Result<HashMap<_, _>, _>>()?;

            Ok(RawTopologySnapshot {
                spaces: self.managed_spaces()?,
                active_space_ids,
                active_space_windows,
                inactive_space_window_ids: self.inactive_space_window_ids()?,
                focused_window_id: None,
            })
        }
    }

    #[derive(Debug, Clone)]
    struct AdjacentHotkeyOnlyApi {
        topology: RawTopologySnapshot,
        switched_space_windows: HashMap<u64, Vec<RawWindow>>,
        current_space_id: Rc<RefCell<u64>>,
        calls: Rc<RefCell<Vec<String>>>,
    }

    impl MacosNativeApi for AdjacentHotkeyOnlyApi {
        fn has_symbol(&self, _symbol: &'static str) -> bool {
            true
        }

        fn ax_is_trusted(&self) -> bool {
            true
        }

        fn minimal_topology_ready(&self) -> bool {
            true
        }

        fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
            Ok(native_desktop_snapshot_from_topology(
                &self.topology_snapshot()?,
            ))
        }

        fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
            Ok(self.topology.spaces.clone())
        }

        fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            Ok(HashSet::from([*self.current_space_id.borrow()]))
        }

        fn active_space_windows(
            &self,
            space_id: u64,
        ) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
            if *self.current_space_id.borrow() == space_id {
                if let Some(windows) = self.switched_space_windows.get(&space_id) {
                    return Ok(windows.clone());
                }
            }
            Ok(self
                .topology
                .active_space_windows
                .get(&space_id)
                .cloned()
                .unwrap_or_default())
        }

        fn inactive_space_window_ids(
            &self,
        ) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
            Ok(self.topology.inactive_space_window_ids.clone())
        }

        fn focused_window_id(&self) -> Result<Option<u64>, MacosNativeProbeError> {
            Ok(self
                .active_space_windows(*self.current_space_id.borrow())?
                .first()
                .map(|window| window.id))
        }

        fn onscreen_window_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            Ok(self
                .active_space_windows(*self.current_space_id.borrow())?
                .into_iter()
                .map(|window| window.id)
                .collect())
        }

        fn switch_space(&self, space_id: u64) -> Result<(), MacosNativeOperationError> {
            self.calls
                .borrow_mut()
                .push(format!("switch_space:{space_id}"));
            Err(MacosNativeOperationError::CallFailed(
                "direct_switch_for_adjacent_space",
            ))
        }

        fn switch_adjacent_space(
            &self,
            _direction: NativeDirection,
            space_id: u64,
        ) -> Result<(), MacosNativeOperationError> {
            *self.current_space_id.borrow_mut() = space_id;
            Ok(())
        }

        fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
            if !self
                .switched_space_windows
                .contains_key(&*self.current_space_id.borrow())
            {
                return Err(MacosNativeOperationError::MissingWindow(window_id));
            }
            self.calls
                .borrow_mut()
                .push(format!("focus_window:{window_id}"));
            Ok(())
        }

        fn move_window_to_space(
            &self,
            _window_id: u64,
            _space_id: u64,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }

        fn swap_window_frames(
            &self,
            _source_window_id: u64,
            _source_frame: NativeBounds,
            _target_window_id: u64,
            _target_frame: NativeBounds,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }
    }

    #[derive(Debug, Clone)]
    struct EmptySpaceSkippingAdjacentHotkeyApi {
        topology: RawTopologySnapshot,
        switched_space_windows: HashMap<u64, Vec<RawWindow>>,
        current_space_id: Rc<RefCell<u64>>,
        adjacent_hotkey_skip_target_space_id: u64,
        calls: Rc<RefCell<Vec<String>>>,
    }

    impl MacosNativeApi for EmptySpaceSkippingAdjacentHotkeyApi {
        fn has_symbol(&self, _symbol: &'static str) -> bool {
            true
        }

        fn ax_is_trusted(&self) -> bool {
            true
        }

        fn minimal_topology_ready(&self) -> bool {
            true
        }

        fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
            Ok(native_desktop_snapshot_from_topology(
                &self.topology_snapshot()?,
            ))
        }

        fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
            Ok(self.topology.spaces.clone())
        }

        fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            Ok(HashSet::from([*self.current_space_id.borrow()]))
        }

        fn active_space_windows(
            &self,
            space_id: u64,
        ) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
            if *self.current_space_id.borrow() == space_id {
                if let Some(windows) = self.switched_space_windows.get(&space_id) {
                    return Ok(windows.clone());
                }
            }
            Ok(self
                .topology
                .active_space_windows
                .get(&space_id)
                .cloned()
                .unwrap_or_default())
        }

        fn inactive_space_window_ids(
            &self,
        ) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
            Ok(self.topology.inactive_space_window_ids.clone())
        }

        fn focused_window_id(&self) -> Result<Option<u64>, MacosNativeProbeError> {
            Ok(self
                .active_space_windows(*self.current_space_id.borrow())?
                .first()
                .map(|window| window.id))
        }

        fn onscreen_window_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            Ok(self
                .active_space_windows(*self.current_space_id.borrow())?
                .into_iter()
                .map(|window| window.id)
                .collect())
        }

        fn switch_space(&self, space_id: u64) -> Result<(), MacosNativeOperationError> {
            self.calls
                .borrow_mut()
                .push(format!("switch_space:{space_id}"));
            *self.current_space_id.borrow_mut() = space_id;
            Ok(())
        }

        fn switch_adjacent_space(
            &self,
            direction: NativeDirection,
            space_id: u64,
        ) -> Result<(), MacosNativeOperationError> {
            self.calls
                .borrow_mut()
                .push(format!("switch_adjacent_space:{direction}:{space_id}"));
            *self.current_space_id.borrow_mut() = self.adjacent_hotkey_skip_target_space_id;
            Ok(())
        }

        fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
            self.calls
                .borrow_mut()
                .push(format!("focus_window:{window_id}"));
            Ok(())
        }

        fn move_window_to_space(
            &self,
            _window_id: u64,
            _space_id: u64,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }

        fn swap_window_frames(
            &self,
            _source_window_id: u64,
            _source_frame: NativeBounds,
            _target_window_id: u64,
            _target_frame: NativeBounds,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }
    }

    struct FocusedWindowFastPathApi;

    impl MacosNativeApi for FocusedWindowFastPathApi {
        fn has_symbol(&self, _symbol: &'static str) -> bool {
            true
        }

        fn ax_is_trusted(&self) -> bool {
            true
        }

        fn minimal_topology_ready(&self) -> bool {
            true
        }

        fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
            let active_space_ids = self.active_space_ids()?;
            let focused_window_id = self.focused_window_id()?;
            let mut windows = Vec::new();

            for &space_id in &active_space_ids {
                windows.extend(
                    order_active_space_windows(&self.active_space_windows(space_id)?)
                        .into_iter()
                        .enumerate()
                        .map(|(order_index, window)| NativeWindowSnapshot {
                            id: window.id,
                            pid: window.pid,
                            app_id: window.app_id,
                            title: window.title,
                            bounds: window.frame,
                            level: window.level,
                            space_id,
                            order_index: Some(order_index),
                        }),
                );
            }

            Ok(NativeDesktopSnapshot {
                spaces: vec![NativeSpaceSnapshot {
                    id: 1,
                    display_index: 0,
                    active: true,
                    kind: SpaceKind::Desktop,
                }],
                active_space_ids,
                windows,
                focused_window_id,
            })
        }

        fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
            panic!("focused_window fast path must not query managed_spaces")
        }

        fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            Ok(HashSet::from([1]))
        }

        fn active_space_windows(
            &self,
            _space_id: u64,
        ) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
            Ok(vec![
                raw_window(10)
                    .with_pid(1010)
                    .with_app_id("first.app")
                    .with_title("first")
                    .with_visible_index(1),
                raw_window(20)
                    .with_pid(2020)
                    .with_app_id("focused.app")
                    .with_title("focused")
                    .with_visible_index(0),
            ])
        }

        fn inactive_space_window_ids(
            &self,
        ) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
            panic!("focused_window fast path must not query inactive_space_window_ids")
        }

        fn focused_window_id(&self) -> Result<Option<u64>, MacosNativeProbeError> {
            Ok(Some(20))
        }

        fn switch_space(&self, _space_id: u64) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }

        fn focus_window(&self, _window_id: u64) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }

        fn move_window_to_space(
            &self,
            _window_id: u64,
            _space_id: u64,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }

        fn swap_window_frames(
            &self,
            _source_window_id: u64,
            _source_frame: NativeBounds,
            _target_window_id: u64,
            _target_frame: NativeBounds,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }
    }

    struct SnapshotOnlyApi {
        snapshot: NativeDesktopSnapshot,
    }

    impl SnapshotOnlyApi {
        fn new(snapshot: NativeDesktopSnapshot) -> Self {
            Self { snapshot }
        }
    }

    struct SnapshotApiFactory {
        snapshot: NativeDesktopSnapshot,
    }

    impl SnapshotApiFactory {
        fn new(snapshot: NativeDesktopSnapshot) -> Self {
            Self { snapshot }
        }
    }

    impl MacosNativeApiFactory for SnapshotApiFactory {
        type Api = SnapshotOnlyApi;

        fn create(&self) -> Self::Api {
            SnapshotOnlyApi::new(self.snapshot.clone())
        }
    }

    impl MacosNativeApi for SnapshotOnlyApi {
        fn has_symbol(&self, _symbol: &'static str) -> bool {
            true
        }

        fn ax_is_trusted(&self) -> bool {
            true
        }

        fn minimal_topology_ready(&self) -> bool {
            true
        }

        fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
            Ok(self.snapshot.clone())
        }

        fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
            panic!("snapshot-only api must not query managed_spaces")
        }

        fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
            panic!("snapshot-only api must not query active_space_ids")
        }

        fn active_space_windows(
            &self,
            _space_id: u64,
        ) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
            panic!("snapshot-only api must not query active_space_windows")
        }

        fn inactive_space_window_ids(
            &self,
        ) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
            panic!("snapshot-only api must not query inactive_space_window_ids")
        }

        fn focused_window_snapshot(&self) -> Result<WindowSnapshot, MacosNativeProbeError> {
            panic!("snapshot-only api must not query focused_window_snapshot")
        }

        fn switch_space(&self, _space_id: u64) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }

        fn focus_window(&self, _window_id: u64) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }

        fn move_window_to_space(
            &self,
            _window_id: u64,
            _space_id: u64,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }

        fn swap_window_frames(
            &self,
            _source_window_id: u64,
            _source_frame: NativeBounds,
            _target_window_id: u64,
            _target_frame: NativeBounds,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
        }
    }

    fn raw_window(id: u64) -> RawWindow {
        RawWindow {
            id,
            pid: None,
            app_id: None,
            title: None,
            level: 0,
            visible_index: None,
            frame: None,
        }
    }

    trait RawWindowTestExt {
        fn with_level(self, level: i32) -> Self;
        fn with_visible_index(self, visible_index: usize) -> Self;
        fn with_pid(self, pid: u32) -> Self;
        fn with_app_id(self, app_id: &str) -> Self;
        fn with_title(self, title: &str) -> Self;
        fn with_frame(self, frame: Rect) -> Self;
    }

    impl RawWindowTestExt for RawWindow {
        fn with_level(mut self, level: i32) -> Self {
            self.level = level;
            self
        }

        fn with_visible_index(mut self, visible_index: usize) -> Self {
            self.visible_index = Some(visible_index);
            self
        }

        fn with_pid(mut self, pid: u32) -> Self {
            self.pid = Some(pid);
            self
        }

        fn with_app_id(mut self, app_id: &str) -> Self {
            self.app_id = Some(app_id.to_string());
            self
        }

        fn with_title(mut self, title: &str) -> Self {
            self.title = Some(title.to_string());
            self
        }

        fn with_frame(mut self, frame: Rect) -> Self {
            self.frame = Some(native_bounds_from_outer(frame));
            self
        }
    }

    fn raw_desktop_space_on_display(managed_space_id: u64, display_index: usize) -> RawSpaceRecord {
        RawSpaceRecord {
            managed_space_id,
            display_index,
            space_type: DESKTOP_SPACE_TYPE,
            tile_spaces: Vec::new(),
            has_tile_layout_manager: false,
            stage_manager_managed: false,
        }
    }

    fn raw_desktop_space(managed_space_id: u64) -> RawSpaceRecord {
        raw_desktop_space_on_display(managed_space_id, 0)
    }

    fn raw_fullscreen_space_on_display(
        managed_space_id: u64,
        display_index: usize,
    ) -> RawSpaceRecord {
        RawSpaceRecord {
            managed_space_id,
            display_index,
            space_type: FULLSCREEN_SPACE_TYPE,
            tile_spaces: Vec::new(),
            has_tile_layout_manager: false,
            stage_manager_managed: false,
        }
    }

    fn raw_fullscreen_space(managed_space_id: u64) -> RawSpaceRecord {
        raw_fullscreen_space_on_display(managed_space_id, 0)
    }

    fn raw_split_space_on_display(
        managed_space_id: u64,
        tile_spaces: &[u64],
        display_index: usize,
    ) -> RawSpaceRecord {
        RawSpaceRecord {
            managed_space_id,
            display_index,
            space_type: DESKTOP_SPACE_TYPE,
            tile_spaces: tile_spaces.to_vec(),
            has_tile_layout_manager: true,
            stage_manager_managed: false,
        }
    }

    fn raw_split_space(managed_space_id: u64, tile_spaces: &[u64]) -> RawSpaceRecord {
        raw_split_space_on_display(managed_space_id, tile_spaces, 0)
    }

    fn raw_stage_manager_space_on_display(
        managed_space_id: u64,
        display_index: usize,
    ) -> RawSpaceRecord {
        RawSpaceRecord {
            managed_space_id,
            display_index,
            space_type: DESKTOP_SPACE_TYPE,
            tile_spaces: Vec::new(),
            has_tile_layout_manager: false,
            stage_manager_managed: true,
        }
    }

    fn raw_stage_manager_space(managed_space_id: u64) -> RawSpaceRecord {
        raw_stage_manager_space_on_display(managed_space_id, 0)
    }

    fn fake_context_with_spaces() -> MacosNativeContext<FakeNativeApi> {
        MacosNativeContext::connect_with_api(FakeNativeApi::default()).unwrap()
    }

    fn fake_context_with_active_window(window_id: u64) -> MacosNativeContext<FakeNativeApi> {
        let topology = FakeNativeApi::topology_fixture(window_id);
        let api = FakeNativeApi::default().with_topology(topology);
        MacosNativeContext::connect_with_api(api).unwrap()
    }

    fn fake_context_with_active_window_calls(
        window_id: u64,
    ) -> (MacosNativeContext<FakeNativeApi>, Rc<RefCell<Vec<String>>>) {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let topology = FakeNativeApi::topology_fixture(window_id);
        let api = FakeNativeApi::default()
            .with_calls(calls.clone())
            .with_topology(topology);

        (MacosNativeContext::connect_with_api(api).unwrap(), calls)
    }

    fn focus_target_topology_fixture(window_id: u64, target_space_id: u64) -> RawTopologySnapshot {
        RawTopologySnapshot {
            spaces: vec![raw_desktop_space(1), raw_desktop_space(target_space_id)],
            active_space_ids: HashSet::from([1]),
            active_space_windows: HashMap::from([(
                1,
                vec![raw_window(11).with_visible_index(0).with_pid(1111)],
            )]),
            inactive_space_window_ids: HashMap::from([(target_space_id, vec![window_id])]),
            focused_window_id: Some(11),
        }
    }

    fn move_target_topology_fixture(window_id: u64, target_space_id: u64) -> RawTopologySnapshot {
        RawTopologySnapshot {
            spaces: vec![raw_desktop_space(1), raw_desktop_space(target_space_id)],
            active_space_ids: HashSet::from([1]),
            active_space_windows: HashMap::from([(
                1,
                vec![raw_window(window_id).with_visible_index(0).with_pid(5151)],
            )]),
            inactive_space_window_ids: HashMap::new(),
            focused_window_id: Some(window_id),
        }
    }

    fn fake_context_for_move(
        window_id: u64,
        target_space_id: u64,
    ) -> (MacosNativeContext<FakeNativeApi>, Rc<RefCell<Vec<String>>>) {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let api = FakeNativeApi::default()
            .with_calls(calls.clone())
            .with_topology(move_target_topology_fixture(window_id, target_space_id));

        (MacosNativeContext::connect_with_api(api).unwrap(), calls)
    }

    fn stage_manager_target_topology_fixture(
        window_id: u64,
        target_space_id: u64,
    ) -> RawTopologySnapshot {
        RawTopologySnapshot {
            spaces: vec![
                raw_desktop_space(1),
                raw_stage_manager_space(target_space_id),
            ],
            active_space_ids: HashSet::from([1]),
            active_space_windows: HashMap::from([(
                1,
                vec![raw_window(11).with_visible_index(0).with_pid(1111)],
            )]),
            inactive_space_window_ids: HashMap::from([(target_space_id, vec![window_id])]),
            focused_window_id: Some(11),
        }
    }

    fn fake_context_for_stage_manager_target(
        window_id: u64,
        target_space_id: u64,
    ) -> (MacosNativeContext<FakeNativeApi>, Rc<RefCell<Vec<String>>>) {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let api = FakeNativeApi::default()
            .with_calls(calls.clone())
            .with_topology(stage_manager_target_topology_fixture(
                window_id,
                target_space_id,
            ));

        (MacosNativeContext::connect_with_api(api).unwrap(), calls)
    }

    fn take_calls(calls: &Rc<RefCell<Vec<String>>>) -> Vec<String> {
        std::mem::take(&mut *calls.borrow_mut())
    }

    fn mission_control_hotkey(
        key_code: u16,
        modifiers: MissionControlModifiers,
    ) -> MissionControlHotkey {
        MissionControlHotkey {
            key_code,
            mission_control: modifiers,
        }
    }

    fn backend_options_with_hotkeys(
        west: MissionControlHotkey,
        east: MissionControlHotkey,
    ) -> NativeBackendOptions {
        NativeBackendOptions {
            west_space_hotkey: west,
            east_space_hotkey: east,
            diagnostics: None,
        }
    }

    struct InstalledConfigGuard {
        _env: std::sync::MutexGuard<'static, ()>,
        old: crate::config::Config,
    }

    impl Drop for InstalledConfigGuard {
        fn drop(&mut self) {
            crate::config::install(self.old.clone());
        }
    }

    fn install_config(raw: &str) -> InstalledConfigGuard {
        let env = crate::utils::env_guard();
        let old = crate::config::snapshot();
        let parsed: crate::config::Config =
            toml::from_str(raw).expect("macOS native test config should parse");
        crate::config::install(parsed);
        InstalledConfigGuard { _env: env, old }
    }

    fn install_macos_native_focus_config(strategy: &str) -> InstalledConfigGuard {
        install_config(&format!(
            r#"
[wm.macos_native]
enabled = true
floating_focus_strategy = "{strategy}"

[wm.macos_native.mission_control_keyboard_shortcuts.move_left_a_space]
keycode = "0x7B"
ctrl = true
fn = true
shift = false
option = false
command = false

[wm.macos_native.mission_control_keyboard_shortcuts.move_right_a_space]
keycode = "0x7C"
ctrl = true
fn = true
shift = false
option = false
command = false
"#,
        ))
    }

    fn cf_test_array(values: &[CFTypeRef]) -> CfOwned {
        CfOwned::from_servo(array_from_type_refs(values))
    }

    fn cf_test_dictionary(entries: &[(CFTypeRef, CFTypeRef)]) -> CfOwned {
        CfOwned::from_servo(dictionary_from_type_refs(entries))
    }

    fn implementation_source() -> &'static str {
        let source = include_str!("macos_native.rs");
        source
            .rsplit_once("#[cfg(test)]\nmod tests {")
            .map(|(implementation, _)| implementation)
            .expect("macos_native.rs source should include a test module")
    }

    fn block_end(implementation: &str, block_start: usize, expectation: &str) -> usize {
        let body_start = implementation[block_start..]
            .find('{')
            .map(|idx| block_start + idx)
            .expect(expectation);
        let mut depth = 0usize;

        for (relative_idx, ch) in implementation[body_start..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return body_start + relative_idx + 1;
                    }
                }
                _ => {}
            }
        }

        panic!("{expectation}");
    }

    #[test]
    fn source_scopes_cfg_test_attributes_to_test_modules() {
        let implementation = implementation_source();
        let lines = implementation.lines().collect::<Vec<_>>();

        for (idx, line) in lines.iter().enumerate() {
            if line.trim() != "#[cfg(test)]" {
                continue;
            }

            let next = lines[idx + 1..]
                .iter()
                .map(|line| line.trim())
                .find(|line| !line.is_empty());
            let gated_declaration = lines[idx + 1..]
                .iter()
                .map(|line| line.trim())
                .find(|line| !line.is_empty() && !line.starts_with("#["));
            assert!(
                gated_declaration.is_some_and(|line| {
                    line.ends_with("mod tests {")
                        || line.ends_with("mod macos_window_manager_test_support;")
                        || line.ends_with("use macos_window_manager_test_support::{")
                }),
                "cfg(test) outside the bottom test module should only gate mod tests blocks; found {:?} after line {}",
                next,
                idx + 1
            );
        }
    }

    #[test]
    fn source_keeps_declared_capability_validation_in_spec_connect() {
        let implementation = implementation_source();

        assert!(
            implementation
                .contains("validate_declared_capabilities::<MacosNativeAdapter<F::Api>>()?;"),
            "WindowManagerSpec::connect should validate declared capabilities before connecting"
        );
        assert!(
            implementation.contains("macos_native.connect.validate_capabilities"),
            "WindowManagerSpec::connect should keep the capability validation span"
        );
    }

    #[test]
    fn source_adapter_has_no_inline_macos_backend_module() {
        let implementation = implementation_source();
        assert!(!implementation.contains("mod macos_window_manager_api {"));
    }

    #[test]
    fn source_adapter_externalizes_macos_test_support_module() {
        let implementation = implementation_source();

        assert!(
            !implementation.contains("mod macos_window_manager_test_support {"),
            "adapter root should externalize macos test support into a sibling module file"
        );
        assert!(
            implementation.contains("mod macos_window_manager_test_support;"),
            "adapter root should keep test support behind a module declaration"
        );
    }

    #[test]
    fn source_compiles_against_shared_macos_window_manager_contract_in_tests() {
        let implementation = implementation_source();

        assert!(
            implementation.contains("use macos_window_manager::{"),
            "adapter should import its backend contract from the shared macos_window_manager crate"
        );
        assert!(
            !implementation.contains("#[cfg(test)]\nuse macos_window_manager_test_support::{"),
            "adapter root should not switch backend contract imports to macos_window_manager_test_support under tests"
        );
    }

    #[test]
    fn servo_cf_array_from_u64s_returns_numbers_in_order() {
        let array = array_from_u64s(&[11, 22])
            .expect("servo-backed helper should build a CFArray of numbers");

        let values = array
            .iter()
            .map(|number| number.to_i64().expect("fixture should stay numeric"))
            .collect::<Vec<_>>();

        assert_eq!(values, vec![11, 22]);
    }

    #[test]
    fn servo_cf_dictionary_accessors_read_string_and_i32_values() {
        let x_key = string("X");
        let title_key = string("Title");
        let x_value = number_from_u64(10).expect("servo-backed helper should build CFNumbers");
        let title_value = string("alpha");
        let dictionary = cf_test_dictionary(&[
            (x_key.as_CFTypeRef(), x_value.as_CFTypeRef()),
            (title_key.as_CFTypeRef(), title_value.as_CFTypeRef()),
        ]);

        assert_eq!(
            dictionary_i32(dictionary.as_type_ref() as CFDictionaryRef, &x_key),
            Some(10)
        );
        assert_eq!(
            dictionary_string(dictionary.as_type_ref() as CFDictionaryRef, &title_key),
            Some("alpha".to_string())
        );
    }

    #[test]
    fn classify_space_distinguishes_desktop_fullscreen_split_and_stage_manager() {
        assert_eq!(classify_space(&raw_desktop_space(1)), SpaceKind::Desktop);
        assert_eq!(
            classify_space(&raw_fullscreen_space(2)),
            SpaceKind::Fullscreen
        );
        assert_eq!(
            classify_space(&raw_split_space(3, &[11, 12])),
            SpaceKind::SplitView
        );
        assert_eq!(
            classify_space(&raw_stage_manager_space(4)),
            SpaceKind::StageManagerOpaque
        );
    }

    #[test]
    fn real_path_app_id_ignores_owner_name_display_label() {
        assert_eq!(stable_app_id_from_real_window(None, Some("Finder")), None);
    }

    #[test]
    fn enrich_real_window_app_ids_resolves_bundle_ids_after_parsing() {
        let windows = vec![raw_window(11).with_pid(42), raw_window(12)];

        let enriched = enrich_real_window_app_ids_with(windows, |pid| match pid {
            42 => Some("com.example.test".to_string()),
            _ => None,
        });

        assert_eq!(
            enriched,
            vec![
                raw_window(11).with_pid(42).with_app_id("com.example.test"),
                raw_window(12)
            ]
        );
    }

    #[test]
    fn enrich_real_window_app_ids_reuses_pid_lookups_within_single_pass() {
        let windows = vec![
            raw_window(11).with_pid(42),
            raw_window(12).with_pid(42),
            raw_window(13).with_pid(7),
            raw_window(14).with_pid(42),
        ];
        let mut resolved_pids = Vec::new();

        let enriched = enrich_real_window_app_ids_with(windows, |pid| {
            resolved_pids.push(pid);
            Some(format!("com.example.{pid}"))
        });

        assert_eq!(resolved_pids, vec![42, 7]);
        assert_eq!(
            enriched,
            vec![
                raw_window(11).with_pid(42).with_app_id("com.example.42"),
                raw_window(12).with_pid(42).with_app_id("com.example.42"),
                raw_window(13).with_pid(7).with_app_id("com.example.7"),
                raw_window(14).with_pid(42).with_app_id("com.example.42"),
            ]
        );
    }

    #[test]
    fn parse_lsappinfo_bundle_identifier_extracts_stable_app_id() {
        let output = "\"LSDisplayName\"=\"Finder\"\n\"CFBundleIdentifier\"=\"com.apple.finder\"\n";

        assert_eq!(
            parse_lsappinfo_bundle_identifier(output),
            Some("com.apple.finder".to_string())
        );
    }

    #[test]
    fn active_space_ordering_prefers_frontmost_visible_windows() {
        let windows = vec![
            raw_window(11).with_level(10).with_visible_index(1),
            raw_window(12).with_level(20).with_visible_index(0),
        ];

        let ordered = order_active_space_windows(&windows);
        assert_eq!(
            ordered.iter().map(|w| w.id).collect::<Vec<_>>(),
            vec![12, 11]
        );
    }

    #[test]
    fn active_space_ordering_uses_window_level_when_visible_order_is_missing() {
        let windows = vec![raw_window(21).with_level(10), raw_window(22).with_level(20)];

        let ordered = order_active_space_windows(&windows);
        assert_eq!(
            ordered.iter().map(|w| w.id).collect::<Vec<_>>(),
            vec![22, 21]
        );
    }

    #[test]
    fn active_space_ordering_prefers_visible_windows_over_fallback_ordering() {
        let windows = vec![
            raw_window(31).with_level(50),
            raw_window(32).with_visible_index(0),
        ];

        let ordered = order_active_space_windows(&windows);
        assert_eq!(
            ordered.iter().map(|w| w.id).collect::<Vec<_>>(),
            vec![32, 31]
        );
    }

    #[test]
    fn non_active_space_windows_remain_unordered() {
        let snapshots = snapshots_for_inactive_space(99, &[21, 22]);
        assert!(snapshots.iter().all(|window| window.order_index.is_none()));
    }

    #[test]
    fn best_window_id_from_windows_ignores_non_normal_layer_targets() {
        let windows = vec![
            raw_window(159)
                .with_pid(946)
                .with_level(0)
                .with_frame(Rect {
                    x: 1200,
                    y: 120,
                    w: 500,
                    h: 900,
                }),
            raw_window(52)
                .with_pid(950)
                .with_level(25)
                .with_frame(Rect {
                    x: 1739,
                    y: 0,
                    w: 63,
                    h: 39,
                }),
        ];

        assert_eq!(
            best_window_id_from_windows(NativeDirection::East, &windows),
            Some(159)
        );
    }

    #[test]
    fn connect_with_api_rejects_missing_required_symbol() {
        let api = FakeNativeApi::default().without_symbol("SLSCopyManagedDisplaySpaces");
        let err = MacosNativeContext::connect_with_api(api).unwrap_err();

        assert_eq!(
            err,
            MacosNativeConnectError::MissingRequiredSymbol("SLSCopyManagedDisplaySpaces")
        );
        assert!(err.to_string().contains("SLSCopyManagedDisplaySpaces"));
    }

    #[test]
    fn connect_with_api_rejects_missing_ax_trust_symbol() {
        let api = FakeNativeApi::default().without_symbol("AXIsProcessTrusted");
        let err = MacosNativeContext::connect_with_api(api).unwrap_err();

        assert_eq!(
            err,
            MacosNativeConnectError::MissingRequiredSymbol("AXIsProcessTrusted")
        );
        assert!(err.to_string().contains("AXIsProcessTrusted"));
    }

    #[test]
    fn connect_with_api_rejects_missing_accessibility_permission() {
        let api = FakeNativeApi::default().with_ax_trusted(false);
        let err = MacosNativeContext::connect_with_api(api).unwrap_err();

        assert_eq!(err, MacosNativeConnectError::MissingAccessibilityPermission);
        assert!(err.to_string().contains("Accessibility"));
    }

    #[test]
    fn connect_with_api_keeps_validation_in_outer_layer() {
        let api = FakeNativeApi::default().with_validate_environment_error(
            MacosNativeConnectError::MissingRequiredSymbol("SLSCopyManagedDisplaySpaces"),
        );

        let err = MacosNativeContext::connect_with_api(api).unwrap_err();

        assert_eq!(
            err,
            MacosNativeConnectError::MissingRequiredSymbol("SLSCopyManagedDisplaySpaces")
        );
    }

    #[test]
    fn connect_with_api_rejects_missing_minimal_topology_precondition() {
        let api = FakeNativeApi::default().with_minimal_topology_ready(false);
        let err = MacosNativeContext::connect_with_api(api).unwrap_err();

        assert_eq!(
            err,
            MacosNativeConnectError::MissingTopologyPrecondition("main SkyLight connection")
        );
        assert!(err.to_string().contains("main SkyLight connection"));
    }

    #[test]
    fn source_fake_validation_delegates_to_shared_helper() {
        let implementation = include_str!("macos_native.rs");
        let fake_impl_start = implementation
            .find("impl MacosNativeApi for FakeNativeApi {")
            .expect("implementation should define the fake api trait impl");
        let fake_validate_start = implementation[fake_impl_start..]
            .find("fn validate_environment(&self) -> Result<(), MacosNativeConnectError> {")
            .map(|idx| fake_impl_start + idx)
            .expect("fake api impl should override validate_environment");
        let fake_validate_end = block_end(
            implementation,
            fake_validate_start,
            "fake validate_environment should have a matching closing brace",
        );
        let fake_validate_source = &implementation[fake_validate_start..fake_validate_end];

        assert!(
            implementation
                .contains("fn validate_environment_with_api<A: MacosNativeApi + ?Sized>("),
            "backend should expose a shared validation helper"
        );
        assert!(
            fake_validate_source.contains("validate_environment_with_api(self)"),
            "fake validate_environment should delegate to the shared helper when not overriding"
        );
        assert!(
            !fake_validate_source.contains("REQUIRED_PRIVATE_SYMBOLS"),
            "fake validate_environment should not duplicate required symbol checks"
        );
    }

    #[test]
    fn spaces_snapshot_includes_active_flags_and_classified_kinds() {
        let ctx = fake_context_with_spaces();
        let spaces = ctx.spaces().unwrap();

        assert!(
            spaces
                .iter()
                .any(|space| space.kind == SpaceKind::Desktop && space.is_active)
        );
        assert!(
            spaces
                .iter()
                .any(|space| space.kind == SpaceKind::SplitView)
        );
    }

    #[test]
    fn focused_window_comes_from_active_space_snapshot() {
        let ctx = fake_context_with_active_window(42);
        let focused = ctx.focused_window().unwrap();
        assert_eq!(focused.id, 42);
        assert_eq!(focused.space_id, 1);
    }

    #[test]
    fn context_uses_api_topology_snapshot_override() {
        let ctx = MacosNativeContext::connect_with_api(SnapshotOverrideApi::default()).unwrap();

        let spaces = ctx.spaces().unwrap();
        let focused = ctx.focused_window().unwrap();

        assert_eq!(
            spaces
                .iter()
                .filter(|space| space.is_active)
                .map(|space| space.id)
                .collect::<Vec<_>>(),
            vec![1, 3]
        );
        assert_eq!(focused.id, 31);
        assert_eq!(focused.space_id, 3);
    }

    #[test]
    fn spaces_snapshot_marks_all_active_display_spaces_active() {
        let topology = FakeNativeApi::multi_display_topology_fixture();

        let spaces = space_snapshots_from_topology(&topology);

        assert_eq!(
            spaces
                .iter()
                .filter(|space| space.is_active)
                .map(|space| space.id)
                .collect::<Vec<_>>(),
            vec![1, 3]
        );
        assert_eq!(
            spaces
                .iter()
                .find(|space| space.id == 1)
                .and_then(|space| space.ordered_window_ids.as_deref()),
            Some(&[11][..])
        );
        assert_eq!(
            spaces
                .iter()
                .find(|space| space.id == 3)
                .and_then(|space| space.ordered_window_ids.as_deref()),
            Some(&[31][..])
        );
    }

    #[test]
    fn focused_window_prefers_frontmost_window_across_active_spaces() {
        let topology = FakeNativeApi::multi_display_topology_fixture();

        let focused = focused_window_from_topology(&topology).unwrap();

        assert_eq!(focused.id, 31);
        assert_eq!(focused.space_id, 3);
        assert_eq!(focused.order_index, Some(0));
    }

    #[test]
    fn focused_window_prefers_explicit_window_id_over_visible_order_heuristic() {
        let topology = RawTopologySnapshot {
            spaces: vec![raw_desktop_space(1)],
            active_space_ids: HashSet::from([1]),
            active_space_windows: HashMap::from([(
                1,
                vec![
                    raw_window(10).with_visible_index(0),
                    raw_window(20).with_visible_index(1),
                ],
            )]),
            inactive_space_window_ids: HashMap::new(),
            focused_window_id: Some(20),
        };

        let focused = focused_window_from_topology(&topology).unwrap();

        assert_eq!(focused.id, 20);
    }

    #[test]
    fn topology_snapshot_uses_api_focused_window_id() {
        let topology = FocusedIdTopologyApi.topology_snapshot().unwrap();

        assert_eq!(topology.focused_window_id, Some(11));
    }

    #[test]
    fn context_focused_window_uses_active_space_fast_path() {
        let ctx = MacosNativeContext::connect_with_api(FocusedWindowFastPathApi).unwrap();

        let focused = ctx.focused_window().unwrap();

        assert_eq!(focused.id, 20);
        assert_eq!(focused.space_id, 1);
        assert_eq!(focused.pid, Some(2020));
        assert_eq!(focused.app_id.as_deref(), Some("focused.app"));
        assert_eq!(focused.title.as_deref(), Some("focused"));
        assert_eq!(focused.order_index, Some(0));
    }

    #[test]
    fn focused_window_and_windows_are_derived_from_native_snapshot() {
        let mut adapter =
            MacosNativeAdapter::connect_with_api(SnapshotOnlyApi::new(NativeDesktopSnapshot {
                spaces: vec![NativeSpaceSnapshot {
                    id: 1,
                    display_index: 0,
                    active: true,
                    kind: SpaceKind::Desktop,
                }],
                active_space_ids: HashSet::from([1]),
                windows: vec![
                    NativeWindowSnapshot {
                        id: 101,
                        pid: Some(4001),
                        app_id: Some("focused.app".to_string()),
                        title: Some("Focused".to_string()),
                        bounds: None,
                        level: 0,
                        space_id: 1,
                        order_index: Some(0),
                    },
                    NativeWindowSnapshot {
                        id: 102,
                        pid: Some(4002),
                        app_id: Some("other.app".to_string()),
                        title: Some("Other".to_string()),
                        bounds: None,
                        level: 0,
                        space_id: 1,
                        order_index: Some(1),
                    },
                ],
                focused_window_id: Some(101),
            }))
            .unwrap();

        let focused = WindowManagerSession::focused_window(&mut adapter).unwrap();
        let windows = WindowManagerSession::windows(&mut adapter).unwrap();

        assert_eq!(focused.id, 101);
        assert_eq!(windows.len(), 2);
    }

    #[test]
    fn native_snapshot_can_drive_outer_directional_selection() {
        let snapshot = NativeDesktopSnapshot {
            spaces: vec![NativeSpaceSnapshot {
                id: 1,
                display_index: 0,
                active: true,
                kind: SpaceKind::Desktop,
            }],
            active_space_ids: HashSet::from([1]),
            windows: vec![
                NativeWindowSnapshot {
                    id: 100,
                    pid: Some(4001),
                    app_id: Some("west.app".to_string()),
                    title: Some("West".to_string()),
                    bounds: Some(NativeBounds {
                        x: 0,
                        y: 0,
                        width: 100,
                        height: 100,
                    }),
                    level: 0,
                    space_id: 1,
                    order_index: Some(0),
                },
                NativeWindowSnapshot {
                    id: 101,
                    pid: Some(4002),
                    app_id: Some("east.app".to_string()),
                    title: Some("East".to_string()),
                    bounds: Some(NativeBounds {
                        x: 200,
                        y: 0,
                        width: 100,
                        height: 100,
                    }),
                    level: 0,
                    space_id: 1,
                    order_index: Some(1),
                },
            ],
            focused_window_id: Some(101),
        };
        let topology = outer_topology_from_native_snapshot(&snapshot).unwrap();

        let target =
            select_closest_in_direction_with_strategy(&topology.rects, 101, Direction::West, None);

        assert_eq!(target, Some(100));
    }

    #[test]
    fn outer_directional_selection_ignores_non_normal_layer_targets_from_raw_snapshot() {
        let topology = RawTopologySnapshot {
            spaces: vec![raw_split_space(1, &[11, 12])],
            active_space_ids: HashSet::from([1]),
            active_space_windows: HashMap::from([(
                1,
                vec![
                    raw_window(100)
                        .with_level(0)
                        .with_pid(4001)
                        .with_frame(Rect {
                            x: 0,
                            y: 120,
                            w: 500,
                            h: 900,
                        }),
                    raw_window(159)
                        .with_level(0)
                        .with_pid(946)
                        .with_frame(Rect {
                            x: 1200,
                            y: 120,
                            w: 500,
                            h: 900,
                        }),
                    raw_window(52)
                        .with_level(25)
                        .with_pid(950)
                        .with_frame(Rect {
                            x: 1739,
                            y: 0,
                            w: 63,
                            h: 39,
                        }),
                ],
            )]),
            inactive_space_window_ids: HashMap::new(),
            focused_window_id: Some(100),
        };

        let snapshot = native_desktop_snapshot_from_topology(&topology);
        let outer_topology = outer_topology_from_native_snapshot(&snapshot).unwrap();

        let target = select_focus_target_from_outer_topology(
            &outer_topology,
            Direction::East,
            crate::engine::topology::FloatingFocusStrategy::RadialCenter,
        )
        .unwrap();

        assert_eq!(target, FocusTarget::SameSpace { window_id: 159 });
    }

    #[test]
    fn focused_window_and_windows_fall_back_when_native_snapshot_has_no_focused_window_id() {
        let mut adapter =
            MacosNativeAdapter::connect_with_api(SnapshotOnlyApi::new(NativeDesktopSnapshot {
                spaces: vec![NativeSpaceSnapshot {
                    id: 1,
                    display_index: 0,
                    active: true,
                    kind: SpaceKind::Desktop,
                }],
                active_space_ids: HashSet::from([1]),
                windows: vec![
                    NativeWindowSnapshot {
                        id: 101,
                        pid: Some(4001),
                        app_id: Some("focused.app".to_string()),
                        title: Some("Focused".to_string()),
                        bounds: None,
                        level: 0,
                        space_id: 1,
                        order_index: Some(0),
                    },
                    NativeWindowSnapshot {
                        id: 102,
                        pid: Some(4002),
                        app_id: Some("other.app".to_string()),
                        title: Some("Other".to_string()),
                        bounds: None,
                        level: 0,
                        space_id: 1,
                        order_index: Some(1),
                    },
                ],
                focused_window_id: None,
            }))
            .unwrap();

        let focused = WindowManagerSession::focused_window(&mut adapter).unwrap();
        let windows = WindowManagerSession::windows(&mut adapter).unwrap();

        assert_eq!(focused.id, 101);
        assert_eq!(
            windows
                .iter()
                .find(|window| window.id == 101)
                .map(|window| window.is_focused),
            Some(true)
        );
        assert_eq!(
            windows
                .iter()
                .find(|window| window.id == 102)
                .map(|window| window.is_focused),
            Some(false)
        );
    }

    #[test]
    fn focused_window_and_windows_use_explicit_native_snapshot_focus_without_active_space_hints() {
        let mut adapter =
            MacosNativeAdapter::connect_with_api(SnapshotOnlyApi::new(NativeDesktopSnapshot {
                spaces: Vec::new(),
                active_space_ids: HashSet::new(),
                windows: vec![
                    NativeWindowSnapshot {
                        id: 101,
                        pid: Some(4001),
                        app_id: Some("focused.app".to_string()),
                        title: Some("Focused".to_string()),
                        bounds: None,
                        level: 0,
                        space_id: 99,
                        order_index: Some(1),
                    },
                    NativeWindowSnapshot {
                        id: 102,
                        pid: Some(4002),
                        app_id: Some("other.app".to_string()),
                        title: Some("Other".to_string()),
                        bounds: None,
                        level: 0,
                        space_id: 100,
                        order_index: Some(0),
                    },
                ],
                focused_window_id: Some(101),
            }))
            .unwrap();

        let focused = WindowManagerSession::focused_window(&mut adapter).unwrap();
        let windows = WindowManagerSession::windows(&mut adapter).unwrap();

        assert_eq!(focused.id, 101);
        assert_eq!(
            windows
                .iter()
                .find(|window| window.id == 101)
                .map(|window| window.is_focused),
            Some(true)
        );
        assert_eq!(
            windows
                .iter()
                .find(|window| window.id == 102)
                .map(|window| window.is_focused),
            Some(false)
        );
    }

    #[test]
    fn focused_app_record_is_derived_from_native_snapshot() {
        let spec = MacosNativeSpec {
            api_factory: SnapshotApiFactory::new(NativeDesktopSnapshot {
                spaces: vec![NativeSpaceSnapshot {
                    id: 1,
                    display_index: 0,
                    active: true,
                    kind: SpaceKind::Desktop,
                }],
                active_space_ids: HashSet::from([1]),
                windows: vec![NativeWindowSnapshot {
                    id: 101,
                    pid: Some(4001),
                    app_id: Some("focused.app".to_string()),
                    title: Some("Focused".to_string()),
                    bounds: None,
                    level: 0,
                    space_id: 1,
                    order_index: Some(0),
                }],
                focused_window_id: Some(101),
            }),
        };
        let focused = WindowManagerSpec::focused_app_record(&spec).unwrap();

        assert_eq!(
            focused,
            Some(FocusedAppRecord {
                app_id: "focused.app".to_string(),
                title: "Focused".to_string(),
                pid: ProcessId::new(4001).unwrap(),
            })
        );
    }

    #[test]
    fn focused_app_record_falls_back_when_native_snapshot_focused_window_id_is_stale() {
        let spec = MacosNativeSpec {
            api_factory: SnapshotApiFactory::new(NativeDesktopSnapshot {
                spaces: vec![NativeSpaceSnapshot {
                    id: 1,
                    display_index: 0,
                    active: true,
                    kind: SpaceKind::Desktop,
                }],
                active_space_ids: HashSet::from([1]),
                windows: vec![
                    NativeWindowSnapshot {
                        id: 101,
                        pid: Some(4001),
                        app_id: Some("focused.app".to_string()),
                        title: Some("Focused".to_string()),
                        bounds: None,
                        level: 0,
                        space_id: 1,
                        order_index: Some(0),
                    },
                    NativeWindowSnapshot {
                        id: 102,
                        pid: Some(4002),
                        app_id: Some("other.app".to_string()),
                        title: Some("Other".to_string()),
                        bounds: None,
                        level: 0,
                        space_id: 1,
                        order_index: Some(1),
                    },
                ],
                focused_window_id: Some(999),
            }),
        };

        let focused = WindowManagerSpec::focused_app_record(&spec).unwrap();

        assert_eq!(
            focused,
            Some(FocusedAppRecord {
                app_id: "focused.app".to_string(),
                title: "Focused".to_string(),
                pid: ProcessId::new(4001).unwrap(),
            })
        );
    }

    #[test]
    fn focused_window_fast_path_desktop_snapshot_stays_topology_free() {
        let snapshot = FocusedWindowFastPathApi.desktop_snapshot().unwrap();

        assert_eq!(snapshot.active_space_ids, HashSet::from([1]));
        assert_eq!(snapshot.focused_window_id, Some(20));
        assert_eq!(
            snapshot.spaces,
            vec![NativeSpaceSnapshot {
                id: 1,
                display_index: 0,
                active: true,
                kind: SpaceKind::Desktop,
            }]
        );
        assert_eq!(
            snapshot.windows,
            vec![
                NativeWindowSnapshot {
                    id: 20,
                    pid: Some(2020),
                    app_id: Some("focused.app".to_string()),
                    title: Some("focused".to_string()),
                    bounds: None,
                    level: 0,
                    space_id: 1,
                    order_index: Some(0),
                },
                NativeWindowSnapshot {
                    id: 10,
                    pid: Some(1010),
                    app_id: Some("first.app".to_string()),
                    title: Some("first".to_string()),
                    bounds: None,
                    level: 0,
                    space_id: 1,
                    order_index: Some(1),
                },
            ]
        );
    }

    #[test]
    fn adapter_windows_reflect_snapshot_order_and_focus_state() {
        let topology = RawTopologySnapshot {
            spaces: vec![raw_desktop_space(1), raw_split_space(2, &[21, 22])],
            active_space_ids: HashSet::from([1]),
            active_space_windows: HashMap::from([(
                1,
                vec![
                    raw_window(11)
                        .with_visible_index(1)
                        .with_pid(1111)
                        .with_app_id("com.example.back")
                        .with_title("Back"),
                    raw_window(12)
                        .with_visible_index(0)
                        .with_pid(2222)
                        .with_app_id("com.example.front")
                        .with_title("Front"),
                    raw_window(13)
                        .with_level(5)
                        .with_pid(3333)
                        .with_app_id("com.example.overlay")
                        .with_title("Overlay"),
                ],
            )]),
            inactive_space_window_ids: HashMap::from([(2, vec![21, 22])]),
            focused_window_id: Some(12),
        };
        let api = SendRecordingApi {
            topology,
            calls: Arc::new(Mutex::new(Vec::new())),
        };
        let mut adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        let windows = WindowManagerSession::windows(&mut adapter).unwrap();

        assert_eq!(
            windows
                .iter()
                .map(|window| (window.id, window.is_focused, window.original_tile_index))
                .collect::<Vec<_>>(),
            vec![
                (12, true, 0),
                (11, false, 1),
                (13, false, 2),
                (21, false, 0),
                (22, false, 0),
            ]
        );
        assert_eq!(windows[0].pid, ProcessId::new(2222));
        assert_eq!(windows[0].app_id.as_deref(), Some("com.example.front"));
        assert_eq!(windows[0].title.as_deref(), Some("Front"));
        assert_eq!(windows[3].pid, None);
        assert_eq!(windows[3].app_id, None);
    }

    #[test]
    fn focused_window_id_via_ax_queries_focused_app_then_window() {
        let focused_window_id = focused_window_id_via_ax(
            || Ok(Some("app")),
            |application| {
                assert_eq!(*application, "app");
                Ok(Some("window"))
            },
            |element| {
                assert_eq!(*element, "window");
                Ok(77)
            },
        )
        .unwrap();

        assert_eq!(focused_window_id, Some(77));
    }

    #[test]
    fn focus_window_via_process_and_raise_fronts_makes_key_then_raises_target_window() {
        let calls = Rc::new(RefCell::new(Vec::new()));

        focus_window_via_process_and_raise(
            77,
            |_| Ok(5151),
            |pid| {
                assert_eq!(pid, 5151);
                Ok(ProcessSerialNumber {
                    high_long_of_psn: 1,
                    low_long_of_psn: 2,
                })
            },
            {
                let calls = calls.clone();
                move |psn, window_id| {
                    calls.borrow_mut().push(format!(
                        "front:{}:{}:{}",
                        psn.high_long_of_psn, psn.low_long_of_psn, window_id
                    ));
                    Ok(())
                }
            },
            {
                let calls = calls.clone();
                move |psn, window_id| {
                    calls.borrow_mut().push(format!(
                        "make_key:{}:{}:{}",
                        psn.high_long_of_psn, psn.low_long_of_psn, window_id
                    ));
                    Ok(())
                }
            },
            {
                let calls = calls.clone();
                move |window_id, pid| {
                    calls.borrow_mut().push(format!("raise:{window_id}:{pid}"));
                    Ok(())
                }
            },
        )
        .unwrap();

        assert_eq!(
            take_calls(&calls),
            vec!["front:1:2:77", "make_key:1:2:77", "raise:77:5151"]
        );
    }

    #[test]
    fn switch_adjacent_space_via_hotkey_posts_configured_shortcut_for_east() {
        let options = backend_options_with_hotkeys(
            mission_control_hotkey(
                0x7B,
                MissionControlModifiers {
                    control: true,
                    option: false,
                    command: false,
                    shift: false,
                    function: true,
                },
            ),
            mission_control_hotkey(
                0x1A,
                MissionControlModifiers {
                    control: false,
                    option: true,
                    command: true,
                    shift: true,
                    function: false,
                },
            ),
        );

        let calls = Rc::new(RefCell::new(Vec::new()));

        switch_adjacent_space_via_hotkey(
            &options,
            NativeDirection::East,
            |key_code, key_down, flags| {
                calls.borrow_mut().push(format!(
                    "key:{key_code}:{}:{flags}",
                    if key_down { "down" } else { "up" }
                ));
                Ok(())
            },
        )
        .unwrap();

        let flags = K_CG_EVENT_FLAG_MASK_SHIFT
            | K_CG_EVENT_FLAG_MASK_ALTERNATE
            | K_CG_EVENT_FLAG_MASK_COMMAND;
        assert_eq!(
            take_calls(&calls),
            vec![
                format!("key:{}:down:{flags}", 0x1A),
                format!("key:{}:up:{flags}", 0x1A),
            ]
        );
    }

    #[test]
    fn switch_adjacent_space_via_hotkey_rejects_vertical_directions() {
        let options = backend_options_with_hotkeys(
            mission_control_hotkey(0x7B, MissionControlModifiers::default()),
            mission_control_hotkey(0x7C, MissionControlModifiers::default()),
        );
        let err =
            switch_adjacent_space_via_hotkey(&options, NativeDirection::North, |_, _, _| Ok(()))
                .unwrap_err();

        assert_eq!(
            err,
            MacosNativeOperationError::CallFailed("adjacent_space_hotkey_direction")
        );
    }

    #[test]
    fn focus_window_via_make_key_and_raise_skips_front_process() {
        let calls = Rc::new(RefCell::new(Vec::new()));

        focus_window_via_make_key_and_raise(
            77,
            |_| Ok(5151),
            |pid| {
                assert_eq!(pid, 5151);
                Ok(ProcessSerialNumber {
                    high_long_of_psn: 1,
                    low_long_of_psn: 2,
                })
            },
            {
                let calls = calls.clone();
                move |psn, window_id| {
                    calls.borrow_mut().push(format!(
                        "make_key:{}:{}:{}",
                        psn.high_long_of_psn, psn.low_long_of_psn, window_id
                    ));
                    Ok(())
                }
            },
            {
                let calls = calls.clone();
                move |window_id, pid| {
                    calls.borrow_mut().push(format!("raise:{window_id}:{pid}"));
                    Ok(())
                }
            },
        )
        .unwrap();

        assert_eq!(take_calls(&calls), vec!["make_key:1:2:77", "raise:77:5151"]);
    }

    #[test]
    fn focus_window_via_make_key_and_raise_retries_missing_ax_window_during_raise() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let attempts = Rc::new(RefCell::new(0usize));

        focus_window_via_make_key_and_raise(
            77,
            |_| Ok(5151),
            |_| {
                Ok(ProcessSerialNumber {
                    high_long_of_psn: 1,
                    low_long_of_psn: 2,
                })
            },
            {
                let calls = calls.clone();
                move |psn, window_id| {
                    calls.borrow_mut().push(format!(
                        "make_key:{}:{}:{}",
                        psn.high_long_of_psn, psn.low_long_of_psn, window_id
                    ));
                    Ok(())
                }
            },
            {
                let calls = calls.clone();
                let attempts = attempts.clone();
                move |window_id, pid| {
                    let mut attempts = attempts.borrow_mut();
                    *attempts += 1;
                    calls
                        .borrow_mut()
                        .push(format!("raise:{window_id}:{pid}:{}", *attempts));
                    if *attempts == 1 {
                        Err(MacosNativeOperationError::MissingWindow(window_id))
                    } else {
                        Ok(())
                    }
                }
            },
        )
        .unwrap();

        assert_eq!(
            take_calls(&calls),
            vec!["make_key:1:2:77", "raise:77:5151:1", "raise:77:5151:2"]
        );
    }

    #[test]
    fn focus_window_via_process_and_raise_retries_missing_ax_window_during_raise() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let attempts = Rc::new(RefCell::new(0usize));

        focus_window_via_process_and_raise(
            77,
            |_| Ok(5151),
            |_| {
                Ok(ProcessSerialNumber {
                    high_long_of_psn: 1,
                    low_long_of_psn: 2,
                })
            },
            {
                let calls = calls.clone();
                move |psn, window_id| {
                    calls.borrow_mut().push(format!(
                        "front:{}:{}:{}",
                        psn.high_long_of_psn, psn.low_long_of_psn, window_id
                    ));
                    Ok(())
                }
            },
            {
                let calls = calls.clone();
                move |psn, window_id| {
                    calls.borrow_mut().push(format!(
                        "make_key:{}:{}:{}",
                        psn.high_long_of_psn, psn.low_long_of_psn, window_id
                    ));
                    Ok(())
                }
            },
            {
                let calls = calls.clone();
                let attempts = attempts.clone();
                move |window_id, pid| {
                    let mut attempts = attempts.borrow_mut();
                    *attempts += 1;
                    calls
                        .borrow_mut()
                        .push(format!("raise:{window_id}:{pid}:{}", *attempts));
                    if *attempts == 1 {
                        Err(MacosNativeOperationError::MissingWindow(window_id))
                    } else {
                        Ok(())
                    }
                }
            },
        )
        .unwrap();

        assert_eq!(
            take_calls(&calls),
            vec![
                "front:1:2:77",
                "make_key:1:2:77",
                "raise:77:5151:1",
                "raise:77:5151:2",
            ]
        );
    }

    #[test]
    fn focus_window_via_process_and_raise_waits_past_three_missing_ax_retries() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let attempts = Rc::new(RefCell::new(0usize));

        focus_window_via_process_and_raise(
            77,
            |_| Ok(5151),
            |_| {
                Ok(ProcessSerialNumber {
                    high_long_of_psn: 1,
                    low_long_of_psn: 2,
                })
            },
            {
                let calls = calls.clone();
                move |psn, window_id| {
                    calls.borrow_mut().push(format!(
                        "front:{}:{}:{}",
                        psn.high_long_of_psn, psn.low_long_of_psn, window_id
                    ));
                    Ok(())
                }
            },
            {
                let calls = calls.clone();
                move |psn, window_id| {
                    calls.borrow_mut().push(format!(
                        "make_key:{}:{}:{}",
                        psn.high_long_of_psn, psn.low_long_of_psn, window_id
                    ));
                    Ok(())
                }
            },
            {
                let calls = calls.clone();
                let attempts = attempts.clone();
                move |window_id, pid| {
                    let mut attempts = attempts.borrow_mut();
                    *attempts += 1;
                    calls
                        .borrow_mut()
                        .push(format!("raise:{window_id}:{pid}:{}", *attempts));
                    if *attempts < 4 {
                        Err(MacosNativeOperationError::MissingWindow(window_id))
                    } else {
                        Ok(())
                    }
                }
            },
        )
        .unwrap();

        assert_eq!(
            take_calls(&calls),
            vec![
                "front:1:2:77",
                "make_key:1:2:77",
                "raise:77:5151:1",
                "raise:77:5151:2",
                "raise:77:5151:3",
                "raise:77:5151:4",
            ]
        );
    }

    #[test]
    fn focus_window_switches_to_target_space_before_fronting_window() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let api = SpaceSettlingApi::new(focus_target_topology_fixture(77, 9), calls.clone(), 0);
        let ctx = MacosNativeContext::connect_with_api(api).unwrap();

        ctx.focus_window(77).unwrap();

        let calls = take_calls(&calls);
        let switch_idx = calls
            .iter()
            .position(|call| call == "switch_space:9")
            .expect("space switch should happen");
        let focus_idx = calls
            .iter()
            .position(|call| call == "focus_window:77")
            .expect("window focus should happen");

        assert!(
            switch_idx < focus_idx,
            "space switch should complete before fronting the target window"
        );
    }

    #[test]
    fn focus_window_waits_for_target_space_to_become_active_before_fronting_window() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let api = SpaceSettlingApi::new(focus_target_topology_fixture(77, 9), calls.clone(), 2);
        let ctx = MacosNativeContext::connect_with_api(api).unwrap();

        ctx.focus_window(77).unwrap();

        let calls = take_calls(&calls);
        let switch_idx = calls
            .iter()
            .position(|call| call == "switch_space:9")
            .expect("space switch should happen first");
        let focus_idx = calls
            .iter()
            .position(|call| call == "focus_window:77")
            .expect("window focus should happen after the Space settles");
        let settle_checks = calls[switch_idx + 1..focus_idx]
            .iter()
            .filter(|call| call.as_str() == "active_space_ids")
            .count();

        assert!(
            settle_checks > 0,
            "focus should poll active_space_ids after switching Spaces before fronting the target window"
        );
    }

    #[test]
    fn move_window_to_space_uses_space_move_primitive() {
        let (ctx, calls) = fake_context_for_move(51, 12);

        ctx.move_window_to_space(51, 12).unwrap();

        assert_eq!(take_calls(&calls), vec!["move_window_to_space:51:12"]);
    }

    #[test]
    fn switch_space_uses_space_switch_primitive() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let api = SpaceSettlingApi::new(focus_target_topology_fixture(51, 12), calls.clone(), 0);
        let ctx = MacosNativeContext::connect_with_api(api).unwrap();

        ctx.switch_space(12).unwrap();

        let calls = take_calls(&calls);
        assert!(
            calls.iter().any(|call| call == "switch_space:12"),
            "switch_space should invoke the Space switch primitive"
        );
    }

    #[test]
    fn switch_space_waits_for_target_space_to_become_active_before_returning() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let api = SpaceSettlingApi::new(focus_target_topology_fixture(77, 9), calls.clone(), 2);
        let ctx = MacosNativeContext::connect_with_api(api).unwrap();

        ctx.switch_space(9).unwrap();

        let calls = take_calls(&calls);
        let switch_idx = calls
            .iter()
            .position(|call| call == "switch_space:9")
            .expect("space switch should happen");
        let settle_checks = calls[switch_idx + 1..]
            .iter()
            .filter(|call| call.as_str() == "active_space_ids")
            .count();

        assert!(
            settle_checks > 0,
            "switch_space should poll active_space_ids before returning"
        );
    }

    #[test]
    fn switch_space_waits_for_onscreen_windows_to_leave_source_space_before_returning() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let api = SpacePresentationApi::new(
            focus_target_topology_fixture(77, 9),
            calls.clone(),
            vec![
                HashSet::from([11]),
                HashSet::from([11]),
                HashSet::from([77]),
            ],
        );
        let ctx = MacosNativeContext::connect_with_api(api).unwrap();

        ctx.switch_space(9).unwrap();

        let calls = take_calls(&calls);
        let switch_idx = calls
            .iter()
            .position(|call| call == "switch_space:9")
            .expect("space switch should happen");
        let onscreen_checks = calls[switch_idx + 1..]
            .iter()
            .filter(|call| call.as_str() == "onscreen_window_ids")
            .count();

        assert!(
            onscreen_checks > 0,
            "switch_space should poll onscreen window ids before returning"
        );
    }

    #[test]
    fn switch_space_allows_nonfocused_source_windows_to_remain_onscreen() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let topology = RawTopologySnapshot {
            spaces: vec![raw_desktop_space(1), raw_desktop_space(9)],
            active_space_ids: HashSet::from([1]),
            active_space_windows: HashMap::from([(
                1,
                vec![
                    raw_window(11).with_visible_index(0).with_pid(1111),
                    raw_window(12).with_pid(1212),
                    raw_window(13).with_pid(1313),
                ],
            )]),
            inactive_space_window_ids: HashMap::from([(9, vec![77])]),
            focused_window_id: Some(11),
        };
        let api =
            SpacePresentationApi::new(topology, calls.clone(), vec![HashSet::from([12, 13, 77])]);
        let ctx = MacosNativeContext::connect_with_api(api).unwrap();

        ctx.switch_space(9).unwrap();

        let calls = take_calls(&calls);
        assert!(
            calls.iter().any(|call| call == "switch_space:9"),
            "switch_space should still complete when only the focused source window disappears"
        );
    }

    #[test]
    fn switch_space_completes_when_target_space_stays_visible_but_source_focus_lingers_onscreen() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let api = SpacePresentationApi::new(
            focus_target_topology_fixture(77, 9),
            calls.clone(),
            vec![
                HashSet::from([11, 77]),
                HashSet::from([11, 77]),
                HashSet::from([11, 77]),
            ],
        );
        let ctx = MacosNativeContext::connect_with_api(api).unwrap();

        ctx.switch_space(9).unwrap();

        let calls = take_calls(&calls);
        let onscreen_checks = calls
            .iter()
            .filter(|call| call.as_str() == "onscreen_window_ids")
            .count();
        assert!(
            onscreen_checks > 1,
            "switch_space should confirm stable target visibility before tolerating a lingering source-focused window id"
        );
    }

    #[test]
    fn focus_window_uses_topology_pid_when_direct_window_lookup_flakes_after_space_switch() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let api = KnownPidAfterSwitchApi::new(focus_target_topology_fixture(77, 9), calls.clone());
        let ctx = MacosNativeContext::connect_with_api(api).unwrap();

        ctx.focus_window(77).unwrap();

        let calls = take_calls(&calls);
        assert_eq!(
            calls,
            vec![
                "switch_space:9".to_string(),
                "focus_window_with_known_pid:77:5151".to_string(),
            ],
            "focus_window should reuse the pid from the refreshed active-space topology instead of re-looking up the window"
        );
    }

    #[test]
    fn context_happy_path_returns_active_space_and_focuses_window() {
        let (ctx, calls) = fake_context_with_active_window_calls(100);

        assert_eq!(ctx.focused_window().unwrap().id, 100);
        ctx.focus_window(100).unwrap();

        assert_eq!(take_calls(&calls), vec!["focus_window:100"]);
    }

    #[test]
    fn backend_focus_direction_selects_closest_neighbor_by_geometry() {
        let _config = install_macos_native_focus_config("radial_center");
        let calls = Rc::new(RefCell::new(Vec::new()));
        let topology = RawTopologySnapshot {
            spaces: vec![raw_desktop_space(1)],
            active_space_ids: HashSet::from([1]),
            active_space_windows: HashMap::from([(
                1,
                vec![
                    raw_window(10)
                        .with_pid(1010)
                        .with_app_id("com.example.left")
                        .with_title("left")
                        .with_frame(crate::engine::topology::Rect {
                            x: 0,
                            y: 0,
                            w: 100,
                            h: 100,
                        }),
                    raw_window(20)
                        .with_visible_index(0)
                        .with_pid(2020)
                        .with_app_id("com.example.center")
                        .with_title("center")
                        .with_frame(crate::engine::topology::Rect {
                            x: 120,
                            y: 0,
                            w: 100,
                            h: 100,
                        }),
                    raw_window(30)
                        .with_pid(3030)
                        .with_app_id("com.example.right")
                        .with_title("right")
                        .with_frame(crate::engine::topology::Rect {
                            x: 240,
                            y: 0,
                            w: 100,
                            h: 100,
                        }),
                ],
            )]),
            inactive_space_window_ids: HashMap::new(),
            focused_window_id: Some(20),
        };
        let api = FakeNativeApi::default()
            .with_topology(topology)
            .with_calls(calls.clone());
        let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        adapter.focus_direction_inner(Direction::West).unwrap();

        assert_eq!(take_calls(&calls), vec!["focus_window:10"]);
    }

    #[test]
    fn focus_direction_uses_outer_policy_with_native_snapshot() {
        let _config = install_macos_native_focus_config("radial_center");
        let api = RecordingFocusApi::from_snapshot(NativeDesktopSnapshot {
            spaces: vec![NativeSpaceSnapshot {
                id: 1,
                display_index: 0,
                active: true,
                kind: SpaceKind::Desktop,
            }],
            active_space_ids: HashSet::from([1]),
            windows: vec![
                NativeWindowSnapshot {
                    id: 100,
                    pid: Some(2000),
                    app_id: Some("com.example.west".to_string()),
                    title: Some("west".to_string()),
                    bounds: Some(NativeBounds {
                        x: 0,
                        y: 0,
                        width: 100,
                        height: 100,
                    }),
                    level: 0,
                    space_id: 1,
                    order_index: Some(0),
                },
                NativeWindowSnapshot {
                    id: 101,
                    pid: Some(2001),
                    app_id: Some("com.example.focused".to_string()),
                    title: Some("focused".to_string()),
                    bounds: Some(NativeBounds {
                        x: 200,
                        y: 0,
                        width: 100,
                        height: 100,
                    }),
                    level: 0,
                    space_id: 1,
                    order_index: Some(1),
                },
            ],
            focused_window_id: Some(101),
        });
        let recorded = api.clone();
        let mut adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        WindowManagerSession::focus_direction(&mut adapter, Direction::West).unwrap();

        assert_eq!(
            recorded.api_calls(),
            vec![
                NativeCall::DesktopSnapshot,
                NativeCall::FocusWindowWithPid(100, 2000)
            ]
        );
    }

    #[test]
    fn focus_direction_delegates_same_pid_splitview_mechanics_to_backend_helper() {
        let _config = install_macos_native_focus_config("overlap_then_gap");
        let api = RecordingSameSpaceDelegationApi::from_snapshot(NativeDesktopSnapshot {
            spaces: vec![NativeSpaceSnapshot {
                id: 1,
                display_index: 0,
                active: true,
                kind: SpaceKind::SplitView,
            }],
            active_space_ids: HashSet::from([1]),
            windows: vec![
                NativeWindowSnapshot {
                    id: 10,
                    pid: Some(3350),
                    app_id: Some("com.github.wez.wezterm".to_string()),
                    title: Some("left-pane".to_string()),
                    bounds: Some(NativeBounds {
                        x: 0,
                        y: 0,
                        width: 120,
                        height: 120,
                    }),
                    level: 0,
                    space_id: 1,
                    order_index: Some(0),
                },
                NativeWindowSnapshot {
                    id: 15,
                    pid: Some(926),
                    app_id: Some("ai.perplexity.mac".to_string()),
                    title: Some("interior-helper".to_string()),
                    bounds: Some(NativeBounds {
                        x: 150,
                        y: 0,
                        width: 60,
                        height: 120,
                    }),
                    level: 0,
                    space_id: 1,
                    order_index: Some(1),
                },
                NativeWindowSnapshot {
                    id: 20,
                    pid: Some(926),
                    app_id: Some("ai.perplexity.mac".to_string()),
                    title: Some("right-pane".to_string()),
                    bounds: Some(NativeBounds {
                        x: 220,
                        y: 0,
                        width: 120,
                        height: 120,
                    }),
                    level: 0,
                    space_id: 1,
                    order_index: Some(2),
                },
            ],
            focused_window_id: Some(20),
        });
        let recorded = api.clone();
        let mut adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        WindowManagerSession::focus_direction(&mut adapter, Direction::West).unwrap();

        assert_eq!(
            recorded.api_calls(),
            vec![
                NativeCall::DesktopSnapshot,
                NativeCall::FocusSameSpaceTargetInSnapshot(NativeDirection::West, 15),
            ]
        );
    }

    #[test]
    fn focus_direction_falls_back_to_generic_focus_when_splitview_known_pid_focus_misses_target() {
        let _config = install_macos_native_focus_config("radial_center");
        let api = SplitViewKnownPidFallbackApi::from_snapshot(NativeDesktopSnapshot {
            spaces: vec![NativeSpaceSnapshot {
                id: 1,
                display_index: 0,
                active: true,
                kind: SpaceKind::SplitView,
            }],
            active_space_ids: HashSet::from([1]),
            windows: vec![
                NativeWindowSnapshot {
                    id: 10,
                    pid: Some(924),
                    app_id: Some("ai.perplexity.mac".to_string()),
                    title: Some("left-pane".to_string()),
                    bounds: Some(NativeBounds {
                        x: 0,
                        y: 0,
                        width: 120,
                        height: 120,
                    }),
                    level: 0,
                    space_id: 1,
                    order_index: Some(0),
                },
                NativeWindowSnapshot {
                    id: 20,
                    pid: Some(33_881),
                    app_id: Some("com.apple.MobileSMS".to_string()),
                    title: Some("right-pane".to_string()),
                    bounds: Some(NativeBounds {
                        x: 220,
                        y: 0,
                        width: 120,
                        height: 120,
                    }),
                    level: 0,
                    space_id: 1,
                    order_index: Some(1),
                },
            ],
            focused_window_id: Some(20),
        });
        let recorded = api.clone();
        let mut adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        WindowManagerSession::focus_direction(&mut adapter, Direction::West).unwrap();

        assert_eq!(
            recorded.api_calls(),
            vec![
                "desktop_snapshot".to_string(),
                "focus_window_with_known_pid:10:924".to_string(),
                "desktop_snapshot".to_string(),
                "focus_window:10".to_string(),
            ]
        );
    }

    #[test]
    fn focus_direction_remaps_splitview_target_to_focusable_same_app_peer_when_target_pid_is_stale()
    {
        let _config = install_macos_native_focus_config("radial_center");
        let api = SplitViewSameAppPeerFallbackApi::from_snapshot(
            NativeDesktopSnapshot {
                spaces: vec![NativeSpaceSnapshot {
                    id: 1,
                    display_index: 0,
                    active: true,
                    kind: SpaceKind::SplitView,
                }],
                active_space_ids: HashSet::from([1]),
                windows: vec![
                    NativeWindowSnapshot {
                        id: 23,
                        pid: Some(924),
                        app_id: Some("com.apple.Safari".to_string()),
                        title: Some("stale-left".to_string()),
                        bounds: Some(NativeBounds {
                            x: 0,
                            y: 0,
                            width: 120,
                            height: 120,
                        }),
                        level: 0,
                        space_id: 1,
                        order_index: Some(0),
                    },
                    NativeWindowSnapshot {
                        id: 24,
                        pid: Some(1728),
                        app_id: Some("com.apple.Safari".to_string()),
                        title: Some("focusable-left".to_string()),
                        bounds: Some(NativeBounds {
                            x: 8,
                            y: 0,
                            width: 112,
                            height: 120,
                        }),
                        level: 0,
                        space_id: 1,
                        order_index: Some(1),
                    },
                    NativeWindowSnapshot {
                        id: 20,
                        pid: Some(33_881),
                        app_id: Some("com.apple.MobileSMS".to_string()),
                        title: Some("right-pane".to_string()),
                        bounds: Some(NativeBounds {
                            x: 220,
                            y: 0,
                            width: 120,
                            height: 120,
                        }),
                        level: 0,
                        space_id: 1,
                        order_index: Some(2),
                    },
                ],
                focused_window_id: Some(20),
            },
            HashMap::from([(924, vec![]), (1728, vec![24])]),
        );
        let recorded = api.clone();
        let mut adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        WindowManagerSession::focus_direction(&mut adapter, Direction::West).unwrap();

        assert_eq!(
            recorded.api_calls(),
            vec![
                "desktop_snapshot".to_string(),
                "focus_window_with_known_pid:23:924".to_string(),
                "ax_window_ids_for_pid:1728".to_string(),
                "focus_window_with_known_pid:24:1728".to_string(),
            ]
        );
    }

    #[test]
    fn focus_direction_remaps_splitview_target_to_same_app_peer_even_when_peer_ax_windows_are_empty_preflight()
     {
        let _config = install_macos_native_focus_config("radial_center");
        let api = SplitViewRefreshedTargetFallbackApi::from_snapshots_with_ax_window_ids(
            vec![NativeDesktopSnapshot {
                spaces: vec![NativeSpaceSnapshot {
                    id: 1,
                    display_index: 0,
                    active: true,
                    kind: SpaceKind::SplitView,
                }],
                active_space_ids: HashSet::from([1]),
                windows: vec![
                    NativeWindowSnapshot {
                        id: 23,
                        pid: Some(924),
                        app_id: Some("com.apple.Safari".to_string()),
                        title: Some("stale-left".to_string()),
                        bounds: Some(NativeBounds {
                            x: 0,
                            y: 0,
                            width: 120,
                            height: 120,
                        }),
                        level: 0,
                        space_id: 1,
                        order_index: Some(0),
                    },
                    NativeWindowSnapshot {
                        id: 24,
                        pid: Some(1728),
                        app_id: Some("com.apple.Safari".to_string()),
                        title: Some("live-left".to_string()),
                        bounds: Some(NativeBounds {
                            x: 8,
                            y: 0,
                            width: 112,
                            height: 120,
                        }),
                        level: 0,
                        space_id: 1,
                        order_index: Some(1),
                    },
                    NativeWindowSnapshot {
                        id: 20,
                        pid: Some(33_881),
                        app_id: Some("com.apple.MobileSMS".to_string()),
                        title: Some("right-pane".to_string()),
                        bounds: Some(NativeBounds {
                            x: 220,
                            y: 0,
                            width: 120,
                            height: 120,
                        }),
                        level: 0,
                        space_id: 1,
                        order_index: Some(2),
                    },
                ],
                focused_window_id: Some(20),
            }],
            HashSet::from([(24, 1728)]),
            HashMap::from([(924, vec![]), (1728, vec![])]),
        );
        let recorded = api.clone();
        let mut adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        WindowManagerSession::focus_direction(&mut adapter, Direction::West).unwrap();

        assert_eq!(
            recorded.api_calls(),
            vec![
                "desktop_snapshot".to_string(),
                "focus_window_with_known_pid:23:924".to_string(),
                "ax_window_ids_for_pid:1728".to_string(),
                "focus_window_with_known_pid:24:1728".to_string(),
            ]
        );
    }

    #[test]
    fn focus_direction_requeries_splitview_target_from_fresh_snapshot_after_missing_window() {
        let _config = install_macos_native_focus_config("radial_center");
        let planning_snapshot = NativeDesktopSnapshot {
            spaces: vec![NativeSpaceSnapshot {
                id: 1,
                display_index: 0,
                active: true,
                kind: SpaceKind::SplitView,
            }],
            active_space_ids: HashSet::from([1]),
            windows: vec![
                NativeWindowSnapshot {
                    id: 23,
                    pid: Some(924),
                    app_id: Some("com.apple.Safari".to_string()),
                    title: Some("stale-left".to_string()),
                    bounds: Some(NativeBounds {
                        x: 0,
                        y: 0,
                        width: 120,
                        height: 120,
                    }),
                    level: 0,
                    space_id: 1,
                    order_index: Some(0),
                },
                NativeWindowSnapshot {
                    id: 20,
                    pid: Some(33_881),
                    app_id: Some("com.apple.MobileSMS".to_string()),
                    title: Some("right-pane".to_string()),
                    bounds: Some(NativeBounds {
                        x: 220,
                        y: 0,
                        width: 120,
                        height: 120,
                    }),
                    level: 0,
                    space_id: 1,
                    order_index: Some(1),
                },
            ],
            focused_window_id: Some(20),
        };
        let refreshed_snapshot = NativeDesktopSnapshot {
            spaces: vec![NativeSpaceSnapshot {
                id: 1,
                display_index: 0,
                active: true,
                kind: SpaceKind::SplitView,
            }],
            active_space_ids: HashSet::from([1]),
            windows: vec![
                NativeWindowSnapshot {
                    id: 24,
                    pid: Some(1728),
                    app_id: Some("com.apple.Safari".to_string()),
                    title: Some("live-left".to_string()),
                    bounds: Some(NativeBounds {
                        x: 0,
                        y: 0,
                        width: 120,
                        height: 120,
                    }),
                    level: 0,
                    space_id: 1,
                    order_index: Some(0),
                },
                NativeWindowSnapshot {
                    id: 20,
                    pid: Some(33_881),
                    app_id: Some("com.apple.MobileSMS".to_string()),
                    title: Some("right-pane".to_string()),
                    bounds: Some(NativeBounds {
                        x: 220,
                        y: 0,
                        width: 120,
                        height: 120,
                    }),
                    level: 0,
                    space_id: 1,
                    order_index: Some(1),
                },
            ],
            focused_window_id: Some(20),
        };
        let api = SplitViewRefreshedTargetFallbackApi::from_snapshots(
            vec![planning_snapshot, refreshed_snapshot],
            HashSet::from([(24, 1728)]),
        );
        let recorded = api.clone();
        let mut adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        WindowManagerSession::focus_direction(&mut adapter, Direction::West).unwrap();

        assert_eq!(
            recorded.api_calls(),
            vec![
                "desktop_snapshot".to_string(),
                "focus_window_with_known_pid:23:924".to_string(),
                "desktop_snapshot".to_string(),
                "focus_window_with_known_pid:24:1728".to_string(),
            ]
        );
    }

    #[test]
    fn focus_direction_requeries_splitview_target_from_fresh_snapshot_even_when_focus_state_drifts()
    {
        let _config = install_macos_native_focus_config("radial_center");
        let planning_snapshot = NativeDesktopSnapshot {
            spaces: vec![NativeSpaceSnapshot {
                id: 1,
                display_index: 0,
                active: true,
                kind: SpaceKind::SplitView,
            }],
            active_space_ids: HashSet::from([1]),
            windows: vec![
                NativeWindowSnapshot {
                    id: 23,
                    pid: Some(924),
                    app_id: Some("com.apple.Safari".to_string()),
                    title: Some("stale-left".to_string()),
                    bounds: Some(NativeBounds {
                        x: 0,
                        y: 0,
                        width: 120,
                        height: 120,
                    }),
                    level: 0,
                    space_id: 1,
                    order_index: Some(0),
                },
                NativeWindowSnapshot {
                    id: 20,
                    pid: Some(33_881),
                    app_id: Some("com.apple.MobileSMS".to_string()),
                    title: Some("right-pane".to_string()),
                    bounds: Some(NativeBounds {
                        x: 220,
                        y: 0,
                        width: 120,
                        height: 120,
                    }),
                    level: 0,
                    space_id: 1,
                    order_index: Some(1),
                },
            ],
            focused_window_id: Some(20),
        };
        let refreshed_snapshot = NativeDesktopSnapshot {
            spaces: vec![NativeSpaceSnapshot {
                id: 1,
                display_index: 0,
                active: true,
                kind: SpaceKind::SplitView,
            }],
            active_space_ids: HashSet::from([1]),
            windows: vec![
                NativeWindowSnapshot {
                    id: 24,
                    pid: Some(1728),
                    app_id: Some("com.apple.Safari".to_string()),
                    title: Some("live-left".to_string()),
                    bounds: Some(NativeBounds {
                        x: 0,
                        y: 0,
                        width: 120,
                        height: 120,
                    }),
                    level: 0,
                    space_id: 1,
                    order_index: Some(0),
                },
                NativeWindowSnapshot {
                    id: 20,
                    pid: Some(33_881),
                    app_id: Some("com.apple.MobileSMS".to_string()),
                    title: Some("right-pane".to_string()),
                    bounds: Some(NativeBounds {
                        x: 220,
                        y: 0,
                        width: 120,
                        height: 120,
                    }),
                    level: 0,
                    space_id: 1,
                    order_index: Some(1),
                },
            ],
            focused_window_id: None,
        };
        let api = SplitViewRefreshedTargetFallbackApi::from_snapshots(
            vec![planning_snapshot, refreshed_snapshot],
            HashSet::from([(24, 1728)]),
        );
        let recorded = api.clone();
        let mut adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        WindowManagerSession::focus_direction(&mut adapter, Direction::West).unwrap();

        assert_eq!(
            recorded.api_calls(),
            vec![
                "desktop_snapshot".to_string(),
                "focus_window_with_known_pid:23:924".to_string(),
                "desktop_snapshot".to_string(),
                "focus_window_with_known_pid:24:1728".to_string(),
            ]
        );
    }

    #[test]
    fn focus_direction_remaps_refreshed_splitview_target_to_focusable_same_app_peer_when_direct_target_stays_stale()
     {
        let _config = install_macos_native_focus_config("radial_center");
        let planning_snapshot = NativeDesktopSnapshot {
            spaces: vec![NativeSpaceSnapshot {
                id: 1,
                display_index: 0,
                active: true,
                kind: SpaceKind::SplitView,
            }],
            active_space_ids: HashSet::from([1]),
            windows: vec![
                NativeWindowSnapshot {
                    id: 23,
                    pid: Some(924),
                    app_id: Some("com.apple.Safari".to_string()),
                    title: Some("stale-left".to_string()),
                    bounds: Some(NativeBounds {
                        x: 0,
                        y: 0,
                        width: 120,
                        height: 120,
                    }),
                    level: 0,
                    space_id: 1,
                    order_index: Some(0),
                },
                NativeWindowSnapshot {
                    id: 20,
                    pid: Some(33_881),
                    app_id: Some("com.apple.MobileSMS".to_string()),
                    title: Some("right-pane".to_string()),
                    bounds: Some(NativeBounds {
                        x: 220,
                        y: 0,
                        width: 120,
                        height: 120,
                    }),
                    level: 0,
                    space_id: 1,
                    order_index: Some(1),
                },
            ],
            focused_window_id: Some(20),
        };
        let refreshed_snapshot = NativeDesktopSnapshot {
            spaces: vec![NativeSpaceSnapshot {
                id: 1,
                display_index: 0,
                active: true,
                kind: SpaceKind::SplitView,
            }],
            active_space_ids: HashSet::from([1]),
            windows: vec![
                NativeWindowSnapshot {
                    id: 23,
                    pid: Some(924),
                    app_id: Some("com.apple.Safari".to_string()),
                    title: Some("still-stale-left".to_string()),
                    bounds: Some(NativeBounds {
                        x: 0,
                        y: 0,
                        width: 120,
                        height: 120,
                    }),
                    level: 0,
                    space_id: 1,
                    order_index: Some(0),
                },
                NativeWindowSnapshot {
                    id: 24,
                    pid: Some(1728),
                    app_id: Some("com.apple.Safari".to_string()),
                    title: Some("focusable-left".to_string()),
                    bounds: Some(NativeBounds {
                        x: 8,
                        y: 0,
                        width: 112,
                        height: 120,
                    }),
                    level: 0,
                    space_id: 1,
                    order_index: Some(1),
                },
                NativeWindowSnapshot {
                    id: 20,
                    pid: Some(33_881),
                    app_id: Some("com.apple.MobileSMS".to_string()),
                    title: Some("right-pane".to_string()),
                    bounds: Some(NativeBounds {
                        x: 220,
                        y: 0,
                        width: 120,
                        height: 120,
                    }),
                    level: 0,
                    space_id: 1,
                    order_index: Some(2),
                },
            ],
            focused_window_id: Some(20),
        };
        let api = SplitViewRefreshedTargetFallbackApi::from_snapshots_with_ax_window_ids(
            vec![planning_snapshot, refreshed_snapshot],
            HashSet::from([(24, 1728)]),
            HashMap::from([(924, vec![]), (1728, vec![24])]),
        );
        let recorded = api.clone();
        let mut adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        WindowManagerSession::focus_direction(&mut adapter, Direction::West).unwrap();

        assert_eq!(
            recorded.api_calls(),
            vec![
                "desktop_snapshot".to_string(),
                "focus_window_with_known_pid:23:924".to_string(),
                "desktop_snapshot".to_string(),
                "ax_window_ids_for_pid:1728".to_string(),
                "focus_window_with_known_pid:24:1728".to_string(),
            ]
        );
    }

    #[test]
    fn focus_direction_returns_success_after_switching_to_empty_adjacent_space() {
        let _config = install_macos_native_focus_config("radial_center");
        let api = RecordingCrossSpaceFocusApi::from_snapshots([
            NativeDesktopSnapshot {
                spaces: vec![
                    NativeSpaceSnapshot {
                        id: 1,
                        display_index: 0,
                        active: false,
                        kind: SpaceKind::Desktop,
                    },
                    NativeSpaceSnapshot {
                        id: 2,
                        display_index: 0,
                        active: true,
                        kind: SpaceKind::Desktop,
                    },
                ],
                active_space_ids: HashSet::from([2]),
                windows: vec![NativeWindowSnapshot {
                    id: 200,
                    pid: Some(2200),
                    app_id: Some("com.example.focused".to_string()),
                    title: Some("focused".to_string()),
                    bounds: Some(NativeBounds {
                        x: 200,
                        y: 0,
                        width: 100,
                        height: 100,
                    }),
                    level: 0,
                    space_id: 2,
                    order_index: Some(0),
                }],
                focused_window_id: Some(200),
            },
            NativeDesktopSnapshot {
                spaces: vec![
                    NativeSpaceSnapshot {
                        id: 1,
                        display_index: 0,
                        active: true,
                        kind: SpaceKind::Desktop,
                    },
                    NativeSpaceSnapshot {
                        id: 2,
                        display_index: 0,
                        active: false,
                        kind: SpaceKind::Desktop,
                    },
                ],
                active_space_ids: HashSet::from([1]),
                windows: vec![NativeWindowSnapshot {
                    id: 200,
                    pid: Some(2200),
                    app_id: Some("com.example.focused".to_string()),
                    title: Some("focused".to_string()),
                    bounds: Some(NativeBounds {
                        x: 200,
                        y: 0,
                        width: 100,
                        height: 100,
                    }),
                    level: 0,
                    space_id: 2,
                    order_index: Some(0),
                }],
                focused_window_id: None,
            },
        ]);
        let recorded = api.clone();
        let mut adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        WindowManagerSession::focus_direction(&mut adapter, Direction::West).unwrap();

        assert_eq!(
            recorded.api_calls(),
            vec![
                NativeCall::DesktopSnapshot,
                NativeCall::SwitchSpaceInSnapshot(1, Some(NativeDirection::West)),
                NativeCall::DesktopSnapshot,
            ]
        );
    }

    #[test]
    fn focus_direction_escapes_overlay_only_space_into_adjacent_real_space() {
        let _config = install_macos_native_focus_config("radial_center");
        let api = RecordingCrossSpaceFocusApi::from_snapshots([
            NativeDesktopSnapshot {
                spaces: vec![
                    NativeSpaceSnapshot {
                        id: 1,
                        display_index: 0,
                        active: false,
                        kind: SpaceKind::Desktop,
                    },
                    NativeSpaceSnapshot {
                        id: 2,
                        display_index: 0,
                        active: true,
                        kind: SpaceKind::Desktop,
                    },
                ],
                active_space_ids: HashSet::from([2]),
                windows: vec![
                    NativeWindowSnapshot {
                        id: 33,
                        pid: Some(671),
                        app_id: Some("com.apple.dock".to_string()),
                        title: Some("overlay-west".to_string()),
                        bounds: Some(NativeBounds {
                            x: 0,
                            y: 0,
                            width: 80,
                            height: 80,
                        }),
                        level: 25,
                        space_id: 2,
                        order_index: Some(1),
                    },
                    NativeWindowSnapshot {
                        id: 40,
                        pid: Some(924),
                        app_id: Some("com.apple.controlcenter".to_string()),
                        title: Some("overlay-focused".to_string()),
                        bounds: Some(NativeBounds {
                            x: 200,
                            y: 0,
                            width: 80,
                            height: 80,
                        }),
                        level: 25,
                        space_id: 2,
                        order_index: Some(0),
                    },
                ],
                focused_window_id: Some(40),
            },
            NativeDesktopSnapshot {
                spaces: vec![
                    NativeSpaceSnapshot {
                        id: 1,
                        display_index: 0,
                        active: true,
                        kind: SpaceKind::Desktop,
                    },
                    NativeSpaceSnapshot {
                        id: 2,
                        display_index: 0,
                        active: false,
                        kind: SpaceKind::Desktop,
                    },
                ],
                active_space_ids: HashSet::from([1]),
                windows: vec![NativeWindowSnapshot {
                    id: 200,
                    pid: Some(2200),
                    app_id: Some("com.example.target".to_string()),
                    title: Some("target".to_string()),
                    bounds: Some(NativeBounds {
                        x: 100,
                        y: 0,
                        width: 160,
                        height: 160,
                    }),
                    level: 0,
                    space_id: 1,
                    order_index: Some(0),
                }],
                focused_window_id: None,
            },
        ]);
        let recorded = api.clone();
        let mut adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        WindowManagerSession::focus_direction(&mut adapter, Direction::West).unwrap();

        assert_eq!(
            recorded.api_calls(),
            vec![
                NativeCall::DesktopSnapshot,
                NativeCall::SwitchSpaceInSnapshot(1, Some(NativeDirection::West)),
                NativeCall::DesktopSnapshot,
                NativeCall::FocusWindowWithPid(200, 2200),
            ]
        );
    }

    #[test]
    fn focus_direction_ignores_overlay_target_when_entering_splitview_from_adjacent_space() {
        let _config = install_macos_native_focus_config("radial_center");
        let api = RecordingCrossSpaceFocusApi::from_snapshots([
            NativeDesktopSnapshot {
                spaces: vec![
                    NativeSpaceSnapshot {
                        id: 1,
                        display_index: 0,
                        active: false,
                        kind: SpaceKind::SplitView,
                    },
                    NativeSpaceSnapshot {
                        id: 2,
                        display_index: 0,
                        active: true,
                        kind: SpaceKind::Desktop,
                    },
                ],
                active_space_ids: HashSet::from([2]),
                windows: vec![NativeWindowSnapshot {
                    id: 7866,
                    pid: Some(1728),
                    app_id: Some("com.apple.Safari".to_string()),
                    title: Some("source".to_string()),
                    bounds: Some(NativeBounds {
                        x: 200,
                        y: 100,
                        width: 300,
                        height: 300,
                    }),
                    level: 0,
                    space_id: 2,
                    order_index: Some(0),
                }],
                focused_window_id: Some(7866),
            },
            NativeDesktopSnapshot {
                spaces: vec![
                    NativeSpaceSnapshot {
                        id: 1,
                        display_index: 0,
                        active: true,
                        kind: SpaceKind::SplitView,
                    },
                    NativeSpaceSnapshot {
                        id: 2,
                        display_index: 0,
                        active: false,
                        kind: SpaceKind::Desktop,
                    },
                ],
                active_space_ids: HashSet::from([1]),
                windows: vec![
                    NativeWindowSnapshot {
                        id: 22,
                        pid: Some(33881),
                        app_id: Some("com.apple.MobileSMS".to_string()),
                        title: Some("left".to_string()),
                        bounds: Some(NativeBounds {
                            x: 0,
                            y: 0,
                            width: 500,
                            height: 900,
                        }),
                        level: 0,
                        space_id: 1,
                        order_index: Some(1),
                    },
                    NativeWindowSnapshot {
                        id: 24,
                        pid: Some(1728),
                        app_id: Some("com.apple.Safari".to_string()),
                        title: Some("right".to_string()),
                        bounds: Some(NativeBounds {
                            x: 520,
                            y: 0,
                            width: 500,
                            height: 900,
                        }),
                        level: 0,
                        space_id: 1,
                        order_index: Some(0),
                    },
                    NativeWindowSnapshot {
                        id: 23,
                        pid: Some(924),
                        app_id: Some("com.apple.controlcenter".to_string()),
                        title: Some("overlay".to_string()),
                        bounds: Some(NativeBounds {
                            x: 1040,
                            y: 0,
                            width: 63,
                            height: 39,
                        }),
                        level: 25,
                        space_id: 1,
                        order_index: Some(2),
                    },
                ],
                focused_window_id: Some(985),
            },
        ]);
        let recorded = api.clone();
        let mut adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        WindowManagerSession::focus_direction(&mut adapter, Direction::West).unwrap();

        assert_eq!(
            recorded.api_calls(),
            vec![
                NativeCall::DesktopSnapshot,
                NativeCall::SwitchSpaceInSnapshot(1, Some(NativeDirection::West)),
                NativeCall::DesktopSnapshot,
                NativeCall::FocusWindowWithPid(24, 1728),
            ]
        );
    }

    #[test]
    fn move_direction_uses_outer_geometry_and_backend_frame_actions() {
        let api = RecordingMoveApi::from_snapshot(NativeDesktopSnapshot {
            spaces: vec![NativeSpaceSnapshot {
                id: 1,
                display_index: 0,
                active: true,
                kind: SpaceKind::Desktop,
            }],
            active_space_ids: HashSet::from([1]),
            windows: vec![
                NativeWindowSnapshot {
                    id: 100,
                    pid: Some(2000),
                    app_id: Some("com.example.west".to_string()),
                    title: Some("west".to_string()),
                    bounds: Some(NativeBounds {
                        x: 0,
                        y: 0,
                        width: 100,
                        height: 100,
                    }),
                    level: 0,
                    space_id: 1,
                    order_index: Some(0),
                },
                NativeWindowSnapshot {
                    id: 101,
                    pid: Some(2001),
                    app_id: Some("com.example.focused".to_string()),
                    title: Some("focused".to_string()),
                    bounds: Some(NativeBounds {
                        x: 200,
                        y: 0,
                        width: 100,
                        height: 100,
                    }),
                    level: 0,
                    space_id: 1,
                    order_index: Some(1),
                },
            ],
            focused_window_id: Some(101),
        });
        let recorded = api.clone();
        let mut adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        WindowManagerSession::move_direction(&mut adapter, Direction::West).unwrap();

        assert_eq!(
            recorded.api_calls(),
            vec![
                NativeCall::DesktopSnapshot,
                NativeCall::SwapWindowFrames {
                    source: 101,
                    target: 100,
                },
            ]
        );
    }

    #[test]
    fn move_direction_moves_window_to_adjacent_space_chosen_from_outer_topology() {
        let api = RecordingMoveApi::from_snapshot(NativeDesktopSnapshot {
            spaces: vec![
                NativeSpaceSnapshot {
                    id: 1,
                    display_index: 0,
                    active: false,
                    kind: SpaceKind::Desktop,
                },
                NativeSpaceSnapshot {
                    id: 2,
                    display_index: 0,
                    active: true,
                    kind: SpaceKind::Desktop,
                },
            ],
            active_space_ids: HashSet::from([2]),
            windows: vec![NativeWindowSnapshot {
                id: 200,
                pid: Some(2200),
                app_id: Some("com.example.focused".to_string()),
                title: Some("focused".to_string()),
                bounds: Some(NativeBounds {
                    x: 200,
                    y: 0,
                    width: 100,
                    height: 100,
                }),
                level: 0,
                space_id: 2,
                order_index: Some(0),
            }],
            focused_window_id: Some(200),
        });
        let recorded = api.clone();
        let mut adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        WindowManagerSession::move_direction(&mut adapter, Direction::West).unwrap();

        assert_eq!(
            recorded.api_calls(),
            vec![
                NativeCall::DesktopSnapshot,
                NativeCall::MoveWindowToSpace {
                    window_id: 200,
                    space_id: 1,
                },
            ]
        );
    }

    #[test]
    fn direct_operations_delegate_to_backend_contract() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let topology = RawTopologySnapshot {
            spaces: vec![raw_desktop_space(1)],
            active_space_ids: HashSet::from([1]),
            active_space_windows: HashMap::from([(
                1,
                vec![
                    raw_window(20).with_visible_index(0).with_frame(Rect {
                        x: 120,
                        y: 0,
                        w: 100,
                        h: 100,
                    }),
                    raw_window(30).with_visible_index(1).with_frame(Rect {
                        x: 240,
                        y: 0,
                        w: 100,
                        h: 100,
                    }),
                ],
            )]),
            inactive_space_window_ids: HashMap::new(),
            focused_window_id: Some(20),
        };
        let api = DirectOperationOverrideApi {
            topology,
            calls: calls.clone(),
        };
        let mut adapter = MacosNativeAdapter::connect_with_api(api.clone()).unwrap();
        let ctx = MacosNativeContext::connect_with_api(api).unwrap();

        WindowManagerSession::focus_window_by_id(&mut adapter, 77).unwrap();
        WindowManagerSession::move_direction(&mut adapter, Direction::East).unwrap();
        ctx.move_window_to_space(20, 1).unwrap();

        assert_eq!(
            std::mem::take(&mut *calls.lock().unwrap()),
            vec![
                "focus_window_by_id:77",
                "swap_window_frames:20:30",
                "move_window_to_space:20:1",
            ]
        );
    }

    #[test]
    fn backend_focus_direction_uses_radial_center_strategy() {
        let _config = install_macos_native_focus_config("radial_center");
        let calls = Rc::new(RefCell::new(Vec::new()));
        let topology = RawTopologySnapshot {
            spaces: vec![raw_desktop_space(1)],
            active_space_ids: HashSet::from([1]),
            active_space_windows: HashMap::from([(
                1,
                vec![
                    raw_window(10)
                        .with_visible_index(0)
                        .with_pid(1010)
                        .with_app_id("com.example.source")
                        .with_title("source")
                        .with_frame(Rect {
                            x: 200,
                            y: 100,
                            w: 100,
                            h: 100,
                        }),
                    raw_window(20)
                        .with_pid(2020)
                        .with_app_id("com.example.radial-target")
                        .with_title("radial-target")
                        .with_frame(Rect {
                            x: 40,
                            y: 80,
                            w: 60,
                            h: 60,
                        }),
                    raw_window(30)
                        .with_pid(3030)
                        .with_app_id("com.example.cross-edge-target")
                        .with_title("cross-edge-target")
                        .with_frame(Rect {
                            x: 90,
                            y: 150,
                            w: 130,
                            h: 130,
                        }),
                ],
            )]),
            inactive_space_window_ids: HashMap::new(),
            focused_window_id: Some(10),
        };
        let api = FakeNativeApi::default()
            .with_topology(topology)
            .with_calls(calls.clone());
        let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        adapter.focus_direction_inner(Direction::West).unwrap();

        assert_eq!(take_calls(&calls), vec!["focus_window:20"]);
    }

    #[test]
    fn backend_focus_direction_uses_cross_edge_gap_strategy() {
        let _config = install_macos_native_focus_config("cross_edge_gap");
        let calls = Rc::new(RefCell::new(Vec::new()));
        let topology = RawTopologySnapshot {
            spaces: vec![raw_desktop_space(1)],
            active_space_ids: HashSet::from([1]),
            active_space_windows: HashMap::from([(
                1,
                vec![
                    raw_window(10)
                        .with_visible_index(0)
                        .with_pid(1010)
                        .with_app_id("com.example.source")
                        .with_title("source")
                        .with_frame(Rect {
                            x: 200,
                            y: 100,
                            w: 100,
                            h: 100,
                        }),
                    raw_window(20)
                        .with_pid(2020)
                        .with_app_id("com.example.radial-target")
                        .with_title("radial-target")
                        .with_frame(Rect {
                            x: 40,
                            y: 80,
                            w: 60,
                            h: 60,
                        }),
                    raw_window(30)
                        .with_pid(3030)
                        .with_app_id("com.example.cross-edge-target")
                        .with_title("cross-edge-target")
                        .with_frame(Rect {
                            x: 90,
                            y: 150,
                            w: 130,
                            h: 130,
                        }),
                ],
            )]),
            inactive_space_window_ids: HashMap::new(),
            focused_window_id: Some(10),
        };
        let api = FakeNativeApi::default()
            .with_topology(topology)
            .with_calls(calls.clone());
        let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        adapter.focus_direction_inner(Direction::West).unwrap();

        assert_eq!(take_calls(&calls), vec!["focus_window:30"]);
    }

    #[test]
    fn outer_same_space_focus_target_keeps_split_view_selection_generic() {
        let topology = OuterMacosTopology {
            spaces: vec![OuterMacosSpace {
                id: 1,
                display_index: 0,
                active: true,
                kind: SpaceKind::SplitView,
            }],
            windows: vec![
                OuterMacosWindow {
                    id: 10,
                    pid: Some(3350),
                    space_id: 1,
                    bounds: Some(Rect {
                        x: 0,
                        y: 0,
                        w: 120,
                        h: 120,
                    }),
                    level: 0,
                    order_index: Some(0),
                },
                OuterMacosWindow {
                    id: 15,
                    pid: Some(926),
                    space_id: 1,
                    bounds: Some(Rect {
                        x: 150,
                        y: 0,
                        w: 60,
                        h: 120,
                    }),
                    level: 0,
                    order_index: Some(1),
                },
                OuterMacosWindow {
                    id: 20,
                    pid: Some(926),
                    space_id: 1,
                    bounds: Some(Rect {
                        x: 220,
                        y: 0,
                        w: 120,
                        h: 120,
                    }),
                    level: 0,
                    order_index: Some(2),
                },
            ],
            focused_window_id: Some(20),
            rects: vec![
                DirectedRect {
                    id: 10,
                    rect: Rect {
                        x: 0,
                        y: 0,
                        w: 120,
                        h: 120,
                    },
                },
                DirectedRect {
                    id: 15,
                    rect: Rect {
                        x: 150,
                        y: 0,
                        w: 60,
                        h: 120,
                    },
                },
                DirectedRect {
                    id: 20,
                    rect: Rect {
                        x: 220,
                        y: 0,
                        w: 120,
                        h: 120,
                    },
                },
            ],
        };

        assert_eq!(
            outer_same_space_focus_target(
                &topology,
                Direction::West,
                crate::engine::topology::FloatingFocusStrategy::OverlapThenGap
            ),
            Some(15)
        );
    }

    #[test]
    fn outer_same_space_focus_target_uses_overlay_focus_as_source_geometry() {
        let topology = OuterMacosTopology {
            spaces: vec![OuterMacosSpace {
                id: 1,
                display_index: 0,
                active: true,
                kind: SpaceKind::Desktop,
            }],
            windows: vec![
                OuterMacosWindow {
                    id: 10,
                    pid: Some(1010),
                    space_id: 1,
                    bounds: Some(Rect {
                        x: 0,
                        y: 0,
                        w: 120,
                        h: 120,
                    }),
                    level: 0,
                    order_index: Some(0),
                },
                OuterMacosWindow {
                    id: 20,
                    pid: Some(2020),
                    space_id: 1,
                    bounds: Some(Rect {
                        x: 160,
                        y: 0,
                        w: 120,
                        h: 120,
                    }),
                    level: 0,
                    order_index: Some(1),
                },
                OuterMacosWindow {
                    id: 99,
                    pid: Some(9999),
                    space_id: 1,
                    bounds: Some(Rect {
                        x: 300,
                        y: 0,
                        w: 40,
                        h: 120,
                    }),
                    level: 25,
                    order_index: Some(2),
                },
            ],
            focused_window_id: Some(99),
            rects: vec![
                DirectedRect {
                    id: 10,
                    rect: Rect {
                        x: 0,
                        y: 0,
                        w: 120,
                        h: 120,
                    },
                },
                DirectedRect {
                    id: 20,
                    rect: Rect {
                        x: 160,
                        y: 0,
                        w: 120,
                        h: 120,
                    },
                },
            ],
        };

        assert_eq!(
            outer_same_space_focus_target(
                &topology,
                Direction::West,
                crate::engine::topology::FloatingFocusStrategy::OverlapThenGap
            ),
            Some(20)
        );
    }

    #[test]
    fn backend_focus_direction_prefers_opposite_split_pane_over_interior_same_app_window() {
        let _config = install_macos_native_focus_config("overlap_then_gap");
        let calls = Rc::new(RefCell::new(Vec::new()));
        let topology = RawTopologySnapshot {
            spaces: vec![raw_split_space(1, &[11, 12])],
            active_space_ids: HashSet::from([1]),
            active_space_windows: HashMap::from([(
                1,
                vec![
                    raw_window(10)
                        .with_visible_index(0)
                        .with_pid(3350)
                        .with_app_id("com.github.wez.wezterm")
                        .with_title("left-pane")
                        .with_frame(Rect {
                            x: 0,
                            y: 0,
                            w: 120,
                            h: 120,
                        }),
                    raw_window(15)
                        .with_visible_index(1)
                        .with_pid(926)
                        .with_app_id("ai.perplexity.mac")
                        .with_title("interior-helper")
                        .with_frame(Rect {
                            x: 150,
                            y: 0,
                            w: 60,
                            h: 120,
                        }),
                    raw_window(20)
                        .with_visible_index(2)
                        .with_pid(926)
                        .with_app_id("ai.perplexity.mac")
                        .with_title("right-pane")
                        .with_frame(Rect {
                            x: 220,
                            y: 0,
                            w: 120,
                            h: 120,
                        }),
                ],
            )]),
            inactive_space_window_ids: HashMap::new(),
            focused_window_id: Some(20),
        };
        let api = FakeNativeApi::default()
            .with_topology(topology)
            .with_calls(calls.clone());
        let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        adapter.focus_direction_inner(Direction::West).unwrap();

        assert_eq!(take_calls(&calls), vec!["focus_window:10"]);
    }

    #[test]
    fn backend_split_view_focus_ignores_non_normal_layer_overlay_targets() {
        let _config = install_macos_native_focus_config("radial_center");
        let calls = Rc::new(RefCell::new(Vec::new()));
        let topology = RawTopologySnapshot {
            spaces: vec![raw_split_space(1, &[11, 12])],
            active_space_ids: HashSet::from([1]),
            active_space_windows: HashMap::from([(
                1,
                vec![
                    raw_window(100)
                        .with_visible_index(0)
                        .with_pid(4001)
                        .with_app_id("com.example.source")
                        .with_title("source")
                        .with_frame(Rect {
                            x: 0,
                            y: 120,
                            w: 500,
                            h: 900,
                        }),
                    raw_window(159)
                        .with_visible_index(1)
                        .with_pid(946)
                        .with_app_id("com.example.target")
                        .with_title("target")
                        .with_frame(Rect {
                            x: 1200,
                            y: 120,
                            w: 500,
                            h: 900,
                        }),
                    raw_window(52)
                        .with_visible_index(2)
                        .with_level(25)
                        .with_pid(950)
                        .with_app_id("com.apple.controlcenter")
                        .with_title("Control Center")
                        .with_frame(Rect {
                            x: 1739,
                            y: 0,
                            w: 63,
                            h: 39,
                        }),
                ],
            )]),
            inactive_space_window_ids: HashMap::new(),
            focused_window_id: Some(100),
        };
        let api = FakeNativeApi::default()
            .with_topology(topology)
            .with_calls(calls.clone());
        let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        adapter.focus_direction_inner(Direction::East).unwrap();

        assert_eq!(take_calls(&calls), vec!["focus_window:159"]);
    }

    #[test]
    fn backend_focus_direction_preflights_same_pid_splitview_ax_target_before_focus_attempt() {
        let _config = install_macos_native_focus_config("overlap_then_gap");
        let calls = Rc::new(RefCell::new(Vec::new()));
        let topology = RawTopologySnapshot {
            spaces: vec![raw_split_space(1, &[11, 12])],
            active_space_ids: HashSet::from([1]),
            active_space_windows: HashMap::from([(
                1,
                vec![
                    raw_window(998)
                        .with_visible_index(0)
                        .with_pid(4613)
                        .with_app_id("com.github.wez.wezterm")
                        .with_title("stale-left")
                        .with_frame(Rect {
                            x: 0,
                            y: 0,
                            w: 120,
                            h: 120,
                        }),
                    raw_window(999)
                        .with_visible_index(1)
                        .with_pid(4613)
                        .with_app_id("com.github.wez.wezterm")
                        .with_title("actual-left")
                        .with_frame(Rect {
                            x: 12,
                            y: 0,
                            w: 108,
                            h: 120,
                        }),
                    raw_window(410)
                        .with_visible_index(2)
                        .with_pid(4613)
                        .with_app_id("com.github.wez.wezterm")
                        .with_title("focused-right")
                        .with_frame(Rect {
                            x: 220,
                            y: 0,
                            w: 120,
                            h: 120,
                        }),
                ],
            )]),
            inactive_space_window_ids: HashMap::new(),
            focused_window_id: Some(410),
        };
        let api = SamePidAxFallbackApi {
            topology,
            ax_backed_window_ids: vec![999, 410],
            calls: calls.clone(),
        };
        let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        adapter.focus_direction_inner(Direction::West).unwrap();

        assert_eq!(
            take_calls(&calls),
            vec!["focus_window_with_known_pid:999:4613"]
        );
    }

    #[test]
    fn focus_direction_uses_planning_snapshot_for_same_pid_ax_fallback() {
        let _config = install_macos_native_focus_config("overlap_then_gap");
        let calls = Arc::new(Mutex::new(Vec::new()));
        let topology_snapshot_calls = Arc::new(Mutex::new(0));
        let planning_topology = RawTopologySnapshot {
            spaces: vec![raw_split_space(1, &[11, 12])],
            active_space_ids: HashSet::from([1]),
            active_space_windows: HashMap::from([(
                1,
                vec![
                    raw_window(998)
                        .with_visible_index(0)
                        .with_pid(4613)
                        .with_app_id("com.github.wez.wezterm")
                        .with_title("stale-left")
                        .with_frame(Rect {
                            x: 0,
                            y: 0,
                            w: 120,
                            h: 120,
                        }),
                    raw_window(999)
                        .with_visible_index(1)
                        .with_pid(4613)
                        .with_app_id("com.github.wez.wezterm")
                        .with_title("actual-left")
                        .with_frame(Rect {
                            x: 12,
                            y: 0,
                            w: 108,
                            h: 120,
                        }),
                    raw_window(410)
                        .with_visible_index(2)
                        .with_pid(4613)
                        .with_app_id("com.github.wez.wezterm")
                        .with_title("focused-right")
                        .with_frame(Rect {
                            x: 220,
                            y: 0,
                            w: 120,
                            h: 120,
                        }),
                ],
            )]),
            inactive_space_window_ids: HashMap::new(),
            focused_window_id: Some(410),
        };
        let execution_topology = RawTopologySnapshot {
            spaces: vec![raw_desktop_space(1)],
            active_space_ids: HashSet::from([1]),
            active_space_windows: HashMap::from([(
                1,
                vec![
                    raw_window(998)
                        .with_visible_index(0)
                        .with_pid(4613)
                        .with_app_id("com.github.wez.wezterm")
                        .with_title("stale-left")
                        .with_frame(Rect {
                            x: 0,
                            y: 0,
                            w: 120,
                            h: 120,
                        }),
                    raw_window(999)
                        .with_visible_index(1)
                        .with_pid(4613)
                        .with_app_id("com.github.wez.wezterm")
                        .with_title("actual-left")
                        .with_frame(Rect {
                            x: 12,
                            y: 0,
                            w: 108,
                            h: 120,
                        }),
                ],
            )]),
            inactive_space_window_ids: HashMap::new(),
            focused_window_id: Some(999),
        };
        let api = SequencedSamePidAxFallbackApi {
            planning_topology,
            execution_topology,
            ax_backed_window_ids: vec![999, 410],
            calls: calls.clone(),
            topology_snapshot_calls: topology_snapshot_calls.clone(),
        };
        let mut adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        WindowManagerSession::focus_direction(&mut adapter, Direction::West).unwrap();

        assert_eq!(
            std::mem::take(&mut *calls.lock().unwrap()),
            vec!["focus_window_with_known_pid:999:4613"]
        );
        assert_eq!(*topology_snapshot_calls.lock().unwrap(), 1);
    }

    #[test]
    fn backend_focus_direction_switches_to_adjacent_split_space_when_desktop_helper_does_not_extend_west()
     {
        let _config = install_macos_native_focus_config("overlap_then_gap");
        let calls = Rc::new(RefCell::new(Vec::new()));
        let topology = RawTopologySnapshot {
            spaces: vec![raw_split_space(1, &[11, 12]), raw_desktop_space(2)],
            active_space_ids: HashSet::from([2]),
            active_space_windows: HashMap::from([(
                2,
                vec![
                    raw_window(203)
                        .with_visible_index(0)
                        .with_pid(898)
                        .with_app_id("com.apple.Safari")
                        .with_title("frontmost")
                        .with_frame(Rect {
                            x: 0,
                            y: 0,
                            w: 240,
                            h: 120,
                        }),
                    raw_window(201)
                        .with_visible_index(1)
                        .with_pid(898)
                        .with_app_id("com.apple.Safari")
                        .with_title("helper")
                        .with_frame(Rect {
                            x: 40,
                            y: 0,
                            w: 80,
                            h: 120,
                        }),
                ],
            )]),
            inactive_space_window_ids: HashMap::from([(1, vec![10, 20])]),
            focused_window_id: Some(203),
        };
        let api = SwitchThenFocusApi {
            topology,
            switched_space_windows: HashMap::from([(
                1,
                vec![
                    raw_window(10)
                        .with_visible_index(0)
                        .with_pid(3350)
                        .with_app_id("com.github.wez.wezterm")
                        .with_title("left-pane")
                        .with_frame(Rect {
                            x: 0,
                            y: 0,
                            w: 120,
                            h: 120,
                        }),
                    raw_window(20)
                        .with_visible_index(1)
                        .with_pid(926)
                        .with_app_id("ai.perplexity.mac")
                        .with_title("right-pane")
                        .with_frame(Rect {
                            x: 220,
                            y: 0,
                            w: 120,
                            h: 120,
                        }),
                ],
            )]),
            current_space_id: Rc::new(RefCell::new(2)),
            calls: calls.clone(),
        };
        let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        adapter.focus_direction_inner(Direction::West).unwrap();

        assert_eq!(
            take_calls(&calls),
            vec!["switch_space:1", "focus_window:20"]
        );
    }

    #[test]
    fn backend_focus_direction_switches_to_adjacent_space_when_desktop_helper_ties_west_edge_despite_visible_order()
     {
        let _config = install_macos_native_focus_config("overlap_then_gap");
        let calls = Rc::new(RefCell::new(Vec::new()));
        let topology = RawTopologySnapshot {
            spaces: vec![raw_split_space(1, &[11, 12]), raw_desktop_space(2)],
            active_space_ids: HashSet::from([2]),
            active_space_windows: HashMap::from([(
                2,
                vec![
                    raw_window(203)
                        .with_visible_index(1)
                        .with_pid(898)
                        .with_app_id("com.apple.Safari")
                        .with_title("frontmost")
                        .with_frame(Rect {
                            x: 0,
                            y: 0,
                            w: 240,
                            h: 120,
                        }),
                    raw_window(201)
                        .with_visible_index(0)
                        .with_pid(898)
                        .with_app_id("com.apple.Safari")
                        .with_title("helper")
                        .with_frame(Rect {
                            x: 0,
                            y: 0,
                            w: 80,
                            h: 120,
                        }),
                ],
            )]),
            inactive_space_window_ids: HashMap::from([(1, vec![10, 20])]),
            focused_window_id: Some(203),
        };
        let api = SwitchThenFocusApi {
            topology,
            switched_space_windows: HashMap::from([(
                1,
                vec![
                    raw_window(10)
                        .with_visible_index(0)
                        .with_pid(3350)
                        .with_app_id("com.github.wez.wezterm")
                        .with_title("left-pane")
                        .with_frame(Rect {
                            x: 0,
                            y: 0,
                            w: 120,
                            h: 120,
                        }),
                    raw_window(20)
                        .with_visible_index(1)
                        .with_pid(926)
                        .with_app_id("ai.perplexity.mac")
                        .with_title("right-pane")
                        .with_frame(Rect {
                            x: 220,
                            y: 0,
                            w: 120,
                            h: 120,
                        }),
                ],
            )]),
            current_space_id: Rc::new(RefCell::new(2)),
            calls: calls.clone(),
        };
        let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        adapter.focus_direction_inner(Direction::West).unwrap();

        assert_eq!(
            take_calls(&calls),
            vec!["switch_space:1", "focus_window:20"]
        );
    }

    #[test]
    fn backend_focus_direction_uses_same_post_switch_snapshot_for_selection_and_focus() {
        let _config = install_macos_native_focus_config("radial_center");
        let calls = Rc::new(RefCell::new(Vec::new()));
        let initial_topology = RawTopologySnapshot {
            spaces: vec![raw_desktop_space(1), raw_desktop_space(2)],
            active_space_ids: HashSet::from([1]),
            active_space_windows: HashMap::from([(
                1,
                vec![
                    raw_window(10)
                        .with_visible_index(0)
                        .with_pid(1010)
                        .with_app_id("com.example.source")
                        .with_title("source")
                        .with_frame(Rect {
                            x: 0,
                            y: 0,
                            w: 100,
                            h: 100,
                        }),
                ],
            )]),
            inactive_space_window_ids: HashMap::from([(2, vec![21, 22])]),
            focused_window_id: Some(10),
        };
        let switched_topology = RawTopologySnapshot {
            spaces: vec![raw_desktop_space(1), raw_desktop_space(2)],
            active_space_ids: HashSet::from([2]),
            active_space_windows: HashMap::from([(
                2,
                vec![
                    raw_window(21)
                        .with_visible_index(0)
                        .with_pid(2121)
                        .with_app_id("com.example.visible")
                        .with_title("visible")
                        .with_frame(Rect {
                            x: 0,
                            y: 0,
                            w: 100,
                            h: 100,
                        }),
                ],
            )]),
            inactive_space_window_ids: HashMap::new(),
            focused_window_id: Some(21),
        };
        let drifted_topology = RawTopologySnapshot {
            spaces: vec![raw_desktop_space(1), raw_desktop_space(2)],
            active_space_ids: HashSet::from([2]),
            active_space_windows: HashMap::from([(
                2,
                vec![
                    raw_window(22)
                        .with_visible_index(0)
                        .with_pid(2222)
                        .with_app_id("com.example.drifted")
                        .with_title("drifted")
                        .with_frame(Rect {
                            x: 240,
                            y: 0,
                            w: 100,
                            h: 100,
                        }),
                ],
            )]),
            inactive_space_window_ids: HashMap::new(),
            focused_window_id: Some(22),
        };
        let api = PostSwitchSelectionDriftApi::new(
            initial_topology,
            switched_topology,
            drifted_topology
                .active_space_windows
                .get(&2)
                .cloned()
                .unwrap_or_default(),
            calls.clone(),
        );
        let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        adapter.focus_direction_inner(Direction::East).unwrap();

        assert_eq!(
            take_calls(&calls),
            vec!["switch_space:2", "focus_window:21"]
        );
    }

    #[test]
    fn backend_focus_direction_switches_then_focuses_rightmost_window_in_previous_space_when_no_west_window_exists()
     {
        let _config = install_macos_native_focus_config("radial_center");
        let calls = Rc::new(RefCell::new(Vec::new()));
        let topology = RawTopologySnapshot {
            spaces: vec![
                raw_desktop_space(1),
                raw_desktop_space(2),
                raw_desktop_space(3),
            ],
            active_space_ids: HashSet::from([2]),
            active_space_windows: HashMap::from([(
                2,
                vec![
                    raw_window(20)
                        .with_visible_index(0)
                        .with_pid(2020)
                        .with_app_id("com.example.center")
                        .with_title("center")
                        .with_frame(crate::engine::topology::Rect {
                            x: 120,
                            y: 0,
                            w: 100,
                            h: 100,
                        }),
                ],
            )]),
            inactive_space_window_ids: HashMap::from([(1, vec![11, 12]), (3, vec![30])]),
            focused_window_id: Some(20),
        };
        let api = SwitchThenFocusApi {
            topology,
            switched_space_windows: HashMap::from([(
                1,
                vec![
                    raw_window(11)
                        .with_visible_index(0)
                        .with_pid(1010)
                        .with_app_id("com.example.left")
                        .with_title("left")
                        .with_frame(Rect {
                            x: 0,
                            y: 0,
                            w: 100,
                            h: 100,
                        }),
                    raw_window(12)
                        .with_visible_index(1)
                        .with_pid(1212)
                        .with_app_id("com.example.right")
                        .with_title("right")
                        .with_frame(Rect {
                            x: 240,
                            y: 0,
                            w: 100,
                            h: 100,
                        }),
                ],
            )]),
            current_space_id: Rc::new(RefCell::new(2)),
            calls: calls.clone(),
        };
        let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        adapter.focus_direction_inner(Direction::West).unwrap();

        assert_eq!(
            take_calls(&calls),
            vec!["switch_space:1", "focus_window:12"]
        );
    }

    #[test]
    fn backend_focus_direction_switches_then_focuses_window_in_previous_space_on_same_display_only()
    {
        let _config = install_macos_native_focus_config("radial_center");
        let calls = Rc::new(RefCell::new(Vec::new()));
        let topology = RawTopologySnapshot {
            spaces: vec![
                raw_desktop_space_on_display(1, 0),
                raw_desktop_space_on_display(2, 0),
                raw_desktop_space_on_display(10, 1),
                raw_desktop_space_on_display(11, 1),
            ],
            active_space_ids: HashSet::from([2, 11]),
            active_space_windows: HashMap::from([
                (
                    2,
                    vec![
                        raw_window(200)
                            .with_pid(2200)
                            .with_app_id("com.example.left-display")
                            .with_title("left display")
                            .with_frame(crate::engine::topology::Rect {
                                x: 0,
                                y: 0,
                                w: 100,
                                h: 100,
                            }),
                    ],
                ),
                (
                    11,
                    vec![
                        raw_window(1100)
                            .with_visible_index(0)
                            .with_pid(1111)
                            .with_app_id("com.example.right-display")
                            .with_title("right display")
                            .with_frame(crate::engine::topology::Rect {
                                x: 120,
                                y: 0,
                                w: 100,
                                h: 100,
                            }),
                    ],
                ),
            ]),
            inactive_space_window_ids: HashMap::from([(1, vec![100]), (10, vec![1000])]),
            focused_window_id: Some(1100),
        };
        let api = SwitchThenFocusApi {
            topology,
            switched_space_windows: HashMap::from([(
                10,
                vec![
                    raw_window(1000)
                        .with_visible_index(0)
                        .with_pid(1001)
                        .with_app_id("com.example.other-display")
                        .with_title("other display")
                        .with_frame(Rect {
                            x: 0,
                            y: 0,
                            w: 100,
                            h: 100,
                        }),
                ],
            )]),
            current_space_id: Rc::new(RefCell::new(11)),
            calls: calls.clone(),
        };
        let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        adapter.focus_direction_inner(Direction::West).unwrap();

        assert_eq!(
            take_calls(&calls),
            vec!["switch_space:10", "focus_window:1000"]
        );
    }

    #[test]
    fn backend_focus_direction_switches_then_focuses_leftmost_window_in_next_space_when_no_east_window_exists()
     {
        let _config = install_macos_native_focus_config("radial_center");
        let calls = Rc::new(RefCell::new(Vec::new()));
        let topology = RawTopologySnapshot {
            spaces: vec![raw_desktop_space(1), raw_desktop_space(2)],
            active_space_ids: HashSet::from([1]),
            active_space_windows: HashMap::from([(
                1,
                vec![
                    raw_window(10)
                        .with_visible_index(0)
                        .with_pid(1010)
                        .with_app_id("com.example.source")
                        .with_title("source")
                        .with_frame(Rect {
                            x: 0,
                            y: 0,
                            w: 100,
                            h: 100,
                        }),
                ],
            )]),
            inactive_space_window_ids: HashMap::from([(2, vec![21, 22])]),
            focused_window_id: Some(10),
        };
        let api = SwitchThenFocusApi {
            topology,
            switched_space_windows: HashMap::from([(
                2,
                vec![
                    raw_window(21)
                        .with_pid(2121)
                        .with_app_id("com.example.left")
                        .with_title("left")
                        .with_frame(Rect {
                            x: 0,
                            y: 0,
                            w: 100,
                            h: 100,
                        }),
                    raw_window(22)
                        .with_pid(2222)
                        .with_app_id("com.example.right")
                        .with_title("right")
                        .with_frame(Rect {
                            x: 240,
                            y: 0,
                            w: 100,
                            h: 100,
                        }),
                ],
            )]),
            current_space_id: Rc::new(RefCell::new(1)),
            calls: calls.clone(),
        };
        let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        adapter.focus_direction_inner(Direction::East).unwrap();

        assert_eq!(
            take_calls(&calls),
            vec!["switch_space:2", "focus_window:21"]
        );
    }

    #[test]
    fn backend_focus_direction_switches_then_focuses_edge_window_when_offspace_metadata_is_missing()
    {
        let _config = install_macos_native_focus_config("radial_center");
        let calls = Rc::new(RefCell::new(Vec::new()));
        let topology = RawTopologySnapshot {
            spaces: vec![raw_desktop_space(1), raw_desktop_space(2)],
            active_space_ids: HashSet::from([1]),
            active_space_windows: HashMap::from([(
                1,
                vec![
                    raw_window(10)
                        .with_visible_index(0)
                        .with_pid(1010)
                        .with_app_id("com.example.source")
                        .with_title("source")
                        .with_frame(Rect {
                            x: 0,
                            y: 0,
                            w: 100,
                            h: 100,
                        }),
                ],
            )]),
            inactive_space_window_ids: HashMap::from([(2, vec![21, 22])]),
            focused_window_id: Some(10),
        };
        let api = SwitchThenFocusApi {
            topology,
            switched_space_windows: HashMap::from([(
                2,
                vec![
                    raw_window(21)
                        .with_visible_index(1)
                        .with_pid(2121)
                        .with_app_id("com.example.left")
                        .with_title("left")
                        .with_frame(Rect {
                            x: 0,
                            y: 0,
                            w: 100,
                            h: 100,
                        }),
                    raw_window(22)
                        .with_visible_index(0)
                        .with_pid(2222)
                        .with_app_id("com.example.right")
                        .with_title("right")
                        .with_frame(Rect {
                            x: 240,
                            y: 0,
                            w: 100,
                            h: 100,
                        }),
                ],
            )]),
            current_space_id: Rc::new(RefCell::new(1)),
            calls: calls.clone(),
        };
        let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        adapter.focus_direction_inner(Direction::East).unwrap();

        assert_eq!(
            take_calls(&calls),
            vec!["switch_space:2", "focus_window:21"]
        );
    }

    #[test]
    fn backend_focus_direction_can_switch_adjacent_space_without_direct_switch_primitive() {
        let _config = install_macos_native_focus_config("radial_center");
        let calls = Rc::new(RefCell::new(Vec::new()));
        let topology = RawTopologySnapshot {
            spaces: vec![raw_desktop_space(1), raw_desktop_space(2)],
            active_space_ids: HashSet::from([1]),
            active_space_windows: HashMap::from([(
                1,
                vec![
                    raw_window(10)
                        .with_visible_index(0)
                        .with_pid(1010)
                        .with_app_id("com.example.source")
                        .with_title("source")
                        .with_frame(Rect {
                            x: 0,
                            y: 0,
                            w: 100,
                            h: 100,
                        }),
                ],
            )]),
            inactive_space_window_ids: HashMap::from([(2, vec![21, 22])]),
            focused_window_id: Some(10),
        };
        let api = AdjacentHotkeyOnlyApi {
            topology,
            switched_space_windows: HashMap::from([(
                2,
                vec![
                    raw_window(21)
                        .with_visible_index(1)
                        .with_pid(2121)
                        .with_app_id("com.example.left")
                        .with_title("left")
                        .with_frame(Rect {
                            x: 0,
                            y: 0,
                            w: 100,
                            h: 100,
                        }),
                    raw_window(22)
                        .with_visible_index(0)
                        .with_pid(2222)
                        .with_app_id("com.example.right")
                        .with_title("right")
                        .with_frame(Rect {
                            x: 240,
                            y: 0,
                            w: 100,
                            h: 100,
                        }),
                ],
            )]),
            current_space_id: Rc::new(RefCell::new(1)),
            calls: calls.clone(),
        };
        let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        adapter.focus_direction_inner(Direction::East).unwrap();

        assert_eq!(take_calls(&calls), vec!["focus_window:21"]);
    }

    #[test]
    fn backend_focus_direction_uses_exact_switch_for_empty_adjacent_space_when_hotkey_would_skip_it()
     {
        let _config = install_macos_native_focus_config("radial_center");
        let calls = Rc::new(RefCell::new(Vec::new()));
        let topology = RawTopologySnapshot {
            spaces: vec![
                raw_desktop_space(1),
                raw_desktop_space(2),
                raw_desktop_space(3),
            ],
            active_space_ids: HashSet::from([3]),
            active_space_windows: HashMap::from([(
                3,
                vec![
                    raw_window(30)
                        .with_visible_index(0)
                        .with_pid(3030)
                        .with_app_id("com.example.center")
                        .with_title("center")
                        .with_frame(crate::engine::topology::Rect {
                            x: 240,
                            y: 0,
                            w: 100,
                            h: 100,
                        }),
                ],
            )]),
            inactive_space_window_ids: HashMap::from([(1, vec![10]), (2, vec![])]),
            focused_window_id: Some(30),
        };
        let api = EmptySpaceSkippingAdjacentHotkeyApi {
            topology,
            switched_space_windows: HashMap::from([(
                1,
                vec![
                    raw_window(10)
                        .with_visible_index(0)
                        .with_pid(1010)
                        .with_app_id("com.example.left")
                        .with_title("left")
                        .with_frame(crate::engine::topology::Rect {
                            x: 0,
                            y: 0,
                            w: 100,
                            h: 100,
                        }),
                ],
            )]),
            current_space_id: Rc::new(RefCell::new(3)),
            adjacent_hotkey_skip_target_space_id: 1,
            calls: calls.clone(),
        };
        let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        adapter.focus_direction_inner(Direction::West).unwrap();

        assert_eq!(take_calls(&calls), vec!["switch_space:2"]);
    }

    #[test]
    fn backend_focus_direction_ignores_ghost_inactive_window_ids_for_empty_adjacent_space() {
        let _config = install_macos_native_focus_config("radial_center");
        let calls = Rc::new(RefCell::new(Vec::new()));
        let topology = RawTopologySnapshot {
            spaces: vec![
                raw_desktop_space(1),
                raw_desktop_space(2),
                raw_desktop_space(3),
            ],
            active_space_ids: HashSet::from([1]),
            active_space_windows: HashMap::from([(
                1,
                vec![
                    raw_window(10)
                        .with_visible_index(0)
                        .with_pid(1010)
                        .with_app_id("com.example.source")
                        .with_title("source")
                        .with_frame(crate::engine::topology::Rect {
                            x: 0,
                            y: 0,
                            w: 100,
                            h: 100,
                        }),
                ],
            )]),
            inactive_space_window_ids: HashMap::from([(2, vec![31, 32]), (3, vec![])]),
            focused_window_id: Some(10),
        };
        let api = EmptySpaceSkippingAdjacentHotkeyApi {
            topology,
            switched_space_windows: HashMap::from([(
                3,
                vec![
                    raw_window(31)
                        .with_visible_index(1)
                        .with_pid(3131)
                        .with_app_id("com.example.skip-left")
                        .with_title("skip-left")
                        .with_frame(crate::engine::topology::Rect {
                            x: 240,
                            y: 0,
                            w: 100,
                            h: 100,
                        }),
                    raw_window(32)
                        .with_visible_index(0)
                        .with_pid(3232)
                        .with_app_id("com.example.skip-right")
                        .with_title("skip-right")
                        .with_frame(crate::engine::topology::Rect {
                            x: 360,
                            y: 0,
                            w: 100,
                            h: 100,
                        }),
                ],
            )]),
            current_space_id: Rc::new(RefCell::new(1)),
            adjacent_hotkey_skip_target_space_id: 3,
            calls: calls.clone(),
        };
        let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        adapter.focus_direction_inner(Direction::East).unwrap();

        assert_eq!(
            take_calls(&calls),
            vec!["switch_adjacent_space:east:2", "switch_space:2"]
        );
    }

    #[test]
    fn backend_move_direction_swaps_with_directional_neighbor() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let topology = RawTopologySnapshot {
            spaces: vec![raw_desktop_space(1)],
            active_space_ids: HashSet::from([1]),
            active_space_windows: HashMap::from([(
                1,
                vec![
                    raw_window(10)
                        .with_pid(1010)
                        .with_title("left")
                        .with_frame(Rect {
                            x: 0,
                            y: 0,
                            w: 100,
                            h: 100,
                        }),
                    raw_window(20)
                        .with_pid(2020)
                        .with_title("center")
                        .with_frame(Rect {
                            x: 120,
                            y: 0,
                            w: 100,
                            h: 100,
                        }),
                    raw_window(30)
                        .with_pid(3030)
                        .with_title("right")
                        .with_frame(Rect {
                            x: 240,
                            y: 0,
                            w: 100,
                            h: 100,
                        }),
                ],
            )]),
            inactive_space_window_ids: HashMap::new(),
            focused_window_id: Some(20),
        };
        let api = SendRecordingApi {
            topology,
            calls: calls.clone(),
        };
        let mut adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        adapter.move_direction(Direction::East).unwrap();

        assert_eq!(
            std::mem::take(&mut *calls.lock().unwrap()),
            vec!["swap_window_frames:20:30"]
        );
    }

    #[test]
    fn stage_manager_targets_are_rejected_explicitly() {
        let (ctx, calls) = fake_context_for_stage_manager_target(88, 9);

        let err = ctx.focus_window(88).unwrap_err();

        assert!(err.to_string().contains("Stage Manager"));
        assert!(take_calls(&calls).is_empty());
    }

    #[test]
    fn switch_space_rejects_unknown_target_space_explicitly() {
        let (ctx, calls) = fake_context_for_move(51, 12);

        let err = ctx.switch_space(99).unwrap_err();

        assert_eq!(err, MacosNativeOperationError::MissingSpace(99));
        assert!(take_calls(&calls).is_empty());
    }

    #[test]
    fn move_window_to_space_rejects_unknown_target_space_explicitly() {
        let (ctx, calls) = fake_context_for_move(51, 12);

        let err = ctx.move_window_to_space(51, 99).unwrap_err();

        assert_eq!(err, MacosNativeOperationError::MissingSpace(99));
        assert!(take_calls(&calls).is_empty());
    }

    #[test]
    fn move_window_to_space_rejects_missing_window_explicitly() {
        let (ctx, calls) = fake_context_for_move(51, 12);

        let err = ctx.move_window_to_space(999, 12).unwrap_err();

        assert_eq!(err, MacosNativeOperationError::MissingWindow(999));
        assert!(take_calls(&calls).is_empty());
    }

    #[test]
    fn active_space_snapshot_ordered_window_ids_match_window_ordering_contract() {
        let topology = RawTopologySnapshot {
            spaces: vec![raw_desktop_space(1)],
            active_space_ids: HashSet::from([1]),
            active_space_windows: HashMap::from([(
                1,
                vec![
                    raw_window(11).with_visible_index(1),
                    raw_window(12).with_visible_index(0),
                    raw_window(13).with_level(5),
                ],
            )]),
            inactive_space_window_ids: HashMap::new(),
            focused_window_id: Some(12),
        };

        let spaces = space_snapshots_from_topology(&topology);
        let active = spaces.iter().find(|space| space.is_active).unwrap();
        let windows = window_snapshots_from_topology(&topology);
        let ordered_window_ids_from_windows = windows
            .iter()
            .filter(|window| topology.active_space_ids.contains(&window.space_id))
            .map(|window| (window.id, window.order_index.unwrap()))
            .collect::<Vec<_>>();

        assert_eq!(
            active.ordered_window_ids.as_deref(),
            Some(&[12, 11, 13][..])
        );
        assert_eq!(
            ordered_window_ids_from_windows,
            vec![(12, 0), (11, 1), (13, 2)]
        );
    }

    #[test]
    fn matching_onscreen_window_descriptions_preserve_target_window_metadata() {
        let window_number_key = cg_window_number_key();
        let window_owner_pid_key = cg_window_owner_pid_key();
        let window_name_key = cg_window_name_key();
        let window_layer_key = cg_window_layer_key();
        let window_bounds_key = cg_window_bounds_key();
        let x_key = cf_string("X").unwrap();
        let y_key = cf_string("Y").unwrap();
        let width_key = cf_string("Width").unwrap();
        let height_key = cf_string("Height").unwrap();
        let id_11 = cf_number_from_u64(11).unwrap();
        let pid_101 = cf_number_from_u64(101).unwrap();
        let level_5 = cf_number_from_u64(5).unwrap();
        let x_10 = cf_number_from_u64(10).unwrap();
        let y_20 = cf_number_from_u64(20).unwrap();
        let width_300 = cf_number_from_u64(300).unwrap();
        let height_400 = cf_number_from_u64(400).unwrap();
        let title_alpha = cf_string("alpha").unwrap();
        let id_22 = cf_number_from_u64(22).unwrap();
        let pid_202 = cf_number_from_u64(202).unwrap();
        let level_7 = cf_number_from_u64(7).unwrap();
        let title_beta = cf_string("beta").unwrap();
        let first_bounds = cf_test_dictionary(&[
            (x_key.as_type_ref(), x_10.as_type_ref()),
            (y_key.as_type_ref(), y_20.as_type_ref()),
            (width_key.as_type_ref(), width_300.as_type_ref()),
            (height_key.as_type_ref(), height_400.as_type_ref()),
        ]);
        let first_window = cf_test_dictionary(&[
            (window_number_key as CFTypeRef, id_11.as_type_ref()),
            (window_owner_pid_key as CFTypeRef, pid_101.as_type_ref()),
            (window_name_key as CFTypeRef, title_alpha.as_type_ref()),
            (window_layer_key as CFTypeRef, level_5.as_type_ref()),
            (window_bounds_key as CFTypeRef, first_bounds.as_type_ref()),
        ]);
        let second_window = cf_test_dictionary(&[
            (window_number_key as CFTypeRef, id_22.as_type_ref()),
            (window_owner_pid_key as CFTypeRef, pid_202.as_type_ref()),
            (window_name_key as CFTypeRef, title_beta.as_type_ref()),
            (window_layer_key as CFTypeRef, level_7.as_type_ref()),
        ]);
        let onscreen_descriptions =
            cf_test_array(&[first_window.as_type_ref(), second_window.as_type_ref()]);

        let filtered = filter_window_descriptions_raw(
            onscreen_descriptions.as_type_ref() as CFArrayRef,
            &[11],
        )
        .unwrap();
        let parsed = parse_window_descriptions(
            filtered.as_type_ref() as CFArrayRef,
            &HashMap::from([(11, 0usize)]),
        )
        .unwrap();

        assert_eq!(
            parsed,
            vec![
                raw_window(11)
                    .with_pid(101)
                    .with_title("alpha")
                    .with_level(5)
                    .with_visible_index(0)
                    .with_frame(Rect {
                        x: 10,
                        y: 20,
                        w: 300,
                        h: 400,
                    }),
            ]
        );
    }

    #[test]
    fn parse_raw_space_record_ignores_non_dictionary_tile_space_entries() {
        let managed_space_id_key = cf_string("ManagedSpaceID").unwrap();
        let space_type_key = cf_string("type").unwrap();
        let tile_layout_manager_key = cf_string("TileLayoutManager").unwrap();
        let tile_spaces_key = cf_string("TileSpaces").unwrap();
        let id64_key = cf_string("id64").unwrap();
        let managed_space_id = cf_number_from_u64(7).unwrap();
        let space_type = cf_number_from_u64(DESKTOP_SPACE_TYPE as u64).unwrap();
        let split_left_id = cf_number_from_u64(11).unwrap();
        let split_right_id = cf_number_from_u64(12).unwrap();
        let non_dictionary_entry = cf_number_from_u64(999).unwrap();

        let tile_space_with_managed_space_id = cf_test_dictionary(&[(
            managed_space_id_key.as_type_ref(),
            split_left_id.as_type_ref(),
        )]);
        let tile_space_with_id64 =
            cf_test_dictionary(&[(id64_key.as_type_ref(), split_right_id.as_type_ref())]);
        let tile_spaces = cf_test_array(&[
            tile_space_with_managed_space_id.as_type_ref(),
            non_dictionary_entry.as_type_ref(),
            tile_space_with_id64.as_type_ref(),
        ]);
        let tile_layout_manager =
            cf_test_dictionary(&[(tile_spaces_key.as_type_ref(), tile_spaces.as_type_ref())]);
        let raw_space = cf_test_dictionary(&[
            (
                managed_space_id_key.as_type_ref(),
                managed_space_id.as_type_ref(),
            ),
            (space_type_key.as_type_ref(), space_type.as_type_ref()),
            (
                tile_layout_manager_key.as_type_ref(),
                tile_layout_manager.as_type_ref(),
            ),
        ]);

        let parsed = parse_raw_space_record(raw_space.as_type_ref() as CFDictionaryRef, 3).unwrap();

        assert_eq!(parsed.managed_space_id, 7);
        assert_eq!(parsed.display_index, 3);
        assert_eq!(parsed.tile_spaces, vec![11, 12]);
        assert!(parsed.has_tile_layout_manager);
    }

    #[test]
    fn parse_managed_spaces_preserves_display_grouping() {
        let display_identifier_key = cf_string("Display Identifier").unwrap();
        let spaces_key = cf_string("Spaces").unwrap();
        let managed_space_id_key = cf_string("ManagedSpaceID").unwrap();
        let space_type_key = cf_string("type").unwrap();
        let space_type = cf_number_from_u64(DESKTOP_SPACE_TYPE as u64).unwrap();

        let display0_space = cf_test_dictionary(&[
            (
                managed_space_id_key.as_type_ref(),
                cf_number_from_u64(1).unwrap().as_type_ref(),
            ),
            (space_type_key.as_type_ref(), space_type.as_type_ref()),
        ]);
        let display1_space = cf_test_dictionary(&[
            (
                managed_space_id_key.as_type_ref(),
                cf_number_from_u64(9).unwrap().as_type_ref(),
            ),
            (space_type_key.as_type_ref(), space_type.as_type_ref()),
        ]);
        let display0 = cf_test_dictionary(&[
            (
                display_identifier_key.as_type_ref(),
                cf_string("display-0").unwrap().as_type_ref(),
            ),
            (
                spaces_key.as_type_ref(),
                cf_test_array(&[display0_space.as_type_ref()]).as_type_ref(),
            ),
        ]);
        let display1 = cf_test_dictionary(&[
            (
                display_identifier_key.as_type_ref(),
                cf_string("display-1").unwrap().as_type_ref(),
            ),
            (
                spaces_key.as_type_ref(),
                cf_test_array(&[display1_space.as_type_ref()]).as_type_ref(),
            ),
        ]);
        let payload = cf_test_array(&[display0.as_type_ref(), display1.as_type_ref()]);

        let parsed = parse_managed_spaces(payload.as_type_ref() as CFArrayRef).unwrap();

        assert_eq!(parsed[0].managed_space_id, 1);
        assert_eq!(parsed[0].display_index, 0);
        assert_eq!(parsed[1].managed_space_id, 9);
        assert_eq!(parsed[1].display_index, 1);
    }
}
