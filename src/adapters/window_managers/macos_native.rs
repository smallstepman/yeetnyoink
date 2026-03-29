use crate::config::{self, WmBackend};
use crate::engine::runtime::{self, CommandContext, ProcessId};
use crate::engine::topology::{DirectedRect, Direction, Rect};
use crate::engine::wm::{
    validate_declared_capabilities, CapabilitySupport, ConfiguredWindowManager,
    DirectionalCapability, FloatingFocusMode, FocusedAppRecord, FocusedWindowRecord,
    PrimitiveWindowManagerCapabilities, ResizeIntent, WindowManagerCapabilities,
    WindowManagerCapabilityDescriptor, WindowManagerFeatures, WindowManagerSession,
    WindowManagerSpec, WindowRecord,
};
use crate::logging;
use anyhow::{bail, Context};

use macos_window_manager::{
    ActiveSpaceFocusTargetHint, MacosNativeApi, MacosNativeConnectError, MacosNativeOperationError,
    MacosNativeProbeError, MissionControlHotkey, MissionControlModifiers, NativeBackendOptions,
    NativeBounds, NativeDesktopSnapshot, NativeDiagnostics, NativeDirection, NativeWindowSnapshot,
    RealNativeApi, SpaceKind,
};

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
    use super::*;
    use crate::engine::topology::{select_closest_in_direction_with_strategy, Rect};
    use macos_window_manager::{
        NativeSpaceSnapshot, RawSpaceRecord, RawTopologySnapshot, RawWindow, WindowSnapshot,
    };
    use std::{
        collections::{HashMap, HashSet, VecDeque},
        sync::{Arc, Mutex},
    };

    const DESKTOP_SPACE_TYPE: i32 = 0;
    const FULLSCREEN_SPACE_TYPE: i32 = 4;

    fn compare_active_windows(left: &RawWindow, right: &RawWindow) -> std::cmp::Ordering {
        match (left.visible_index, right.visible_index) {
            (Some(left_index), Some(right_index)) => left_index.cmp(&right_index),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
        .then_with(|| right.level.cmp(&left.level))
        .then_with(|| left.id.cmp(&right.id))
    }

    fn order_active_space_windows(windows: &[RawWindow]) -> Vec<RawWindow> {
        let mut ordered = windows.to_vec();
        ordered.sort_by(compare_active_windows);
        ordered
    }

    fn classify_space(space: &RawSpaceRecord) -> SpaceKind {
        if space.stage_manager_managed {
            SpaceKind::StageManagerOpaque
        } else if space.has_tile_layout_manager || !space.tile_spaces.is_empty() {
            SpaceKind::SplitView
        } else if space.space_type == FULLSCREEN_SPACE_TYPE {
            SpaceKind::Fullscreen
        } else if space.space_type == DESKTOP_SPACE_TYPE {
            SpaceKind::Desktop
        } else {
            SpaceKind::System
        }
    }

    fn native_desktop_snapshot_from_topology(
        topology: &RawTopologySnapshot,
    ) -> NativeDesktopSnapshot {
        let spaces = topology
            .spaces
            .iter()
            .map(|space| NativeSpaceSnapshot {
                id: space.managed_space_id,
                display_index: space.display_index,
                active: topology.active_space_ids.contains(&space.managed_space_id),
                kind: classify_space(space),
            })
            .collect();
        let mut windows = Vec::new();

        for space in &topology.spaces {
            if topology.active_space_ids.contains(&space.managed_space_id) {
                windows.extend(
                    order_active_space_windows(
                        topology
                            .active_space_windows
                            .get(&space.managed_space_id)
                            .map(Vec::as_slice)
                            .unwrap_or(&[]),
                    )
                    .into_iter()
                    .enumerate()
                    .map(|(index, window)| NativeWindowSnapshot {
                        id: window.id,
                        pid: window.pid,
                        app_id: window.app_id,
                        title: window.title,
                        bounds: window.frame,
                        level: window.level,
                        space_id: space.managed_space_id,
                        order_index: Some(index),
                    }),
                );
            } else {
                windows.extend(
                    topology
                        .inactive_space_window_ids
                        .get(&space.managed_space_id)
                        .map(Vec::as_slice)
                        .unwrap_or(&[])
                        .iter()
                        .copied()
                        .map(|window_id| NativeWindowSnapshot {
                            id: window_id,
                            pid: None,
                            app_id: None,
                            title: None,
                            bounds: None,
                            level: 0,
                            space_id: space.managed_space_id,
                            order_index: None,
                        }),
                );
            }
        }

        NativeDesktopSnapshot {
            spaces,
            active_space_ids: topology.active_space_ids.clone(),
            windows,
            focused_window_id: topology.focused_window_id,
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

    fn implementation_source() -> &'static str {
        let source = include_str!("macos_native.rs");
        source
            .rsplit_once("#[cfg(test)]\nmod tests {")
            .map(|(implementation, _)| implementation)
            .expect("macos_native.rs source should include a test module")
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
                gated_declaration.is_some_and(|line| line.ends_with("mod tests {")),
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
    fn source_adapter_tests_do_not_import_macos_test_support() {
        let source = include_str!("macos_native.rs");
        assert!(!source.lines().any(|line| {
            line.trim_start()
                .starts_with("use super::macos_window_manager_test_support::")
        }));
    }

    #[test]
    fn source_adapter_keeps_backend_test_markers_out_of_adapter_file() {
        let source = include_str!("macos_native.rs");

        for marker in [
            concat!("include!(\"", "macos_native_backend_tests.rs", "\")"),
            concat!("fn back", "end_"),
            concat!("struct Sequenced", "SamePidAxFallbackApi"),
            concat!(
                "fn focus_direction_uses_planning_",
                "snapshot_for_same_pid_ax_fallback"
            ),
        ] {
            assert!(
                !source.contains(marker),
                "adapter source should not keep backend test marker `{marker}`"
            );
        }
    }

    #[test]
    fn source_adapter_tests_do_not_redeclare_backend_helper_blocks() {
        let source = include_str!("macos_native.rs");

        for marker in [
            concat!("const REQUIRED_PRIVATE_", "SYMBOLS:"),
            concat!("const SPACE_SWITCH_SETTLE_", "TIMEOUT:"),
            concat!("const SPACE_SWITCH_POLL_", "INTERVAL:"),
            concat!("const SPACE_SWITCH_STABLE_TARGET_", "POLLS:"),
            concat!("fn switch_space_in_", "topology("),
            concat!("fn wait_for_space_", "presentation("),
            concat!("fn validate_environment_with_", "api<"),
            concat!("fn focused_window_from_", "topology("),
            concat!("fn space_transition_window_", "ids("),
            concat!("fn ensure_supported_target_", "space("),
            concat!("fn back", "end_"),
            concat!("struct FakeNative", "Api"),
            concat!("struct PostSwitchSelectionDrift", "Api"),
            concat!("struct SamePidAxFallback", "Api"),
            concat!("struct SwitchThenFocus", "Api"),
            concat!("struct AdjacentHotkeyOnly", "Api"),
            concat!("struct EmptySpaceSkippingAdjacentHotkey", "Api"),
            concat!("struct SpaceSettling", "Api"),
            concat!("struct SpacePresentation", "Api"),
        ] {
            assert!(
                !source.contains(marker),
                "adapter test module should not redeclare backend helper block marker `{marker}`"
            );
        }
    }

    #[test]
    fn source_adapter_has_no_macos_test_support_module() {
        let implementation = implementation_source();
        assert!(!implementation.contains("mod macos_window_manager_test_support;"));
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
    fn source_adapter_root_imports_do_not_include_native_space_snapshot() {
        let implementation = implementation_source();
        let import_start = implementation
            .find("use macos_window_manager::{")
            .expect("adapter should import the shared macos window manager contract");
        let import_end = implementation[import_start..]
            .find("};")
            .map(|idx| import_start + idx)
            .expect("root macos_window_manager import should close");
        let import_block = &implementation[import_start..import_end];

        assert!(
            !import_block.contains("NativeSpaceSnapshot"),
            "production adapter root import should keep NativeSpaceSnapshot scoped to tests"
        );
    }

    #[test]
    fn source_adapter_does_not_define_outer_space_transition_window_ids() {
        let implementation = implementation_source();

        assert!(
            !implementation.contains("fn outer_space_transition_window_ids("),
            "production adapter should not define outer_space_transition_window_ids once it only serves test support"
        );
    }
    #[test]
    fn topology_snapshot_uses_api_focused_window_id() {
        let topology = FocusedIdTopologyApi.topology_snapshot().unwrap();

        assert_eq!(topology.focused_window_id, Some(11));
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
    fn focus_direction_uses_radial_center_outer_policy_with_native_snapshot() {
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
                    id: 10,
                    pid: Some(1010),
                    app_id: Some("com.example.source".to_string()),
                    title: Some("source".to_string()),
                    bounds: Some(NativeBounds {
                        x: 200,
                        y: 100,
                        width: 100,
                        height: 100,
                    }),
                    level: 0,
                    space_id: 1,
                    order_index: Some(0),
                },
                NativeWindowSnapshot {
                    id: 20,
                    pid: Some(2020),
                    app_id: Some("com.example.radial-target".to_string()),
                    title: Some("radial-target".to_string()),
                    bounds: Some(NativeBounds {
                        x: 40,
                        y: 80,
                        width: 60,
                        height: 60,
                    }),
                    level: 0,
                    space_id: 1,
                    order_index: Some(1),
                },
                NativeWindowSnapshot {
                    id: 30,
                    pid: Some(3030),
                    app_id: Some("com.example.cross-edge-target".to_string()),
                    title: Some("cross-edge-target".to_string()),
                    bounds: Some(NativeBounds {
                        x: 90,
                        y: 150,
                        width: 130,
                        height: 130,
                    }),
                    level: 0,
                    space_id: 1,
                    order_index: Some(2),
                },
            ],
            focused_window_id: Some(10),
        });
        let recorded = api.clone();
        let mut adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        WindowManagerSession::focus_direction(&mut adapter, Direction::West).unwrap();

        assert_eq!(
            recorded.api_calls(),
            vec![
                NativeCall::DesktopSnapshot,
                NativeCall::FocusWindowWithPid(20, 2020)
            ]
        );
    }

    #[test]
    fn focus_direction_uses_cross_edge_gap_outer_policy_with_native_snapshot() {
        let _config = install_macos_native_focus_config("cross_edge_gap");
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
                    id: 10,
                    pid: Some(1010),
                    app_id: Some("com.example.source".to_string()),
                    title: Some("source".to_string()),
                    bounds: Some(NativeBounds {
                        x: 200,
                        y: 100,
                        width: 100,
                        height: 100,
                    }),
                    level: 0,
                    space_id: 1,
                    order_index: Some(0),
                },
                NativeWindowSnapshot {
                    id: 20,
                    pid: Some(2020),
                    app_id: Some("com.example.radial-target".to_string()),
                    title: Some("radial-target".to_string()),
                    bounds: Some(NativeBounds {
                        x: 40,
                        y: 80,
                        width: 60,
                        height: 60,
                    }),
                    level: 0,
                    space_id: 1,
                    order_index: Some(1),
                },
                NativeWindowSnapshot {
                    id: 30,
                    pid: Some(3030),
                    app_id: Some("com.example.cross-edge-target".to_string()),
                    title: Some("cross-edge-target".to_string()),
                    bounds: Some(NativeBounds {
                        x: 90,
                        y: 150,
                        width: 130,
                        height: 130,
                    }),
                    level: 0,
                    space_id: 1,
                    order_index: Some(2),
                },
            ],
            focused_window_id: Some(10),
        });
        let recorded = api.clone();
        let mut adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        WindowManagerSession::focus_direction(&mut adapter, Direction::West).unwrap();

        assert_eq!(
            recorded.api_calls(),
            vec![
                NativeCall::DesktopSnapshot,
                NativeCall::FocusWindowWithPid(30, 3030)
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
    fn focus_direction_remaps_splitview_target_to_same_app_peer_even_when_peer_ax_windows_are_empty_preflight(
    ) {
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
    fn focus_direction_remaps_refreshed_splitview_target_to_focusable_same_app_peer_when_direct_target_stays_stale(
    ) {
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

        WindowManagerSession::focus_window_by_id(&mut adapter, 77).unwrap();
        WindowManagerSession::move_direction(&mut adapter, Direction::East).unwrap();
        api.move_window_to_space(20, 1).unwrap();

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

}
