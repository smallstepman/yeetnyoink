use anyhow::{Context, bail};
use std::{
    collections::{HashMap, HashSet},
    ffi::{CStr, CString, c_char, c_int, c_void},
    fmt,
    ptr::{self, NonNull},
};

use crate::config::WmBackend;
use crate::engine::runtime::{self, CommandContext, ProcessId};
use crate::engine::topology::{DirectedRect, Direction, Rect, select_closest_in_direction};
use crate::engine::wm::{
    CapabilitySupport, ConfiguredWindowManager, DirectionalCapability, FocusedWindowRecord,
    PrimitiveWindowManagerCapabilities, ResizeIntent, WindowManagerCapabilities,
    WindowManagerCapabilityDescriptor, WindowManagerFeatures, WindowManagerSession,
    WindowManagerSpec, WindowRecord, validate_declared_capabilities,
};

type CFIndex = isize;
type Boolean = u8;
type CFTypeID = usize;
type CFNumberType = isize;
type CFStringEncoding = u32;
type CFTypeRef = *const c_void;
type CFArrayRef = *const c_void;
type CFDictionaryRef = *const c_void;
type CFNumberRef = *const c_void;
type CFStringRef = *const c_void;
type CFAllocatorRef = *const c_void;
type AXUIElementRef = *const c_void;
type AXValueType = u32;
type CGWindowID = u32;
type CGWindowListOption = u32;
type OSStatus = i32;
type CFArrayRetainCallBack = unsafe extern "C" fn(CFAllocatorRef, *const c_void) -> *const c_void;
type CFArrayReleaseCallBack = unsafe extern "C" fn(CFAllocatorRef, *const c_void);
type CFArrayCopyDescriptionCallBack = unsafe extern "C" fn(*const c_void) -> CFStringRef;
type CFArrayEqualCallBack = unsafe extern "C" fn(*const c_void, *const c_void) -> Boolean;

const K_CF_NUMBER_SINT64_TYPE: CFNumberType = 4;
const K_CF_STRING_ENCODING_UTF8: CFStringEncoding = 0x0800_0100;
const K_CG_NULL_WINDOW_ID: CGWindowID = 0;
const K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY: CGWindowListOption = 1 << 0;
const K_CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS: CGWindowListOption = 1 << 4;
const K_AX_VALUE_TYPE_CGPOINT: AXValueType = 1;
const K_AX_VALUE_TYPE_CGSIZE: AXValueType = 2;
const CPS_USER_GENERATED: u32 = 0x200;

#[repr(C)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
struct CGSize {
    width: f64,
    height: f64,
}

#[repr(C)]
struct CFArrayCallBacks {
    version: CFIndex,
    retain: Option<CFArrayRetainCallBack>,
    release: Option<CFArrayReleaseCallBack>,
    copy_description: Option<CFArrayCopyDescriptionCallBack>,
    equal: Option<CFArrayEqualCallBack>,
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFTypeArrayCallBacks: CFArrayCallBacks;

    fn CFGetTypeID(cf: CFTypeRef) -> CFTypeID;
    fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
    fn CFRelease(cf: CFTypeRef);
    fn CFArrayGetTypeID() -> CFTypeID;
    fn CFArrayCreate(
        allocator: CFAllocatorRef,
        values: *const *const c_void,
        num_values: CFIndex,
        callbacks: *const CFArrayCallBacks,
    ) -> CFArrayRef;
    fn CFArrayGetCount(the_array: CFArrayRef) -> CFIndex;
    fn CFArrayGetValueAtIndex(the_array: CFArrayRef, idx: CFIndex) -> *const c_void;
    fn CFDictionaryGetTypeID() -> CFTypeID;
    fn CFDictionaryGetValueIfPresent(
        dictionary: CFDictionaryRef,
        key: *const c_void,
        value: *mut *const c_void,
    ) -> Boolean;
    fn CFStringGetTypeID() -> CFTypeID;
    fn CFStringCreateWithCString(
        allocator: CFAllocatorRef,
        c_string: *const c_char,
        encoding: CFStringEncoding,
    ) -> CFStringRef;
    fn CFStringGetLength(the_string: CFStringRef) -> CFIndex;
    fn CFStringGetMaximumSizeForEncoding(length: CFIndex, encoding: CFStringEncoding) -> CFIndex;
    fn CFStringGetCString(
        the_string: CFStringRef,
        buffer: *mut c_char,
        buffer_size: CFIndex,
        encoding: CFStringEncoding,
    ) -> Boolean;
    fn CFNumberGetTypeID() -> CFTypeID;
    fn CFNumberCreate(
        allocator: CFAllocatorRef,
        number_type: CFNumberType,
        value: *const c_void,
    ) -> CFNumberRef;
    fn CFNumberGetValue(
        number: CFNumberRef,
        number_type: CFNumberType,
        value: *mut c_void,
    ) -> Boolean;
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXUIElementCreateApplication(pid: c_int) -> AXUIElementRef;
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> OSStatus;
    fn AXUIElementPerformAction(element: AXUIElementRef, action: CFStringRef) -> OSStatus;
    fn AXUIElementSetAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> OSStatus;
    fn AXValueCreate(value_type: AXValueType, value_ptr: *const c_void) -> CFTypeRef;
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    static kCGWindowNumber: CFStringRef;
    static kCGWindowOwnerPID: CFStringRef;
    static kCGWindowName: CFStringRef;
    static kCGWindowLayer: CFStringRef;
    static kCGWindowBounds: CFStringRef;

    fn CGWindowListCopyWindowInfo(
        option: CGWindowListOption,
        relative_to_window: CGWindowID,
    ) -> CFArrayRef;
    fn CGWindowListCreateDescriptionFromArray(window_array: CFArrayRef) -> CFArrayRef;
}

struct CfOwned {
    raw: NonNull<c_void>,
}

impl CfOwned {
    unsafe fn from_create_rule(raw: CFTypeRef) -> Option<Self> {
        NonNull::new(raw.cast_mut()).map(|raw| Self { raw })
    }

    fn as_type_ref(&self) -> CFTypeRef {
        self.raw.as_ptr() as CFTypeRef
    }
}

impl Drop for CfOwned {
    fn drop(&mut self) {
        unsafe {
            CFRelease(self.as_type_ref());
        }
    }
}

impl Clone for CfOwned {
    fn clone(&self) -> Self {
        unsafe {
            Self::from_create_rule(CFRetain(self.as_type_ref()))
                .expect("CFRetain should never return null")
        }
    }
}

fn focused_window_id_via_ax<App, Window, FocusedApplication, FocusedWindow, WindowId>(
    mut focused_application: FocusedApplication,
    mut focused_window: FocusedWindow,
    mut window_id: WindowId,
) -> Result<Option<u64>, MacosNativeProbeError>
where
    FocusedApplication: FnMut() -> Result<Option<App>, MacosNativeProbeError>,
    FocusedWindow: FnMut(&App) -> Result<Option<Window>, MacosNativeProbeError>,
    WindowId: FnMut(&Window) -> Result<u64, MacosNativeProbeError>,
{
    let Some(application) = focused_application()? else {
        return Ok(None);
    };
    let Some(window) = focused_window(&application)? else {
        return Ok(None);
    };
    window_id(&window).map(Some)
}

fn focus_window_via_process_and_raise<WindowPid, ProcessSerial, FrontProcessWindow, RaiseWindow>(
    window_id: u64,
    mut window_pid: WindowPid,
    mut process_serial_number: ProcessSerial,
    mut front_process_window: FrontProcessWindow,
    mut raise_window: RaiseWindow,
) -> Result<(), MacosNativeOperationError>
where
    WindowPid: FnMut(u64) -> Result<u32, MacosNativeOperationError>,
    ProcessSerial: FnMut(u32) -> Result<ProcessSerialNumber, MacosNativeOperationError>,
    FrontProcessWindow: FnMut(&ProcessSerialNumber, u64) -> Result<(), MacosNativeOperationError>,
    RaiseWindow: FnMut(u64, u32) -> Result<(), MacosNativeOperationError>,
{
    let pid = window_pid(window_id)?;
    let psn = process_serial_number(pid)?;
    front_process_window(&psn, window_id)?;
    raise_window(window_id, pid)
}

pub(crate) struct MacosNativeAdapter<A = RealNativeApi> {
    ctx: MacosNativeContext<A>,
}

pub(crate) struct MacosNativeSpec;

pub(crate) static MACOS_NATIVE_SPEC: MacosNativeSpec = MacosNativeSpec;

impl WindowManagerSpec for MacosNativeSpec {
    fn backend(&self) -> WmBackend {
        WmBackend::MacosNative
    }

    fn name(&self) -> &'static str {
        MacosNativeAdapter::<RealNativeApi>::NAME
    }

    fn connect(&self) -> anyhow::Result<ConfiguredWindowManager> {
        ConfiguredWindowManager::try_new(
            Box::new(MacosNativeAdapter::connect()?),
            WindowManagerFeatures::default(),
        )
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

    fn focused_window_record(&self) -> anyhow::Result<FocusedWindowRecord> {
        let focused = self.ctx.focused_window().map_err(map_probe_error)?;
        Ok(FocusedWindowRecord {
            id: focused.id,
            app_id: focused.app_id,
            title: focused.title,
            pid: focused.pid.and_then(ProcessId::new),
            original_tile_index: focused.order_index.unwrap_or(0),
        })
    }

    fn windows_vec(&self) -> anyhow::Result<Vec<WindowRecord>> {
        let topology = self.ctx.topology_snapshot().map_err(map_probe_error)?;
        let focused_window_id = focused_window_from_topology(&topology)
            .ok()
            .map(|window| window.id);

        Ok(window_snapshots_from_topology(&topology)
            .into_iter()
            .map(|window| WindowRecord {
                id: window.id,
                app_id: window.app_id,
                title: window.title,
                pid: window.pid.and_then(ProcessId::new),
                is_focused: focused_window_id == Some(window.id),
                original_tile_index: window.order_index.unwrap_or(0),
            })
            .collect())
    }

    fn focus_direction_inner(&self, direction: Direction) -> anyhow::Result<()> {
        let topology = self.ctx.topology_snapshot().map_err(map_probe_error)?;
        let focused = focused_window_from_topology(&topology).map_err(map_probe_error)?;
        let rects = display_index_for_space(&topology, focused.space_id)
            .map(|display_index| active_directed_rects_for_display(&topology, display_index))
            .filter(|rects| !rects.is_empty())
            .unwrap_or_else(|| active_directed_rects(&topology));
        let Some(target_id) = select_closest_in_direction(&rects, focused.id, direction) else {
            if let Some(target_space_id) =
                adjacent_space_in_direction(&topology, focused.space_id, direction)
            {
                return self
                    .ctx
                    .switch_space(target_space_id)
                    .map_err(map_operation_error);
            }
            anyhow::bail!("macos_native: no window to focus {direction}");
        };
        self.ctx
            .focus_window(target_id)
            .map_err(map_operation_error)
    }

    fn move_direction_inner(&self, direction: Direction) -> anyhow::Result<()> {
        let topology = self.ctx.topology_snapshot().map_err(map_probe_error)?;
        let focused = focused_window_from_topology(&topology).map_err(map_probe_error)?;
        let rects = active_directed_rects(&topology);
        let target_id = select_closest_in_direction(&rects, focused.id, direction)
            .with_context(|| format!("macos_native: no window to move {direction}"))?;
        let source = active_window_by_id(&topology, focused.id)
            .and_then(|window| window.frame)
            .with_context(|| format!("macos_native: focused window {} has no frame", focused.id))?;
        let target = active_window_by_id(&topology, target_id)
            .and_then(|window| window.frame)
            .with_context(|| format!("macos_native: target window {target_id} has no frame"))?;
        self.ctx
            .swap_window_frames(focused.id, source, target_id, target)
            .map_err(map_operation_error)
    }
}

impl MacosNativeAdapter<RealNativeApi> {
    pub fn connect() -> anyhow::Result<Self> {
        validate_declared_capabilities::<Self>()?;
        Self::connect_with_api(RealNativeApi::new()).map_err(anyhow::Error::new)
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
        self.focused_window_record()
    }

    fn windows(&mut self) -> anyhow::Result<Vec<WindowRecord>> {
        self.windows_vec()
    }

    fn focus_direction(&mut self, direction: Direction) -> anyhow::Result<()> {
        self.focus_direction_inner(direction)
    }

    fn move_direction(&mut self, direction: Direction) -> anyhow::Result<()> {
        self.move_direction_inner(direction)
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
        self.ctx.focus_window(id).map_err(map_operation_error)
    }

    fn close_window_by_id(&mut self, id: u64) -> anyhow::Result<()> {
        bail!("macos_native: close_window_by_id({id}) is not implemented")
    }
}

#[allow(dead_code)]
const REQUIRED_PRIVATE_SYMBOLS: &[&str] = &[
    "SLSMainConnectionID",
    "SLSCopyManagedDisplaySpaces",
    "SLSManagedDisplayGetCurrentSpace",
    "SLSManagedDisplaySetCurrentSpace",
    "SLSCopyManagedDisplayForSpace",
    "SLSCopyWindowsWithOptionsAndTags",
    "SLSMoveWindowsToManagedSpace",
    "AXIsProcessTrusted",
    "_AXUIElementGetWindow",
    "_SLPSSetFrontProcessWithOptions",
    "GetProcessForPID",
];

#[allow(dead_code)]
const SKYLIGHT_FRAMEWORK_PATH: &CStr =
    c"/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight";
#[allow(dead_code)]
const HISERVICES_FRAMEWORK_PATH: &CStr =
    c"/System/Library/Frameworks/ApplicationServices.framework/Frameworks/HIServices.framework/HIServices";
#[allow(dead_code)]
const RTLD_LAZY: c_int = 0x1;

#[allow(dead_code)]
type SlsMainConnectionIdFn = unsafe extern "C" fn() -> u32;
#[allow(dead_code)]
type AxIsProcessTrustedFn = unsafe extern "C" fn() -> u8;
#[allow(dead_code)]
type SlsCopyManagedDisplaySpacesFn = unsafe extern "C" fn(u32) -> CFArrayRef;
#[allow(dead_code)]
type SlsManagedDisplayGetCurrentSpaceFn = unsafe extern "C" fn(u32, CFStringRef) -> u64;
#[allow(dead_code)]
type SlsManagedDisplaySetCurrentSpaceFn = unsafe extern "C" fn(u32, CFStringRef, u64);
#[allow(dead_code)]
type SlsCopyManagedDisplayForSpaceFn = unsafe extern "C" fn(u32, u64) -> CFStringRef;
#[allow(dead_code)]
type SlsCopyWindowsWithOptionsAndTagsFn =
    unsafe extern "C" fn(u32, u32, CFArrayRef, i32, *mut i64, *mut i64) -> CFArrayRef;
#[allow(dead_code)]
type SlsMoveWindowsToManagedSpaceFn = unsafe extern "C" fn(u32, CFArrayRef, u64);
#[allow(dead_code)]
type SlpsSetFrontProcessWithOptionsFn =
    unsafe extern "C" fn(*const ProcessSerialNumber, CGWindowID, u32) -> OSStatus;
#[allow(dead_code)]
type GetProcessForPidFn = unsafe extern "C" fn(c_int, *mut ProcessSerialNumber) -> OSStatus;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ProcessSerialNumber {
    high_long_of_psn: u32,
    low_long_of_psn: u32,
}

#[allow(dead_code)]
unsafe extern "C" {
    fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MacosNativeConnectError {
    MissingRequiredSymbol(&'static str),
    MissingAccessibilityPermission,
    MissingTopologyPrecondition(&'static str),
}

impl fmt::Display for MacosNativeConnectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingRequiredSymbol(symbol) => {
                write!(f, "required macOS private symbol is unavailable: {symbol}")
            }
            Self::MissingAccessibilityPermission => {
                f.write_str("Accessibility permission is required for macOS native support")
            }
            Self::MissingTopologyPrecondition(precondition) => {
                write!(
                    f,
                    "macOS native topology precondition is unavailable: {precondition}"
                )
            }
        }
    }
}

impl std::error::Error for MacosNativeConnectError {}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MacosNativeProbeError {
    MissingTopology(&'static str),
    MissingFocusedWindow,
}

impl fmt::Display for MacosNativeProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingTopology(query) => {
                write!(f, "macOS native topology query is unavailable: {query}")
            }
            Self::MissingFocusedWindow => {
                f.write_str("no focused window was found for any active Space")
            }
        }
    }
}

impl std::error::Error for MacosNativeProbeError {}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MacosNativeOperationError {
    Probe(MacosNativeProbeError),
    MissingSpace(u64),
    MissingWindow(u64),
    MissingWindowPid(u64),
    UnsupportedStageManagerSpace(u64),
    CallFailed(&'static str),
}

impl fmt::Display for MacosNativeOperationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Probe(err) => err.fmt(f),
            Self::MissingSpace(space_id) => {
                write!(
                    f,
                    "macOS native space {space_id} was not found in the current topology"
                )
            }
            Self::MissingWindow(window_id) => {
                write!(
                    f,
                    "macOS native window {window_id} was not found in the current topology"
                )
            }
            Self::MissingWindowPid(window_id) => {
                write!(
                    f,
                    "macOS native window {window_id} does not expose an owner pid"
                )
            }
            Self::UnsupportedStageManagerSpace(space_id) => {
                write!(
                    f,
                    "macOS native Stage Manager space {space_id} is intentionally unsupported"
                )
            }
            Self::CallFailed(call) => write!(f, "macOS native operation failed: {call}"),
        }
    }
}

impl std::error::Error for MacosNativeOperationError {}

impl From<MacosNativeProbeError> for MacosNativeOperationError {
    fn from(err: MacosNativeProbeError) -> Self {
        Self::Probe(err)
    }
}

#[allow(dead_code)]
pub(crate) trait MacosNativeApi {
    fn has_symbol(&self, symbol: &'static str) -> bool;
    fn ax_is_trusted(&self) -> bool;
    fn minimal_topology_ready(&self) -> bool;
    fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError>;
    fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError>;
    fn active_space_windows(&self, space_id: u64) -> Result<Vec<RawWindow>, MacosNativeProbeError>;
    fn inactive_space_window_ids(&self) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError>;
    fn focused_window_id(&self) -> Result<Option<u64>, MacosNativeProbeError> {
        Ok(None)
    }
    fn switch_space(&self, space_id: u64) -> Result<(), MacosNativeOperationError>;
    fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError>;
    fn move_window_to_space(
        &self,
        window_id: u64,
        space_id: u64,
    ) -> Result<(), MacosNativeOperationError>;
    fn swap_window_frames(
        &self,
        source_window_id: u64,
        source_frame: Rect,
        target_window_id: u64,
        target_frame: Rect,
    ) -> Result<(), MacosNativeOperationError>;

    fn topology_snapshot(&self) -> Result<RawTopologySnapshot, MacosNativeProbeError> {
        let spaces = self.managed_spaces()?;
        let active_space_ids = self.active_space_ids()?;
        let active_space_windows = active_space_ids
            .iter()
            .copied()
            .map(|space_id| {
                self.active_space_windows(space_id)
                    .map(|windows| (space_id, windows))
            })
            .collect::<Result<HashMap<_, _>, _>>()?;
        let inactive_space_window_ids = self.inactive_space_window_ids()?;

        Ok(RawTopologySnapshot {
            spaces,
            active_space_ids,
            active_space_windows,
            inactive_space_window_ids,
            focused_window_id: self.focused_window_id()?,
        })
    }
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug)]
pub(crate) struct MacosNativeContext<A = RealNativeApi> {
    #[allow(dead_code)]
    api: A,
}

impl<A> MacosNativeContext<A>
where
    A: MacosNativeApi,
{
    #[allow(dead_code)]
    pub(crate) fn connect_with_api(api: A) -> Result<Self, MacosNativeConnectError> {
        for symbol in REQUIRED_PRIVATE_SYMBOLS {
            if !api.has_symbol(symbol) {
                return Err(MacosNativeConnectError::MissingRequiredSymbol(symbol));
            }
        }

        if !api.ax_is_trusted() {
            return Err(MacosNativeConnectError::MissingAccessibilityPermission);
        }

        if !api.minimal_topology_ready() {
            return Err(MacosNativeConnectError::MissingTopologyPrecondition(
                "main SkyLight connection",
            ));
        }

        Ok(Self { api })
    }

    #[allow(dead_code)]
    pub(crate) fn spaces(&self) -> Result<Vec<SpaceSnapshot>, MacosNativeProbeError> {
        let topology = self.topology_snapshot()?;
        Ok(space_snapshots_from_topology(&topology))
    }

    #[allow(dead_code)]
    pub(crate) fn focused_window(&self) -> Result<WindowSnapshot, MacosNativeProbeError> {
        let topology = self.topology_snapshot()?;
        focused_window_from_topology(&topology)
    }

    #[allow(dead_code)]
    pub(crate) fn switch_space(&self, space_id: u64) -> Result<(), MacosNativeOperationError> {
        let topology = self.topology_snapshot()?;
        ensure_supported_target_space(&topology, space_id)?;

        if topology.active_space_ids.contains(&space_id) {
            return Ok(());
        }

        self.api.switch_space(space_id)
    }

    #[allow(dead_code)]
    pub(crate) fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
        let topology = self.topology_snapshot()?;
        let target_space_id = space_id_for_window(&topology, window_id)
            .ok_or(MacosNativeOperationError::MissingWindow(window_id))?;
        ensure_supported_target_space(&topology, target_space_id)?;

        if !topology.active_space_ids.contains(&target_space_id) {
            self.api.switch_space(target_space_id)?;
        }

        self.api.focus_window(window_id)
    }

    #[allow(dead_code)]
    pub(crate) fn move_window_to_space(
        &self,
        window_id: u64,
        space_id: u64,
    ) -> Result<(), MacosNativeOperationError> {
        let topology = self.topology_snapshot()?;
        ensure_supported_target_space(&topology, space_id)?;
        if !topology_contains_window(&topology, window_id) {
            return Err(MacosNativeOperationError::MissingWindow(window_id));
        }
        self.api.move_window_to_space(window_id, space_id)
    }

    pub(crate) fn swap_window_frames(
        &self,
        source_window_id: u64,
        source_frame: Rect,
        target_window_id: u64,
        target_frame: Rect,
    ) -> Result<(), MacosNativeOperationError> {
        self.api.swap_window_frames(
            source_window_id,
            source_frame,
            target_window_id,
            target_frame,
        )
    }

    fn topology_snapshot(&self) -> Result<RawTopologySnapshot, MacosNativeProbeError> {
        self.api.topology_snapshot()
    }
}

impl MacosNativeContext<RealNativeApi> {
    #[allow(dead_code)]
    pub(crate) fn connect() -> Result<Self, MacosNativeConnectError> {
        Self::connect_with_api(RealNativeApi::new())
    }
}

#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct RealNativeApi {
    skylight: Option<DylibHandle>,
    hiservices: Option<DylibHandle>,
}

impl RealNativeApi {
    #[allow(dead_code)]
    fn new() -> Self {
        Self {
            skylight: DylibHandle::open(SKYLIGHT_FRAMEWORK_PATH),
            hiservices: DylibHandle::open(HISERVICES_FRAMEWORK_PATH),
        }
    }

    #[allow(dead_code)]
    fn resolve_symbol(&self, symbol: &'static str) -> Option<*mut c_void> {
        let symbol = CString::new(symbol).expect("required symbol names should not contain NULs");

        self.skylight
            .as_ref()
            .and_then(|handle| handle.resolve(symbol.as_c_str()))
            .or_else(|| {
                self.hiservices
                    .as_ref()
                    .and_then(|handle| handle.resolve(symbol.as_c_str()))
            })
    }

    fn main_connection_id(&self) -> Result<u32, MacosNativeProbeError> {
        let Some(symbol) = self.resolve_symbol("SLSMainConnectionID") else {
            return Err(MacosNativeProbeError::MissingTopology(
                "SLSMainConnectionID",
            ));
        };

        let main_connection_id: SlsMainConnectionIdFn = unsafe { std::mem::transmute(symbol) };
        let connection_id = unsafe { main_connection_id() };

        (connection_id != 0)
            .then_some(connection_id)
            .ok_or(MacosNativeProbeError::MissingTopology(
                "SLSMainConnectionID",
            ))
    }

    fn copy_managed_display_spaces_raw(&self) -> Result<CfOwned, MacosNativeProbeError> {
        let Some(symbol) = self.resolve_symbol("SLSCopyManagedDisplaySpaces") else {
            return Err(MacosNativeProbeError::MissingTopology(
                "SLSCopyManagedDisplaySpaces",
            ));
        };

        let copy_managed_display_spaces: SlsCopyManagedDisplaySpacesFn =
            unsafe { std::mem::transmute(symbol) };
        let connection_id = self.main_connection_id()?;
        let payload =
            unsafe { CfOwned::from_create_rule(copy_managed_display_spaces(connection_id)) }
                .ok_or(MacosNativeProbeError::MissingTopology(
                    "SLSCopyManagedDisplaySpaces",
                ))?;

        Ok(payload)
    }

    fn current_space_for_display(
        &self,
        display_identifier: &str,
    ) -> Result<u64, MacosNativeProbeError> {
        let Some(symbol) = self.resolve_symbol("SLSManagedDisplayGetCurrentSpace") else {
            return Err(MacosNativeProbeError::MissingTopology(
                "SLSManagedDisplayGetCurrentSpace",
            ));
        };

        let current_space_for_display: SlsManagedDisplayGetCurrentSpaceFn =
            unsafe { std::mem::transmute(symbol) };
        let connection_id = self.main_connection_id()?;
        let display_identifier = cf_string(display_identifier)?;
        let space_id =
            unsafe { current_space_for_display(connection_id, display_identifier.as_type_ref()) };

        (space_id != 0)
            .then_some(space_id)
            .ok_or(MacosNativeProbeError::MissingTopology(
                "SLSManagedDisplayGetCurrentSpace",
            ))
    }

    fn copy_windows_for_space_raw(&self, space_id: u64) -> Result<CfOwned, MacosNativeProbeError> {
        let Some(symbol) = self.resolve_symbol("SLSCopyWindowsWithOptionsAndTags") else {
            return Err(MacosNativeProbeError::MissingTopology(
                "SLSCopyWindowsWithOptionsAndTags",
            ));
        };

        let copy_windows_with_options_and_tags: SlsCopyWindowsWithOptionsAndTagsFn =
            unsafe { std::mem::transmute(symbol) };
        let connection_id = self.main_connection_id()?;
        let space_number = cf_number_from_u64(space_id)?;
        let values = [space_number.as_type_ref()];
        let space_list = unsafe {
            CfOwned::from_create_rule(CFArrayCreate(
                ptr::null(),
                values.as_ptr(),
                values.len() as CFIndex,
                &kCFTypeArrayCallBacks,
            ))
        }
        .ok_or(MacosNativeProbeError::MissingTopology(
            "SLSCopyWindowsWithOptionsAndTags",
        ))?;
        let mut set_tags = 0i64;
        let mut clear_tags = 0i64;
        let payload = unsafe {
            copy_windows_with_options_and_tags(
                connection_id,
                0,
                space_list.as_type_ref() as CFArrayRef,
                0x2,
                &mut set_tags,
                &mut clear_tags,
            )
        };
        let payload = unsafe { CfOwned::from_create_rule(payload) }.ok_or(
            MacosNativeProbeError::MissingTopology("SLSCopyWindowsWithOptionsAndTags"),
        )?;

        Ok(payload)
    }

    fn copy_system_wide_ax_element(&self) -> Result<CfOwned, MacosNativeProbeError> {
        unsafe { CfOwned::from_create_rule(AXUIElementCreateSystemWide() as CFTypeRef) }.ok_or(
            MacosNativeProbeError::MissingTopology("AXUIElementCreateSystemWide"),
        )
    }

    fn copy_ax_attribute_value(
        &self,
        element: AXUIElementRef,
        attribute_name: &str,
    ) -> Result<Option<CfOwned>, MacosNativeProbeError> {
        let attribute = cf_string(attribute_name)?;
        let mut value = ptr::null();
        let status =
            unsafe { AXUIElementCopyAttributeValue(element, attribute.as_type_ref(), &mut value) };

        if status != 0 {
            return Ok(None);
        }

        Ok(unsafe { CfOwned::from_create_rule(value) })
    }

    fn copy_focused_application_ax(&self) -> Result<Option<CfOwned>, MacosNativeProbeError> {
        let system = self.copy_system_wide_ax_element()?;
        self.copy_ax_attribute_value(
            system.as_type_ref() as AXUIElementRef,
            "AXFocusedApplication",
        )
    }

    fn copy_focused_window_ax(
        &self,
        application: &CfOwned,
    ) -> Result<Option<CfOwned>, MacosNativeProbeError> {
        self.copy_ax_attribute_value(
            application.as_type_ref() as AXUIElementRef,
            "AXFocusedWindow",
        )
    }

    fn ax_window_id(&self, element: &CfOwned) -> Result<u64, MacosNativeProbeError> {
        let Some(symbol) = self.resolve_symbol("_AXUIElementGetWindow") else {
            return Err(MacosNativeProbeError::MissingTopology(
                "_AXUIElementGetWindow",
            ));
        };
        let ax_ui_element_get_window: unsafe extern "C" fn(AXUIElementRef, *mut u32) -> OSStatus =
            unsafe { std::mem::transmute(symbol) };
        let mut window_id = 0u32;
        let status = unsafe {
            ax_ui_element_get_window(element.as_type_ref() as AXUIElementRef, &mut window_id)
        };

        if status != 0 || window_id == 0 {
            return Err(MacosNativeProbeError::MissingFocusedWindow);
        }

        Ok(window_id as u64)
    }

    fn probe_focused_window_id(&self) -> Result<Option<u64>, MacosNativeProbeError> {
        focused_window_id_via_ax(
            || self.copy_focused_application_ax(),
            |application| self.copy_focused_window_ax(application),
            |window| self.ax_window_id(window),
        )
    }

    fn process_serial_number_for_pid(
        &self,
        pid: u32,
    ) -> Result<ProcessSerialNumber, MacosNativeOperationError> {
        let Some(get_process_for_pid_symbol) = self.resolve_symbol("GetProcessForPID") else {
            return Err(MacosNativeOperationError::CallFailed("GetProcessForPID"));
        };
        let get_process_for_pid: GetProcessForPidFn =
            unsafe { std::mem::transmute(get_process_for_pid_symbol) };
        let mut psn = ProcessSerialNumber::default();
        let status = unsafe { get_process_for_pid(pid as c_int, &mut psn) };

        (status == 0)
            .then_some(psn)
            .ok_or(MacosNativeOperationError::CallFailed("GetProcessForPID"))
    }

    fn front_process_window(
        &self,
        psn: &ProcessSerialNumber,
        window_id: u64,
    ) -> Result<(), MacosNativeOperationError> {
        let Some(front_process_symbol) = self.resolve_symbol("_SLPSSetFrontProcessWithOptions")
        else {
            return Err(MacosNativeOperationError::CallFailed(
                "_SLPSSetFrontProcessWithOptions",
            ));
        };
        let front_process_with_options: SlpsSetFrontProcessWithOptionsFn =
            unsafe { std::mem::transmute(front_process_symbol) };
        let status =
            unsafe { front_process_with_options(psn, window_id as CGWindowID, CPS_USER_GENERATED) };

        (status == 0)
            .then_some(())
            .ok_or(MacosNativeOperationError::CallFailed(
                "_SLPSSetFrontProcessWithOptions",
            ))
    }

    fn copy_application_ax_element(&self, pid: u32) -> Result<CfOwned, MacosNativeOperationError> {
        unsafe { CfOwned::from_create_rule(AXUIElementCreateApplication(pid as c_int) as CFTypeRef) }
            .ok_or(MacosNativeOperationError::CallFailed(
                "AXUIElementCreateApplication",
            ))
    }

    fn copy_window_ax_for_id(
        &self,
        pid: u32,
        window_id: u64,
    ) -> Result<CfOwned, MacosNativeOperationError> {
        let application = self.copy_application_ax_element(pid)?;
        let windows = self
            .copy_ax_attribute_value(application.as_type_ref() as AXUIElementRef, "AXWindows")
            .map_err(MacosNativeOperationError::from)?
            .ok_or(MacosNativeOperationError::MissingWindow(window_id))?;
        let windows = windows.as_type_ref() as CFArrayRef;

        for candidate in cf_array_iter(windows) {
            let Some(candidate) = (unsafe { CfOwned::from_create_rule(CFRetain(candidate)) }) else {
                continue;
            };
            if self.ax_window_id(&candidate).ok() == Some(window_id) {
                return Ok(candidate);
            }
        }

        Err(MacosNativeOperationError::MissingWindow(window_id))
    }

    fn raise_window_via_ax(&self, window_id: u64, pid: u32) -> Result<(), MacosNativeOperationError> {
        let window = self.copy_window_ax_for_id(pid, window_id)?;
        let action = cf_string("AXRaise").map_err(MacosNativeOperationError::from)?;
        let status = unsafe {
            AXUIElementPerformAction(
                window.as_type_ref() as AXUIElementRef,
                action.as_type_ref(),
            )
        };

        (status == 0)
            .then_some(())
            .ok_or(MacosNativeOperationError::CallFailed(
                "AXUIElementPerformAction",
            ))
    }

    fn set_window_frame_via_ax(
        &self,
        window_id: u64,
        pid: u32,
        frame: Rect,
    ) -> Result<(), MacosNativeOperationError> {
        let window = self.copy_window_ax_for_id(pid, window_id)?;
        let position_attr = cf_string("AXPosition").map_err(MacosNativeOperationError::from)?;
        let size_attr = cf_string("AXSize").map_err(MacosNativeOperationError::from)?;
        let position = CGPoint {
            x: f64::from(frame.x),
            y: f64::from(frame.y),
        };
        let position_value = unsafe {
            CfOwned::from_create_rule(AXValueCreate(
                K_AX_VALUE_TYPE_CGPOINT,
                (&raw const position).cast(),
            ))
        }
        .ok_or(MacosNativeOperationError::CallFailed("AXValueCreate"))?;
        let position_status = unsafe {
            AXUIElementSetAttributeValue(
                window.as_type_ref() as AXUIElementRef,
                position_attr.as_type_ref(),
                position_value.as_type_ref(),
            )
        };

        if position_status != 0 {
            return Err(MacosNativeOperationError::CallFailed(
                "AXUIElementSetAttributeValue",
            ));
        }

        let size = CGSize {
            width: f64::from(frame.w),
            height: f64::from(frame.h),
        };
        let size_value = unsafe {
            CfOwned::from_create_rule(AXValueCreate(
                K_AX_VALUE_TYPE_CGSIZE,
                (&raw const size).cast(),
            ))
        }
        .ok_or(MacosNativeOperationError::CallFailed("AXValueCreate"))?;
        let size_status = unsafe {
            AXUIElementSetAttributeValue(
                window.as_type_ref() as AXUIElementRef,
                size_attr.as_type_ref(),
                size_value.as_type_ref(),
            )
        };

        (size_status == 0)
            .then_some(())
            .ok_or(MacosNativeOperationError::CallFailed(
                "AXUIElementSetAttributeValue",
            ))
    }

    fn swap_window_frames_via_ax(
        &self,
        source_window_id: u64,
        source_frame: Rect,
        target_window_id: u64,
        target_frame: Rect,
    ) -> Result<(), MacosNativeOperationError> {
        let source = self.window_description(source_window_id)?;
        let source_pid = source
            .pid
            .ok_or(MacosNativeOperationError::MissingWindowPid(source_window_id))?;
        let target = self.window_description(target_window_id)?;
        let target_pid = target
            .pid
            .ok_or(MacosNativeOperationError::MissingWindowPid(target_window_id))?;

        self.set_window_frame_via_ax(source_window_id, source_pid, target_frame)?;
        self.set_window_frame_via_ax(target_window_id, target_pid, source_frame)
    }

    fn copy_window_descriptions_raw(
        &self,
        window_ids: CFArrayRef,
    ) -> Result<CfOwned, MacosNativeProbeError> {
        let descriptions = unsafe { CfOwned::from_create_rule(CGWindowListCreateDescriptionFromArray(window_ids)) }
            .ok_or(MacosNativeProbeError::MissingTopology(
                "CGWindowListCreateDescriptionFromArray",
            ))?;

        if cf_array_count(descriptions.as_type_ref() as CFArrayRef) > 0 {
            return Ok(descriptions);
        }

        let target_window_ids = parse_window_ids(window_ids)?;
        let fallback = copy_matching_onscreen_window_descriptions_raw(&target_window_ids)?;
        crate::logging::debug(format!(
            "macos_native: falling back to onscreen descriptions requested_ids={} fallback_descriptions={}",
            target_window_ids.len(),
            cf_array_count(fallback.as_type_ref() as CFArrayRef)
        ));
        Ok(fallback)
    }

    fn copy_managed_display_for_space_raw(
        &self,
        space_id: u64,
    ) -> Result<CfOwned, MacosNativeOperationError> {
        let Some(symbol) = self.resolve_symbol("SLSCopyManagedDisplayForSpace") else {
            return Err(MacosNativeOperationError::CallFailed(
                "SLSCopyManagedDisplayForSpace",
            ));
        };

        let copy_managed_display_for_space: SlsCopyManagedDisplayForSpaceFn =
            unsafe { std::mem::transmute(symbol) };
        let connection_id = self.main_connection_id()?;
        let payload = unsafe {
            CfOwned::from_create_rule(copy_managed_display_for_space(connection_id, space_id))
        }
        .ok_or(MacosNativeOperationError::CallFailed(
            "SLSCopyManagedDisplayForSpace",
        ))?;

        Ok(payload)
    }

    fn switch_space(&self, space_id: u64) -> Result<(), MacosNativeOperationError> {
        let Some(symbol) = self.resolve_symbol("SLSManagedDisplaySetCurrentSpace") else {
            return Err(MacosNativeOperationError::CallFailed(
                "SLSManagedDisplaySetCurrentSpace",
            ));
        };

        let set_current_space: SlsManagedDisplaySetCurrentSpaceFn =
            unsafe { std::mem::transmute(symbol) };
        let connection_id = self.main_connection_id()?;
        let display_identifier = self.copy_managed_display_for_space_raw(space_id)?;

        unsafe {
            set_current_space(
                connection_id,
                display_identifier.as_type_ref() as CFStringRef,
                space_id,
            );
        }

        Ok(())
    }

    fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
        focus_window_via_process_and_raise(
            window_id,
            |target_window_id| {
                let window = self.window_description(target_window_id)?;
                window
                    .pid
                    .ok_or(MacosNativeOperationError::MissingWindowPid(target_window_id))
            },
            |pid| self.process_serial_number_for_pid(pid),
            |psn, target_window_id| self.front_process_window(psn, target_window_id),
            |target_window_id, pid| self.raise_window_via_ax(target_window_id, pid),
        )
    }

    fn window_description(&self, window_id: u64) -> Result<RawWindow, MacosNativeOperationError> {
        let window_number =
            cf_number_from_u64(window_id).map_err(MacosNativeOperationError::from)?;
        let values = [window_number.as_type_ref()];
        let window_list = unsafe {
            CfOwned::from_create_rule(CFArrayCreate(
                ptr::null(),
                values.as_ptr(),
                values.len() as CFIndex,
                &kCFTypeArrayCallBacks,
            ))
        }
        .ok_or(MacosNativeOperationError::CallFailed("CFArrayCreate"))?;
        let descriptions =
            self.copy_window_descriptions_raw(window_list.as_type_ref() as CFArrayRef)?;
        let visible_order = HashMap::new();

        parse_window_descriptions(descriptions.as_type_ref() as CFArrayRef, &visible_order)?
            .into_iter()
            .find(|window| window.id == window_id)
            .ok_or(MacosNativeOperationError::MissingWindow(window_id))
    }

    fn move_window_to_space(
        &self,
        window_id: u64,
        space_id: u64,
    ) -> Result<(), MacosNativeOperationError> {
        let Some(symbol) = self.resolve_symbol("SLSMoveWindowsToManagedSpace") else {
            return Err(MacosNativeOperationError::CallFailed(
                "SLSMoveWindowsToManagedSpace",
            ));
        };

        let move_windows_to_managed_space: SlsMoveWindowsToManagedSpaceFn =
            unsafe { std::mem::transmute(symbol) };
        let connection_id = self.main_connection_id()?;
        let window_number =
            cf_number_from_u64(window_id).map_err(MacosNativeOperationError::from)?;
        let values = [window_number.as_type_ref()];
        let window_list = unsafe {
            CfOwned::from_create_rule(CFArrayCreate(
                ptr::null(),
                values.as_ptr(),
                values.len() as CFIndex,
                &kCFTypeArrayCallBacks,
            ))
        }
        .ok_or(MacosNativeOperationError::CallFailed("CFArrayCreate"))?;

        unsafe {
            move_windows_to_managed_space(
                connection_id,
                window_list.as_type_ref() as CFArrayRef,
                space_id,
            );
        }

        Ok(())
    }
}

impl MacosNativeApi for RealNativeApi {
    fn has_symbol(&self, symbol: &'static str) -> bool {
        self.resolve_symbol(symbol).is_some()
    }

    fn ax_is_trusted(&self) -> bool {
        let Some(symbol) = self.resolve_symbol("AXIsProcessTrusted") else {
            return false;
        };

        let ax_is_process_trusted: AxIsProcessTrustedFn = unsafe { std::mem::transmute(symbol) };
        unsafe { ax_is_process_trusted() != 0 }
    }

    fn minimal_topology_ready(&self) -> bool {
        let Some(symbol) = self.resolve_symbol("SLSMainConnectionID") else {
            return false;
        };

        let main_connection_id: SlsMainConnectionIdFn = unsafe { std::mem::transmute(symbol) };
        unsafe { main_connection_id() != 0 }
    }

    fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
        let payload = self.copy_managed_display_spaces_raw()?;
        parse_managed_spaces(payload.as_type_ref() as CFArrayRef)
    }

    fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
        let payload = self.copy_managed_display_spaces_raw()?;
        let display_identifiers = parse_display_identifiers(payload.as_type_ref() as CFArrayRef)?;
        let active_space_ids = display_identifiers
            .into_iter()
            .map(|display_identifier| self.current_space_for_display(&display_identifier))
            .collect::<Result<HashSet<_>, _>>()?;

        (!active_space_ids.is_empty())
            .then_some(active_space_ids)
            .ok_or(MacosNativeProbeError::MissingTopology(
                "SLSManagedDisplayGetCurrentSpace",
            ))
    }

    fn active_space_windows(&self, space_id: u64) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
        let payload = self.copy_windows_for_space_raw(space_id)?;
        let visible_order =
            query_visible_window_order(&parse_window_ids(payload.as_type_ref() as CFArrayRef)?)?;
        let descriptions = self.copy_window_descriptions_raw(payload.as_type_ref() as CFArrayRef)?;

        assemble_real_active_space_windows(descriptions.as_type_ref() as CFArrayRef, &visible_order)
    }

    fn inactive_space_window_ids(&self) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
        let spaces = self.managed_spaces()?;
        let active_space_ids = self.active_space_ids()?;
        let mut inactive_space_window_ids = HashMap::new();

        for space in spaces {
            if active_space_ids.contains(&space.managed_space_id) {
                continue;
            }

            let payload = self.copy_windows_for_space_raw(space.managed_space_id)?;
            inactive_space_window_ids.insert(
                space.managed_space_id,
                parse_window_ids(payload.as_type_ref() as CFArrayRef)?,
            );
        }

        Ok(inactive_space_window_ids)
    }

    fn switch_space(&self, space_id: u64) -> Result<(), MacosNativeOperationError> {
        Self::switch_space(self, space_id)
    }

    fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
        Self::focus_window(self, window_id)
    }

    fn move_window_to_space(
        &self,
        window_id: u64,
        space_id: u64,
    ) -> Result<(), MacosNativeOperationError> {
        Self::move_window_to_space(self, window_id, space_id)
    }

    fn swap_window_frames(
        &self,
        source_window_id: u64,
        source_frame: Rect,
        target_window_id: u64,
        target_frame: Rect,
    ) -> Result<(), MacosNativeOperationError> {
        Self::swap_window_frames_via_ax(
            self,
            source_window_id,
            source_frame,
            target_window_id,
            target_frame,
        )
    }

    fn focused_window_id(&self) -> Result<Option<u64>, MacosNativeProbeError> {
        Self::probe_focused_window_id(self)
    }

    fn topology_snapshot(&self) -> Result<RawTopologySnapshot, MacosNativeProbeError> {
        let payload = self.copy_managed_display_spaces_raw()?;
        let payload = payload.as_type_ref() as CFArrayRef;
        let spaces = parse_managed_spaces(payload)?;
        let active_space_ids = parse_active_space_ids(payload)?;
        let mut active_space_windows = HashMap::new();
        let mut inactive_space_window_ids = HashMap::new();

        for space in &spaces {
            let payload = self.copy_windows_for_space_raw(space.managed_space_id)?;
            let raw_window_ids = parse_window_ids(payload.as_type_ref() as CFArrayRef)?;

            if active_space_ids.contains(&space.managed_space_id) {
                let visible_order = query_visible_window_order(&raw_window_ids)?;
                let descriptions =
                    self.copy_window_descriptions_raw(payload.as_type_ref() as CFArrayRef)?;
                let windows = assemble_real_active_space_windows(
                    descriptions.as_type_ref() as CFArrayRef,
                    &visible_order,
                )?;

                active_space_windows.insert(space.managed_space_id, windows);
            } else {
                inactive_space_window_ids.insert(space.managed_space_id, raw_window_ids);
            }
        }

        Ok(RawTopologySnapshot {
            spaces,
            active_space_ids,
            active_space_windows,
            inactive_space_window_ids,
            focused_window_id: self.focused_window_id()?,
        })
    }
}

#[allow(dead_code)]
#[derive(Debug)]
struct DylibHandle {
    raw: *mut c_void,
}

// The handle is only used behind immutable method calls and closed on drop.
// We do not share aliasing Rust references into the loaded dylib state itself.
unsafe impl Send for DylibHandle {}

impl DylibHandle {
    #[allow(dead_code)]
    fn open(path: &CStr) -> Option<Self> {
        let raw = unsafe { dlopen(path.as_ptr(), RTLD_LAZY) };
        if raw.is_null() {
            None
        } else {
            Some(Self { raw })
        }
    }

    #[allow(dead_code)]
    fn resolve(&self, symbol: &CStr) -> Option<*mut c_void> {
        let raw = unsafe { dlsym(self.raw, symbol.as_ptr()) };
        if raw.is_null() { None } else { Some(raw) }
    }
}

impl Drop for DylibHandle {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe {
                let _ = dlclose(self.raw);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceKind {
    Desktop,
    Fullscreen,
    SplitView,
    System,
    StageManagerOpaque,
}

#[cfg_attr(not(test), allow(dead_code))]
impl SpaceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Fullscreen => "fullscreen",
            Self::SplitView => "split_view",
            Self::System => "system",
            Self::StageManagerOpaque => "stage_manager_opaque",
        }
    }
}

const DESKTOP_SPACE_TYPE: i32 = 0;
const FULLSCREEN_SPACE_TYPE: i32 = 4;

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawSpaceRecord {
    managed_space_id: u64,
    display_index: usize,
    space_type: i32,
    tile_spaces: Vec<u64>,
    has_tile_layout_manager: bool,
    stage_manager_managed: bool,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceSnapshot {
    pub id: u64,
    pub kind: SpaceKind,
    pub is_active: bool,
    pub ordered_window_ids: Option<Vec<u64>>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowSnapshot {
    pub id: u64,
    pub pid: Option<u32>,
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub space_id: u64,
    pub order_index: Option<usize>,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawWindow {
    id: u64,
    pid: Option<u32>,
    app_id: Option<String>,
    title: Option<String>,
    level: i32,
    visible_index: Option<usize>,
    frame: Option<Rect>,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawTopologySnapshot {
    spaces: Vec<RawSpaceRecord>,
    active_space_ids: HashSet<u64>,
    active_space_windows: HashMap<u64, Vec<RawWindow>>,
    inactive_space_window_ids: HashMap<u64, Vec<u64>>,
    focused_window_id: Option<u64>,
}

fn cf_type_is(value: CFTypeRef, expected_type_id: CFTypeID) -> bool {
    !value.is_null() && unsafe { CFGetTypeID(value) == expected_type_id }
}

fn cf_array_count(array: CFArrayRef) -> usize {
    unsafe { CFArrayGetCount(array) as usize }
}

fn cf_array_value_at(array: CFArrayRef, index: usize) -> Option<CFTypeRef> {
    (index < cf_array_count(array))
        .then(|| unsafe { CFArrayGetValueAtIndex(array, index as CFIndex) })
}

fn cf_array_iter(array: CFArrayRef) -> impl Iterator<Item = CFTypeRef> {
    (0..cf_array_count(array)).filter_map(move |index| cf_array_value_at(array, index))
}

fn cf_as_dictionary(value: CFTypeRef) -> Option<CFDictionaryRef> {
    cf_type_is(value, unsafe { CFDictionaryGetTypeID() }).then_some(value as CFDictionaryRef)
}

fn cf_dictionary_value(dictionary: CFDictionaryRef, key: CFStringRef) -> Option<CFTypeRef> {
    let mut value = ptr::null();
    (unsafe { CFDictionaryGetValueIfPresent(dictionary, key, &mut value) } != 0).then_some(value)
}

fn cf_string(value: &str) -> Result<CfOwned, MacosNativeProbeError> {
    let value = CString::new(value)
        .map_err(|_| MacosNativeProbeError::MissingTopology("CFStringCreateWithCString"))?;
    unsafe {
        CfOwned::from_create_rule(CFStringCreateWithCString(
            ptr::null(),
            value.as_ptr(),
            K_CF_STRING_ENCODING_UTF8,
        ))
    }
    .ok_or(MacosNativeProbeError::MissingTopology(
        "CFStringCreateWithCString",
    ))
}

fn cf_string_to_string(value: CFStringRef) -> Option<String> {
    if !cf_type_is(value as CFTypeRef, unsafe { CFStringGetTypeID() }) {
        return None;
    }

    let length = unsafe { CFStringGetLength(value) };
    let max_size =
        unsafe { CFStringGetMaximumSizeForEncoding(length, K_CF_STRING_ENCODING_UTF8) } + 1;
    let mut buffer = vec![0u8; max_size as usize];
    let ok = unsafe {
        CFStringGetCString(
            value,
            buffer.as_mut_ptr().cast(),
            buffer.len() as CFIndex,
            K_CF_STRING_ENCODING_UTF8,
        ) != 0
    };

    ok.then(|| {
        let nul = buffer
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(buffer.len());
        String::from_utf8_lossy(&buffer[..nul]).into_owned()
    })
}

fn cf_number_from_u64(value: u64) -> Result<CfOwned, MacosNativeProbeError> {
    let value = i64::try_from(value)
        .map_err(|_| MacosNativeProbeError::MissingTopology("SLSCopyWindowsWithOptionsAndTags"))?;

    unsafe {
        CfOwned::from_create_rule(CFNumberCreate(
            ptr::null(),
            K_CF_NUMBER_SINT64_TYPE,
            &value as *const i64 as *const c_void,
        ))
    }
    .ok_or(MacosNativeProbeError::MissingTopology(
        "SLSCopyWindowsWithOptionsAndTags",
    ))
}

fn cf_number_to_i64(number: CFTypeRef) -> Option<i64> {
    if !cf_type_is(number, unsafe { CFNumberGetTypeID() }) {
        return None;
    }

    let mut value = 0i64;
    unsafe {
        CFNumberGetValue(
            number as CFNumberRef,
            K_CF_NUMBER_SINT64_TYPE,
            &mut value as *mut i64 as *mut c_void,
        )
    }
    .ne(&0)
    .then_some(value)
}

fn cf_number_to_u64(number: CFTypeRef) -> Option<u64> {
    cf_number_to_i64(number).and_then(|value| u64::try_from(value).ok())
}

fn cf_number_to_u32(number: CFTypeRef) -> Option<u32> {
    cf_number_to_i64(number).and_then(|value| u32::try_from(value).ok())
}

fn cf_number_to_i32(number: CFTypeRef) -> Option<i32> {
    cf_number_to_i64(number).and_then(|value| i32::try_from(value).ok())
}

fn cf_dictionary_string(dictionary: CFDictionaryRef, key: CFStringRef) -> Option<String> {
    cf_dictionary_value(dictionary, key).and_then(|value| cf_string_to_string(value as CFStringRef))
}

fn cf_dictionary_u64(dictionary: CFDictionaryRef, key: CFStringRef) -> Option<u64> {
    cf_dictionary_value(dictionary, key).and_then(cf_number_to_u64)
}

fn cf_dictionary_u32(dictionary: CFDictionaryRef, key: CFStringRef) -> Option<u32> {
    cf_dictionary_value(dictionary, key).and_then(cf_number_to_u32)
}

fn cf_dictionary_i32(dictionary: CFDictionaryRef, key: CFStringRef) -> Option<i32> {
    cf_dictionary_value(dictionary, key).and_then(cf_number_to_i32)
}

fn cf_dictionary_array(dictionary: CFDictionaryRef, key: CFStringRef) -> Option<CFArrayRef> {
    let value = cf_dictionary_value(dictionary, key)?;
    cf_type_is(value, unsafe { CFArrayGetTypeID() }).then_some(value as CFArrayRef)
}

fn cf_dictionary_dictionary(
    dictionary: CFDictionaryRef,
    key: CFStringRef,
) -> Option<CFDictionaryRef> {
    let value = cf_dictionary_value(dictionary, key)?;
    cf_type_is(value, unsafe { CFDictionaryGetTypeID() }).then_some(value as CFDictionaryRef)
}

fn cg_window_number_key() -> CFStringRef {
    unsafe { kCGWindowNumber }
}

fn cg_window_owner_pid_key() -> CFStringRef {
    unsafe { kCGWindowOwnerPID }
}

fn cg_window_name_key() -> CFStringRef {
    unsafe { kCGWindowName }
}

fn cg_window_layer_key() -> CFStringRef {
    unsafe { kCGWindowLayer }
}

fn cg_window_bounds_key() -> CFStringRef {
    unsafe { kCGWindowBounds }
}

fn cg_window_bounds(description: CFDictionaryRef) -> Option<Rect> {
    let bounds = cf_dictionary_dictionary(description, cg_window_bounds_key())?;
    let x_key = cf_string("X").ok()?;
    let y_key = cf_string("Y").ok()?;
    let width_key = cf_string("Width").ok()?;
    let height_key = cf_string("Height").ok()?;

    Some(Rect {
        x: cf_dictionary_i32(bounds, x_key.as_type_ref() as CFStringRef)?,
        y: cf_dictionary_i32(bounds, y_key.as_type_ref() as CFStringRef)?,
        w: cf_dictionary_i32(bounds, width_key.as_type_ref() as CFStringRef)?,
        h: cf_dictionary_i32(bounds, height_key.as_type_ref() as CFStringRef)?,
    })
}

fn stage_manager_managed(dictionary: CFDictionaryRef) -> bool {
    [
        "StageManagerManaged",
        "StageManagerSpace",
        "isStageManager",
        "StageManager",
    ]
    .into_iter()
    .any(|key| {
        cf_string(key)
            .ok()
            .and_then(|key| cf_dictionary_u64(dictionary, key.as_type_ref() as CFStringRef))
            .is_some()
    })
}

fn parse_display_identifiers(payload: CFArrayRef) -> Result<Vec<String>, MacosNativeProbeError> {
    let display_identifier_key = cf_string("Display Identifier")?;

    cf_array_iter(payload)
        .map(|display| {
            let display = cf_as_dictionary(display).ok_or(
                MacosNativeProbeError::MissingTopology("SLSCopyManagedDisplaySpaces"),
            )?;
            cf_dictionary_string(display, display_identifier_key.as_type_ref()).ok_or(
                MacosNativeProbeError::MissingTopology("SLSCopyManagedDisplaySpaces"),
            )
        })
        .collect()
}

fn parse_active_space_ids(payload: CFArrayRef) -> Result<HashSet<u64>, MacosNativeProbeError> {
    let current_space_key = cf_string("Current Space")?;
    let current_space_id_key = cf_string("Current Space ID")?;
    let current_managed_space_id_key = cf_string("CurrentManagedSpaceID")?;
    let managed_space_id_key = cf_string("ManagedSpaceID")?;
    let id64_key = cf_string("id64")?;
    let active_space_ids = cf_array_iter(payload)
        .map(|display| {
            let display = cf_as_dictionary(display).ok_or(
                MacosNativeProbeError::MissingTopology("SLSCopyManagedDisplaySpaces"),
            )?;

            cf_dictionary_u64(display, current_space_id_key.as_type_ref())
                .or_else(|| cf_dictionary_u64(display, current_managed_space_id_key.as_type_ref()))
                .or_else(|| {
                    cf_dictionary_dictionary(display, current_space_key.as_type_ref()).and_then(
                        |current_space| {
                            cf_dictionary_u64(current_space, managed_space_id_key.as_type_ref())
                                .or_else(|| {
                                    cf_dictionary_u64(current_space, id64_key.as_type_ref())
                                })
                        },
                    )
                })
                .ok_or(MacosNativeProbeError::MissingTopology(
                    "SLSCopyManagedDisplaySpaces",
                ))
        })
        .collect::<Result<HashSet<_>, _>>()?;

    (!active_space_ids.is_empty())
        .then_some(active_space_ids)
        .ok_or(MacosNativeProbeError::MissingTopology(
            "SLSCopyManagedDisplaySpaces",
        ))
}

fn parse_managed_spaces(payload: CFArrayRef) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
    let spaces_key = cf_string("Spaces")?;
    let mut spaces = Vec::new();

    for (display_index, display) in cf_array_iter(payload).enumerate() {
        let display = cf_as_dictionary(display).ok_or(MacosNativeProbeError::MissingTopology(
            "SLSCopyManagedDisplaySpaces",
        ))?;
        let display_spaces = cf_dictionary_array(display, spaces_key.as_type_ref() as CFStringRef)
            .ok_or(MacosNativeProbeError::MissingTopology(
                "SLSCopyManagedDisplaySpaces",
            ))?;

        for space in cf_array_iter(display_spaces) {
            let space = cf_as_dictionary(space).ok_or(MacosNativeProbeError::MissingTopology(
                "SLSCopyManagedDisplaySpaces",
            ))?;
            spaces.push(parse_raw_space_record(space, display_index)?);
        }
    }

    Ok(spaces)
}

fn parse_raw_space_record(
    space: CFDictionaryRef,
    display_index: usize,
) -> Result<RawSpaceRecord, MacosNativeProbeError> {
    let managed_space_id_key = cf_string("ManagedSpaceID")?;
    let space_type_key = cf_string("type")?;
    let tile_layout_manager_key = cf_string("TileLayoutManager")?;
    let tile_spaces_key = cf_string("TileSpaces")?;
    let id64_key = cf_string("id64")?;

    let managed_space_id = cf_dictionary_u64(space, managed_space_id_key.as_type_ref()).ok_or(
        MacosNativeProbeError::MissingTopology("SLSCopyManagedDisplaySpaces"),
    )?;
    let space_type = cf_dictionary_i32(space, space_type_key.as_type_ref()).ok_or(
        MacosNativeProbeError::MissingTopology("SLSCopyManagedDisplaySpaces"),
    )?;
    let tile_layout_manager =
        cf_dictionary_dictionary(space, tile_layout_manager_key.as_type_ref());
    let has_tile_layout_manager = tile_layout_manager.is_some();
    let tile_spaces = tile_layout_manager
        .and_then(|manager| cf_dictionary_array(manager, tile_spaces_key.as_type_ref()))
        .map(|tile_spaces| {
            cf_array_iter(tile_spaces)
                .filter_map(|tile_space| {
                    cf_as_dictionary(tile_space).and_then(|tile_space| {
                        cf_dictionary_u64(tile_space, managed_space_id_key.as_type_ref())
                            .or_else(|| cf_dictionary_u64(tile_space, id64_key.as_type_ref()))
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(RawSpaceRecord {
        managed_space_id,
        display_index,
        space_type,
        tile_spaces,
        has_tile_layout_manager,
        stage_manager_managed: stage_manager_managed(space),
    })
}

fn parse_window_ids(payload: CFArrayRef) -> Result<Vec<u64>, MacosNativeProbeError> {
    cf_array_iter(payload)
        .map(|window_id| {
            cf_number_to_u64(window_id).ok_or(MacosNativeProbeError::MissingTopology(
                "SLSCopyWindowsWithOptionsAndTags",
            ))
        })
        .collect()
}

fn query_visible_window_order(
    target_window_ids: &[u64],
) -> Result<HashMap<u64, usize>, MacosNativeProbeError> {
    let onscreen_descriptions = copy_onscreen_window_descriptions_raw()?;
    let target_window_ids = target_window_ids.iter().copied().collect::<HashSet<_>>();
    let mut visible_order = HashMap::new();
    let window_number_key = cg_window_number_key();

    for (index, window) in
        cf_array_iter(onscreen_descriptions.as_type_ref() as CFArrayRef).enumerate()
    {
        let Some(window) = cf_as_dictionary(window) else {
            continue;
        };
        let Some(window_id) = cf_dictionary_u64(window, window_number_key) else {
            continue;
        };

        if target_window_ids.contains(&window_id) {
            visible_order.insert(window_id, index);
        }
    }

    Ok(visible_order)
}

fn copy_onscreen_window_descriptions_raw() -> Result<CfOwned, MacosNativeProbeError> {
    unsafe {
        CfOwned::from_create_rule(CGWindowListCopyWindowInfo(
            K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY | K_CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS,
            K_CG_NULL_WINDOW_ID,
        ))
    }
    .ok_or(MacosNativeProbeError::MissingTopology(
        "CGWindowListCopyWindowInfo",
    ))
}

fn filter_window_descriptions_raw(
    payload: CFArrayRef,
    target_window_ids: &[u64],
) -> Result<CfOwned, MacosNativeProbeError> {
    let target_window_ids = target_window_ids.iter().copied().collect::<HashSet<_>>();
    let window_number_key = cg_window_number_key();
    let matching = cf_array_iter(payload)
        .filter(|description| {
            cf_as_dictionary(*description)
                .and_then(|description| cf_dictionary_u64(description, window_number_key))
                .is_some_and(|window_id| target_window_ids.contains(&window_id))
        })
        .collect::<Vec<_>>();

    unsafe {
        CfOwned::from_create_rule(CFArrayCreate(
            ptr::null(),
            if matching.is_empty() {
                ptr::null()
            } else {
                matching.as_ptr()
            },
            matching.len() as CFIndex,
            &kCFTypeArrayCallBacks,
        ))
    }
    .ok_or(MacosNativeProbeError::MissingTopology(
        "CGWindowListCopyWindowInfo",
    ))
}

fn copy_matching_onscreen_window_descriptions_raw(
    target_window_ids: &[u64],
) -> Result<CfOwned, MacosNativeProbeError> {
    let onscreen_descriptions = copy_onscreen_window_descriptions_raw()?;
    filter_window_descriptions_raw(onscreen_descriptions.as_type_ref() as CFArrayRef, target_window_ids)
}

fn parse_window_descriptions(
    payload: CFArrayRef,
    visible_order: &HashMap<u64, usize>,
) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
    let mut windows = Vec::new();
    let window_number_key = cg_window_number_key();
    let window_owner_pid_key = cg_window_owner_pid_key();
    let window_name_key = cg_window_name_key();
    let window_layer_key = cg_window_layer_key();

    for description in cf_array_iter(payload) {
        let description = cf_as_dictionary(description).ok_or(
            MacosNativeProbeError::MissingTopology("CGWindowListCreateDescriptionFromArray"),
        )?;
        let id = cf_dictionary_u64(description, window_number_key).ok_or(
            MacosNativeProbeError::MissingTopology("CGWindowListCreateDescriptionFromArray"),
        )?;
        let pid = cf_dictionary_u32(description, window_owner_pid_key);

        windows.push(RawWindow {
            id,
            pid,
            app_id: None,
            title: cf_dictionary_string(description, window_name_key),
            level: cf_dictionary_i32(description, window_layer_key).unwrap_or_default(),
            visible_index: visible_order.get(&id).copied(),
            frame: cg_window_bounds(description),
        });
    }

    Ok(windows)
}

fn assemble_real_active_space_windows(
    payload: CFArrayRef,
    visible_order: &HashMap<u64, usize>,
) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
    parse_window_descriptions(payload, visible_order).map(enrich_real_window_app_ids)
}

#[cfg_attr(not(test), allow(dead_code))]
fn classify_space(raw_space: &RawSpaceRecord) -> SpaceKind {
    if raw_space.stage_manager_managed {
        SpaceKind::StageManagerOpaque
    } else if raw_space.has_tile_layout_manager || !raw_space.tile_spaces.is_empty() {
        SpaceKind::SplitView
    } else if raw_space.space_type == FULLSCREEN_SPACE_TYPE {
        SpaceKind::Fullscreen
    } else if raw_space.space_type == DESKTOP_SPACE_TYPE {
        SpaceKind::Desktop
    } else {
        SpaceKind::System
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn stable_app_id_from_real_window(pid: Option<u32>, _owner_name: Option<&str>) -> Option<String> {
    pid.and_then(stable_app_id_from_pid)
}

fn enrich_real_window_app_ids(windows: Vec<RawWindow>) -> Vec<RawWindow> {
    enrich_real_window_app_ids_with(windows, stable_app_id_from_pid)
}

fn enrich_real_window_app_ids_with<F>(
    windows: Vec<RawWindow>,
    mut resolve_app_id: F,
) -> Vec<RawWindow>
where
    F: FnMut(u32) -> Option<String>,
{
    windows
        .into_iter()
        .map(|mut window| {
            if window.app_id.is_none() {
                window.app_id = window.pid.and_then(&mut resolve_app_id);
            }
            window
        })
        .collect()
}

fn stable_app_id_from_pid(pid: u32) -> Option<String> {
    let lsappinfo_output = lsappinfo_bundle_identifier_output(pid)?;
    parse_lsappinfo_bundle_identifier(&lsappinfo_output)
}

fn lsappinfo_bundle_identifier_output(pid: u32) -> Option<String> {
    let application_specifier = format!("#{pid}");
    let output = std::process::Command::new("lsappinfo")
        .args(["info", "-only", "bundleid", application_specifier.as_str()])
        .output()
        .ok()?;

    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn parse_lsappinfo_bundle_identifier(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        line.strip_prefix("\"CFBundleIdentifier\"=")
            .and_then(|value| {
                let bundle_identifier = value.trim().trim_matches('"');
                (!bundle_identifier.is_empty()).then(|| bundle_identifier.to_string())
            })
    })
}

#[cfg_attr(not(test), allow(dead_code))]
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

#[cfg_attr(not(test), allow(dead_code))]
fn order_active_space_windows(windows: &[RawWindow]) -> Vec<RawWindow> {
    let mut ordered = windows.to_vec();
    ordered.sort_by(compare_active_windows);
    ordered
}

fn active_directed_rects(topology: &RawTopologySnapshot) -> Vec<DirectedRect<u64>> {
    topology
        .active_space_windows
        .values()
        .flat_map(|windows| {
            windows.iter().filter_map(|window| {
                window.frame.map(|rect| DirectedRect {
                    id: window.id,
                    rect,
                })
            })
        })
        .collect()
}

fn active_directed_rects_for_display(
    topology: &RawTopologySnapshot,
    display_index: usize,
) -> Vec<DirectedRect<u64>> {
    topology
        .active_space_windows
        .iter()
        .filter_map(|(space_id, windows)| {
            (display_index_for_space(topology, *space_id) == Some(display_index)).then_some(windows)
        })
        .flat_map(|windows| {
            windows.iter().filter_map(|window| {
                window.frame.map(|rect| DirectedRect {
                    id: window.id,
                    rect,
                })
            })
        })
        .collect()
}

fn active_window_by_id(topology: &RawTopologySnapshot, window_id: u64) -> Option<&RawWindow> {
    topology
        .active_space_windows
        .values()
        .flat_map(|windows| windows.iter())
        .find(|window| window.id == window_id)
}

#[allow(dead_code)]
fn snapshots_for_active_space(space_id: u64, windows: &[RawWindow]) -> Vec<WindowSnapshot> {
    order_active_space_windows(windows)
        .into_iter()
        .enumerate()
        .map(|(index, window)| WindowSnapshot {
            id: window.id,
            pid: window.pid,
            app_id: window.app_id,
            title: window.title,
            space_id,
            order_index: Some(index),
        })
        .collect()
}

#[cfg_attr(not(test), allow(dead_code))]
fn snapshots_for_inactive_space(space_id: u64, window_ids: &[u64]) -> Vec<WindowSnapshot> {
    window_ids
        .iter()
        .map(|id| WindowSnapshot {
            id: *id,
            pid: None,
            app_id: None,
            title: None,
            space_id,
            order_index: None,
        })
        .collect()
}

#[cfg_attr(not(test), allow(dead_code))]
fn window_snapshots_from_topology(topology: &RawTopologySnapshot) -> Vec<WindowSnapshot> {
    let mut snapshots = Vec::new();

    for space in &topology.spaces {
        if topology.active_space_ids.contains(&space.managed_space_id) {
            snapshots.extend(snapshots_for_active_space(
                space.managed_space_id,
                topology
                    .active_space_windows
                    .get(&space.managed_space_id)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
            ));
        } else {
            let window_ids = topology
                .inactive_space_window_ids
                .get(&space.managed_space_id)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            snapshots.extend(snapshots_for_inactive_space(
                space.managed_space_id,
                window_ids,
            ));
        }
    }

    snapshots
}

#[cfg_attr(not(test), allow(dead_code))]
fn space_snapshots_from_topology(topology: &RawTopologySnapshot) -> Vec<SpaceSnapshot> {
    topology
        .spaces
        .iter()
        .map(|space| {
            let is_active = topology.active_space_ids.contains(&space.managed_space_id);
            let ordered_window_ids = is_active.then(|| {
                snapshots_for_active_space(
                    space.managed_space_id,
                    topology
                        .active_space_windows
                        .get(&space.managed_space_id)
                        .map(Vec::as_slice)
                        .unwrap_or(&[]),
                )
                .into_iter()
                .map(|window| window.id)
                .collect::<Vec<_>>()
            });

            SpaceSnapshot {
                id: space.managed_space_id,
                kind: classify_space(space),
                is_active,
                ordered_window_ids,
            }
        })
        .collect()
}

#[cfg_attr(not(test), allow(dead_code))]
fn focused_window_from_topology(
    topology: &RawTopologySnapshot,
) -> Result<WindowSnapshot, MacosNativeProbeError> {
    let focused_window_id = topology
        .focused_window_id
        .and_then(|window_id| {
            window_snapshots_from_topology(topology)
                .into_iter()
                .find(|window| window.id == window_id)
                .map(|window| (window.space_id, window.id))
        })
        .or_else(|| {
            topology
                .active_space_windows
                .iter()
                .flat_map(|(space_id, windows)| {
                    windows.iter().cloned().map(|window| (*space_id, window))
                })
                .min_by(|(_, left), (_, right)| compare_active_windows(left, right))
                .map(|(space_id, window)| (space_id, window.id))
        })
        .ok_or(MacosNativeProbeError::MissingFocusedWindow)?;

    window_snapshots_from_topology(topology)
        .into_iter()
        .find(|window| (window.space_id, window.id) == focused_window_id)
        .ok_or(MacosNativeProbeError::MissingFocusedWindow)
}

fn space_id_for_window(topology: &RawTopologySnapshot, window_id: u64) -> Option<u64> {
    topology
        .active_space_windows
        .iter()
        .find_map(|(space_id, windows)| {
            windows
                .iter()
                .any(|window| window.id == window_id)
                .then_some(*space_id)
        })
        .or_else(|| {
            topology
                .inactive_space_window_ids
                .iter()
                .find_map(|(space_id, windows)| windows.contains(&window_id).then_some(*space_id))
        })
}

fn display_index_for_space(topology: &RawTopologySnapshot, space_id: u64) -> Option<usize> {
    topology
        .spaces
        .iter()
        .find(|space| space.managed_space_id == space_id)
        .map(|space| space.display_index)
}

fn adjacent_space_in_direction(
    topology: &RawTopologySnapshot,
    source_space_id: u64,
    direction: Direction,
) -> Option<u64> {
    let source_space = topology
        .spaces
        .iter()
        .find(|space| space.managed_space_id == source_space_id)?;
    let display_spaces = topology
        .spaces
        .iter()
        .filter(|space| space.display_index == source_space.display_index)
        .collect::<Vec<_>>();
    let source_index = display_spaces
        .iter()
        .position(|space| space.managed_space_id == source_space_id)?;

    match direction {
        Direction::West => display_spaces[..source_index]
            .iter()
            .rev()
            .find(|space| classify_space(space) != SpaceKind::StageManagerOpaque)
            .map(|space| space.managed_space_id),
        Direction::East => display_spaces[source_index + 1..]
            .iter()
            .find(|space| classify_space(space) != SpaceKind::StageManagerOpaque)
            .map(|space| space.managed_space_id),
        Direction::North | Direction::South => None,
    }
}

fn ensure_supported_target_space(
    topology: &RawTopologySnapshot,
    space_id: u64,
) -> Result<(), MacosNativeOperationError> {
    let Some(space) = topology
        .spaces
        .iter()
        .find(|space| space.managed_space_id == space_id)
    else {
        return Err(MacosNativeOperationError::MissingSpace(space_id));
    };

    (classify_space(space) != SpaceKind::StageManagerOpaque)
        .then_some(())
        .ok_or(MacosNativeOperationError::UnsupportedStageManagerSpace(
            space_id,
        ))
}

fn topology_contains_window(topology: &RawTopologySnapshot, window_id: u64) -> bool {
    space_id_for_window(topology, window_id).is_some()
}

fn map_probe_error(err: MacosNativeProbeError) -> anyhow::Error {
    match err {
        MacosNativeProbeError::MissingFocusedWindow => anyhow::anyhow!("no focused window"),
        other => anyhow::Error::new(other),
    }
}

fn map_operation_error(err: MacosNativeOperationError) -> anyhow::Error {
    anyhow::Error::new(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::RefCell,
        collections::BTreeSet,
        rc::Rc,
        sync::{Arc, Mutex},
    };

    type CFDictionaryHashCallBack = unsafe extern "C" fn(*const c_void) -> usize;

    #[repr(C)]
    struct CFDictionaryKeyCallBacks {
        version: CFIndex,
        retain: Option<CFArrayRetainCallBack>,
        release: Option<CFArrayReleaseCallBack>,
        copy_description: Option<CFArrayCopyDescriptionCallBack>,
        equal: Option<CFArrayEqualCallBack>,
        hash: Option<CFDictionaryHashCallBack>,
    }

    #[repr(C)]
    struct CFDictionaryValueCallBacks {
        version: CFIndex,
        retain: Option<CFArrayRetainCallBack>,
        release: Option<CFArrayReleaseCallBack>,
        copy_description: Option<CFArrayCopyDescriptionCallBack>,
        equal: Option<CFArrayEqualCallBack>,
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        static kCFTypeDictionaryKeyCallBacks: CFDictionaryKeyCallBacks;
        static kCFTypeDictionaryValueCallBacks: CFDictionaryValueCallBacks;

        fn CFDictionaryCreate(
            allocator: CFAllocatorRef,
            keys: *const *const c_void,
            values: *const *const c_void,
            num_values: CFIndex,
            key_callbacks: *const CFDictionaryKeyCallBacks,
            value_callbacks: *const CFDictionaryValueCallBacks,
        ) -> CFDictionaryRef;
    }

    #[derive(Debug, Clone)]
    struct FakeNativeApi {
        symbols: BTreeSet<&'static str>,
        ax_trusted: bool,
        minimal_topology_ready: bool,
        topology: RawTopologySnapshot,
        calls: Rc<RefCell<Vec<String>>>,
    }

    impl Default for FakeNativeApi {
        fn default() -> Self {
            Self {
                symbols: REQUIRED_PRIVATE_SYMBOLS.iter().copied().collect(),
                ax_trusted: true,
                minimal_topology_ready: true,
                topology: Self::topology_fixture(41),
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
            _source_frame: Rect,
            target_window_id: u64,
            _target_frame: Rect,
        ) -> Result<(), MacosNativeOperationError> {
            self.calls
                .borrow_mut()
                .push(format!("swap_window_frames:{source_window_id}:{target_window_id}"));
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
            _source_frame: Rect,
            _target_window_id: u64,
            _target_frame: Rect,
        ) -> Result<(), MacosNativeOperationError> {
            Ok(())
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
            _source_frame: Rect,
            target_window_id: u64,
            _target_frame: Rect,
        ) -> Result<(), MacosNativeOperationError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("swap_window_frames:{source_window_id}:{target_window_id}"));
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
            _source_frame: Rect,
            _target_window_id: u64,
            _target_frame: Rect,
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

    impl RawWindow {
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
            self.frame = Some(frame);
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

    fn fake_context_for_focus(
        window_id: u64,
        target_space_id: u64,
    ) -> (MacosNativeContext<FakeNativeApi>, Rc<RefCell<Vec<String>>>) {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let api = FakeNativeApi::default()
            .with_calls(calls.clone())
            .with_topology(focus_target_topology_fixture(window_id, target_space_id));

        (MacosNativeContext::connect_with_api(api).unwrap(), calls)
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

    fn cf_test_array(values: &[CFTypeRef]) -> CfOwned {
        unsafe {
            CfOwned::from_create_rule(CFArrayCreate(
                ptr::null(),
                values.as_ptr(),
                values.len() as CFIndex,
                &kCFTypeArrayCallBacks,
            ))
        }
        .expect("CFArrayCreate should produce a test payload")
    }

    fn cf_test_dictionary(entries: &[(CFTypeRef, CFTypeRef)]) -> CfOwned {
        let (keys, values): (Vec<_>, Vec<_>) = entries.iter().copied().unzip();

        unsafe {
            CfOwned::from_create_rule(CFDictionaryCreate(
                ptr::null(),
                keys.as_ptr(),
                values.as_ptr(),
                entries.len() as CFIndex,
                &kCFTypeDictionaryKeyCallBacks,
                &kCFTypeDictionaryValueCallBacks,
            ))
        }
        .expect("CFDictionaryCreate should produce a test payload")
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
    fn focus_window_via_process_and_raise_fronts_then_raises_target_window() {
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
                move |window_id, pid| {
                    calls.borrow_mut().push(format!("raise:{window_id}:{pid}"));
                    Ok(())
                }
            },
        )
        .unwrap();

        assert_eq!(
            take_calls(&calls),
            vec!["front:1:2:77", "raise:77:5151"]
        );
    }

    #[test]
    fn focus_window_switches_to_target_space_before_fronting_window() {
        let (ctx, calls) = fake_context_for_focus(77, 9);

        ctx.focus_window(77).unwrap();

        assert_eq!(
            take_calls(&calls),
            vec!["switch_space:9", "focus_window:77"]
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
        let (ctx, calls) = fake_context_for_move(51, 12);

        ctx.switch_space(12).unwrap();

        assert_eq!(take_calls(&calls), vec!["switch_space:12"]);
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
            .with_calls(calls.clone())
            .with_topology(topology);
        let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        adapter.focus_direction_inner(Direction::West).unwrap();

        assert_eq!(take_calls(&calls), vec!["focus_window:10"]);
    }

    #[test]
    fn backend_focus_direction_switches_to_previous_space_when_no_west_window_exists() {
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
            inactive_space_window_ids: HashMap::from([(1, vec![10]), (3, vec![30])]),
            focused_window_id: Some(20),
        };
        let api = FakeNativeApi::default()
            .with_calls(calls.clone())
            .with_topology(topology);
        let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        adapter.focus_direction_inner(Direction::West).unwrap();

        assert_eq!(take_calls(&calls), vec!["switch_space:1"]);
    }

    #[test]
    fn backend_focus_direction_switches_to_previous_space_on_same_display_only() {
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
                    vec![raw_window(200)
                        .with_pid(2200)
                        .with_app_id("com.example.left-display")
                        .with_title("left display")
                        .with_frame(crate::engine::topology::Rect {
                            x: 0,
                            y: 0,
                            w: 100,
                            h: 100,
                        })],
                ),
                (
                    11,
                    vec![raw_window(1100)
                        .with_visible_index(0)
                        .with_pid(1111)
                        .with_app_id("com.example.right-display")
                        .with_title("right display")
                        .with_frame(crate::engine::topology::Rect {
                            x: 120,
                            y: 0,
                            w: 100,
                            h: 100,
                        })],
                ),
            ]),
            inactive_space_window_ids: HashMap::from([(1, vec![100]), (10, vec![1000])]),
            focused_window_id: Some(1100),
        };
        let api = FakeNativeApi::default()
            .with_calls(calls.clone())
            .with_topology(topology);
        let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

        adapter.focus_direction_inner(Direction::West).unwrap();

        assert_eq!(take_calls(&calls), vec!["switch_space:10"]);
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
