use std::{
    collections::{HashMap, HashSet},
    ffi::{c_char, c_int, c_void, CStr, CString},
    fmt,
    ptr::NonNull,
};

use objc2_core_foundation::{
    CFArray, CFDictionary, CFNumber, CFNumberType, CFRetained, CFString, CFType,
};
use objc2_core_graphics::{
    kCGNullWindowID, kCGWindowLayer, kCGWindowName, kCGWindowNumber, kCGWindowOwnerName,
    kCGWindowOwnerPID, CGWindowListCopyWindowInfo, CGWindowListCreateDescriptionFromArray,
    CGWindowListOption,
};

#[allow(dead_code)]
const REQUIRED_PRIVATE_SYMBOLS: &[&str] = &[
    "SLSMainConnectionID",
    "SLSCopyManagedDisplaySpaces",
    "SLSManagedDisplayGetCurrentSpace",
    "SLSCopyWindowsWithOptionsAndTags",
    "AXIsProcessTrusted",
    "_AXUIElementGetWindow",
    "_SLPSSetFrontProcessWithOptions",
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
type SlsCopyManagedDisplaySpacesFn = unsafe extern "C" fn(u32) -> Option<NonNull<CFArray>>;
#[allow(dead_code)]
type SlsManagedDisplayGetCurrentSpaceFn = unsafe extern "C" fn(u32, *const CFString) -> u64;
#[allow(dead_code)]
type SlsCopyWindowsWithOptionsAndTagsFn = unsafe extern "C" fn(
    u32,
    u32,
    *const CFArray,
    i32,
    *mut i64,
    *mut i64,
) -> Option<NonNull<CFArray>>;

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
    MissingFocusedWindow(u64),
}

impl fmt::Display for MacosNativeProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingTopology(query) => {
                write!(f, "macOS native topology query is unavailable: {query}")
            }
            Self::MissingFocusedWindow(space_id) => {
                write!(f, "no focused window was found for active Space {space_id}")
            }
        }
    }
}

impl std::error::Error for MacosNativeProbeError {}

#[allow(dead_code)]
pub(crate) trait MacosNativeApi {
    fn has_symbol(&self, symbol: &'static str) -> bool;
    fn ax_is_trusted(&self) -> bool;
    fn minimal_topology_ready(&self) -> bool;
    fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError>;
    fn current_space_id(&self) -> Result<u64, MacosNativeProbeError>;
    fn active_space_windows(&self, space_id: u64) -> Result<Vec<RawWindow>, MacosNativeProbeError>;
    fn inactive_space_window_ids(&self) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError>;
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

    fn topology_snapshot(&self) -> Result<RawTopologySnapshot, MacosNativeProbeError> {
        let spaces = self.api.managed_spaces()?;
        let active_space_id = self.api.current_space_id()?;
        let active_space_windows = self.api.active_space_windows(active_space_id)?;
        let inactive_space_window_ids = self.api.inactive_space_window_ids()?;

        Ok(RawTopologySnapshot {
            spaces,
            active_space_id,
            active_space_windows,
            inactive_space_window_ids,
        })
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

    fn copy_managed_display_spaces_raw(
        &self,
    ) -> Result<CFRetained<CFArray>, MacosNativeProbeError> {
        let Some(symbol) = self.resolve_symbol("SLSCopyManagedDisplaySpaces") else {
            return Err(MacosNativeProbeError::MissingTopology(
                "SLSCopyManagedDisplaySpaces",
            ));
        };

        let copy_managed_display_spaces: SlsCopyManagedDisplaySpacesFn =
            unsafe { std::mem::transmute(symbol) };
        let connection_id = self.main_connection_id()?;
        let payload = unsafe { copy_managed_display_spaces(connection_id) }.ok_or(
            MacosNativeProbeError::MissingTopology("SLSCopyManagedDisplaySpaces"),
        )?;

        Ok(unsafe { CFRetained::from_raw(payload) })
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
        let display_identifier = CFString::from_str(display_identifier);
        let space_id = unsafe {
            current_space_for_display(connection_id, &*display_identifier as *const CFString)
        };

        (space_id != 0)
            .then_some(space_id)
            .ok_or(MacosNativeProbeError::MissingTopology(
                "SLSManagedDisplayGetCurrentSpace",
            ))
    }

    fn copy_windows_for_space_raw(
        &self,
        space_id: u64,
    ) -> Result<CFRetained<CFArray>, MacosNativeProbeError> {
        let Some(symbol) = self.resolve_symbol("SLSCopyWindowsWithOptionsAndTags") else {
            return Err(MacosNativeProbeError::MissingTopology(
                "SLSCopyWindowsWithOptionsAndTags",
            ));
        };

        let copy_windows_with_options_and_tags: SlsCopyWindowsWithOptionsAndTagsFn =
            unsafe { std::mem::transmute(symbol) };
        let connection_id = self.main_connection_id()?;
        let space_number = cf_number_from_u64(space_id)?;
        let space_list = CFArray::from_objects(&[&*space_number]);
        let space_list = unsafe { (&*space_list).cast_unchecked::<CFType>() };
        let mut set_tags = 0i64;
        let mut clear_tags = 0i64;
        let payload = unsafe {
            copy_windows_with_options_and_tags(
                connection_id,
                0,
                space_list as *const CFArray<CFType> as *const CFArray,
                0x2,
                &mut set_tags,
                &mut clear_tags,
            )
        }
        .ok_or(MacosNativeProbeError::MissingTopology(
            "SLSCopyWindowsWithOptionsAndTags",
        ))?;

        Ok(unsafe { CFRetained::from_raw(payload) })
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
        parse_managed_spaces(&payload)
    }

    fn current_space_id(&self) -> Result<u64, MacosNativeProbeError> {
        let payload = self.copy_managed_display_spaces_raw()?;
        let display_identifiers = parse_display_identifiers(&payload)?;

        display_identifiers
            .into_iter()
            .find_map(|display_identifier| self.current_space_for_display(&display_identifier).ok())
            .ok_or(MacosNativeProbeError::MissingTopology(
                "SLSManagedDisplayGetCurrentSpace",
            ))
    }

    fn active_space_windows(&self, space_id: u64) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
        let payload = self.copy_windows_for_space_raw(space_id)?;
        let visible_order = query_visible_window_order(&parse_window_ids(&payload)?)?;
        let descriptions = unsafe { CGWindowListCreateDescriptionFromArray(Some(&payload)) }
            .ok_or(MacosNativeProbeError::MissingTopology(
                "CGWindowListCreateDescriptionFromArray",
            ))?;

        parse_window_descriptions(&descriptions, &visible_order)
    }

    fn inactive_space_window_ids(&self) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
        let spaces = self.managed_spaces()?;
        let active_space_id = self.current_space_id()?;
        let mut inactive_space_window_ids = HashMap::new();

        for space in spaces {
            if space.managed_space_id == active_space_id {
                continue;
            }

            let payload = self.copy_windows_for_space_raw(space.managed_space_id)?;
            inactive_space_window_ids.insert(space.managed_space_id, parse_window_ids(&payload)?);
        }

        Ok(inactive_space_window_ids)
    }
}

#[allow(dead_code)]
#[derive(Debug)]
struct DylibHandle {
    raw: *mut c_void,
}

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
        if raw.is_null() {
            None
        } else {
            Some(raw)
        }
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
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
struct RawTopologySnapshot {
    spaces: Vec<RawSpaceRecord>,
    active_space_id: u64,
    active_space_windows: Vec<RawWindow>,
    inactive_space_window_ids: HashMap<u64, Vec<u64>>,
}

fn cf_number_from_u64(value: u64) -> Result<CFRetained<CFNumber>, MacosNativeProbeError> {
    let value = i64::try_from(value)
        .map_err(|_| MacosNativeProbeError::MissingTopology("SLSCopyWindowsWithOptionsAndTags"))?;

    unsafe {
        CFNumber::new(
            None,
            CFNumberType::SInt64Type,
            &value as *const i64 as *const c_void,
        )
    }
    .ok_or(MacosNativeProbeError::MissingTopology(
        "SLSCopyWindowsWithOptionsAndTags",
    ))
}

fn cf_number_to_i64(number: &CFNumber) -> Option<i64> {
    let mut value = 0i64;
    unsafe {
        number.value(
            CFNumberType::SInt64Type,
            &mut value as *mut i64 as *mut c_void,
        )
    }
    .then_some(value)
}

fn cf_number_to_u64(number: &CFNumber) -> Option<u64> {
    cf_number_to_i64(number).and_then(|value| u64::try_from(value).ok())
}

fn cf_number_to_u32(number: &CFNumber) -> Option<u32> {
    cf_number_to_i64(number).and_then(|value| u32::try_from(value).ok())
}

fn cf_number_to_i32(number: &CFNumber) -> Option<i32> {
    cf_number_to_i64(number).and_then(|value| i32::try_from(value).ok())
}

fn cf_dictionary_value<'a>(
    dictionary: &'a CFDictionary<CFString, CFType>,
    key: &CFString,
) -> Option<CFRetained<CFType>> {
    dictionary.get(key)
}

fn cf_dictionary_string(
    dictionary: &CFDictionary<CFString, CFType>,
    key: &CFString,
) -> Option<String> {
    cf_dictionary_value(dictionary, key)
        .and_then(|value| value.downcast::<CFString>().ok())
        .map(|value| value.to_string())
}

fn cf_dictionary_u64(dictionary: &CFDictionary<CFString, CFType>, key: &CFString) -> Option<u64> {
    cf_dictionary_value(dictionary, key)
        .and_then(|value| value.downcast::<CFNumber>().ok())
        .and_then(|value| cf_number_to_u64(&value))
}

fn cf_dictionary_u32(dictionary: &CFDictionary<CFString, CFType>, key: &CFString) -> Option<u32> {
    cf_dictionary_value(dictionary, key)
        .and_then(|value| value.downcast::<CFNumber>().ok())
        .and_then(|value| cf_number_to_u32(&value))
}

fn cf_dictionary_i32(dictionary: &CFDictionary<CFString, CFType>, key: &CFString) -> Option<i32> {
    cf_dictionary_value(dictionary, key)
        .and_then(|value| value.downcast::<CFNumber>().ok())
        .and_then(|value| cf_number_to_i32(&value))
}

fn cf_dictionary_array(
    dictionary: &CFDictionary<CFString, CFType>,
    key: &CFString,
) -> Option<CFRetained<CFArray>> {
    cf_dictionary_value(dictionary, key).and_then(|value| value.downcast::<CFArray>().ok())
}

fn cf_dictionary_dictionary(
    dictionary: &CFDictionary<CFString, CFType>,
    key: &CFString,
) -> Option<CFRetained<CFDictionary>> {
    cf_dictionary_value(dictionary, key).and_then(|value| value.downcast::<CFDictionary>().ok())
}

fn cg_window_number_key() -> &'static CFString {
    unsafe { kCGWindowNumber }
}

fn cg_window_owner_pid_key() -> &'static CFString {
    unsafe { kCGWindowOwnerPID }
}

fn cg_window_owner_name_key() -> &'static CFString {
    unsafe { kCGWindowOwnerName }
}

fn cg_window_name_key() -> &'static CFString {
    unsafe { kCGWindowName }
}

fn cg_window_layer_key() -> &'static CFString {
    unsafe { kCGWindowLayer }
}

fn stage_manager_managed(dictionary: &CFDictionary<CFString, CFType>) -> bool {
    [
        "StageManagerManaged",
        "StageManagerSpace",
        "isStageManager",
        "StageManager",
    ]
    .into_iter()
    .any(|key| cf_dictionary_u64(dictionary, &CFString::from_static_str(key)).is_some())
}

fn parse_display_identifiers(payload: &CFArray) -> Result<Vec<String>, MacosNativeProbeError> {
    let displays = unsafe { payload.cast_unchecked::<CFDictionary>() };
    let display_identifier_key = CFString::from_static_str("Display Identifier");

    displays
        .iter()
        .map(|display| {
            let display = unsafe { (&*display).cast_unchecked::<CFString, CFType>() };
            cf_dictionary_string(display, &display_identifier_key).ok_or(
                MacosNativeProbeError::MissingTopology("SLSCopyManagedDisplaySpaces"),
            )
        })
        .collect()
}

fn parse_managed_spaces(payload: &CFArray) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
    let displays = unsafe { payload.cast_unchecked::<CFDictionary>() };
    let spaces_key = CFString::from_static_str("Spaces");
    let mut spaces = Vec::new();

    for display in displays.iter() {
        let display = unsafe { (&*display).cast_unchecked::<CFString, CFType>() };
        let display_spaces = cf_dictionary_array(display, &spaces_key).ok_or(
            MacosNativeProbeError::MissingTopology("SLSCopyManagedDisplaySpaces"),
        )?;
        let display_spaces = unsafe { (&*display_spaces).cast_unchecked::<CFDictionary>() };

        for space in display_spaces.iter() {
            spaces.push(parse_raw_space_record(&space)?);
        }
    }

    Ok(spaces)
}

fn parse_raw_space_record(space: &CFDictionary) -> Result<RawSpaceRecord, MacosNativeProbeError> {
    let space = unsafe { space.cast_unchecked::<CFString, CFType>() };
    let managed_space_id_key = CFString::from_static_str("ManagedSpaceID");
    let space_type_key = CFString::from_static_str("type");
    let tile_layout_manager_key = CFString::from_static_str("TileLayoutManager");
    let tile_spaces_key = CFString::from_static_str("TileSpaces");

    let managed_space_id = cf_dictionary_u64(space, &managed_space_id_key).ok_or(
        MacosNativeProbeError::MissingTopology("SLSCopyManagedDisplaySpaces"),
    )?;
    let space_type = cf_dictionary_i32(space, &space_type_key).ok_or(
        MacosNativeProbeError::MissingTopology("SLSCopyManagedDisplaySpaces"),
    )?;
    let tile_layout_manager = cf_dictionary_dictionary(space, &tile_layout_manager_key);
    let has_tile_layout_manager = tile_layout_manager.is_some();
    let tile_spaces = tile_layout_manager
        .as_ref()
        .and_then(|manager| {
            let manager = unsafe { (&**manager).cast_unchecked::<CFString, CFType>() };
            cf_dictionary_array(manager, &tile_spaces_key)
        })
        .map(|tile_spaces| {
            let tile_spaces = unsafe { (&*tile_spaces).cast_unchecked::<CFDictionary>() };

            tile_spaces
                .iter()
                .filter_map(|tile_space| {
                    let tile_space = unsafe { (&*tile_space).cast_unchecked::<CFString, CFType>() };
                    cf_dictionary_u64(tile_space, &managed_space_id_key).or_else(|| {
                        cf_dictionary_u64(tile_space, &CFString::from_static_str("id64"))
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(RawSpaceRecord {
        managed_space_id,
        space_type,
        tile_spaces,
        has_tile_layout_manager,
        stage_manager_managed: stage_manager_managed(space),
    })
}

fn parse_window_ids(payload: &CFArray) -> Result<Vec<u64>, MacosNativeProbeError> {
    let window_ids = unsafe { payload.cast_unchecked::<CFNumber>() };

    window_ids
        .iter()
        .map(|window_id| {
            cf_number_to_u64(window_id.as_ref()).ok_or(MacosNativeProbeError::MissingTopology(
                "SLSCopyWindowsWithOptionsAndTags",
            ))
        })
        .collect()
}

fn query_visible_window_order(
    target_window_ids: &[u64],
) -> Result<HashMap<u64, usize>, MacosNativeProbeError> {
    let onscreen_descriptions = CGWindowListCopyWindowInfo(
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements,
        kCGNullWindowID,
    )
    .ok_or(MacosNativeProbeError::MissingTopology(
        "CGWindowListCopyWindowInfo",
    ))?;
    let onscreen_descriptions =
        unsafe { (&*onscreen_descriptions).cast_unchecked::<CFDictionary>() };
    let target_window_ids = target_window_ids.iter().copied().collect::<HashSet<_>>();
    let mut visible_order = HashMap::new();
    let window_number_key = cg_window_number_key();

    for (index, window) in onscreen_descriptions.iter().enumerate() {
        let window = unsafe { (&*window).cast_unchecked::<CFString, CFType>() };
        let Some(window_id) = cf_dictionary_u64(window, window_number_key) else {
            continue;
        };

        if target_window_ids.contains(&window_id) {
            visible_order.insert(window_id, index);
        }
    }

    Ok(visible_order)
}

fn parse_window_descriptions(
    payload: &CFArray,
    visible_order: &HashMap<u64, usize>,
) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
    let descriptions = unsafe { payload.cast_unchecked::<CFDictionary>() };
    let mut windows = Vec::new();
    let window_number_key = cg_window_number_key();
    let window_owner_pid_key = cg_window_owner_pid_key();
    let window_owner_name_key = cg_window_owner_name_key();
    let window_name_key = cg_window_name_key();
    let window_layer_key = cg_window_layer_key();

    for description in descriptions.iter() {
        let description = unsafe { (&*description).cast_unchecked::<CFString, CFType>() };
        let id = cf_dictionary_u64(description, window_number_key).ok_or(
            MacosNativeProbeError::MissingTopology("CGWindowListCreateDescriptionFromArray"),
        )?;

        windows.push(RawWindow {
            id,
            pid: cf_dictionary_u32(description, window_owner_pid_key),
            app_id: cf_dictionary_string(description, window_owner_name_key),
            title: cf_dictionary_string(description, window_name_key),
            level: cf_dictionary_i32(description, window_layer_key).unwrap_or_default(),
            visible_index: visible_order.get(&id).copied(),
        });
    }

    Ok(windows)
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
fn order_active_space_windows(windows: &[RawWindow]) -> Vec<RawWindow> {
    let mut ordered = windows.to_vec();
    ordered.sort_by(|left, right| {
        match (left.visible_index, right.visible_index) {
            (Some(left_index), Some(right_index)) => left_index.cmp(&right_index),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
        .then_with(|| right.level.cmp(&left.level))
        .then_with(|| left.id.cmp(&right.id))
    });
    ordered
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
        if space.managed_space_id == topology.active_space_id {
            snapshots.extend(snapshots_for_active_space(
                space.managed_space_id,
                &topology.active_space_windows,
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
    let active_window_ids =
        snapshots_for_active_space(topology.active_space_id, &topology.active_space_windows)
            .into_iter()
            .map(|window| window.id)
            .collect::<Vec<_>>();

    topology
        .spaces
        .iter()
        .map(|space| SpaceSnapshot {
            id: space.managed_space_id,
            kind: classify_space(space),
            is_active: space.managed_space_id == topology.active_space_id,
            ordered_window_ids: (space.managed_space_id == topology.active_space_id)
                .then(|| active_window_ids.clone()),
        })
        .collect()
}

#[cfg_attr(not(test), allow(dead_code))]
fn focused_window_from_topology(
    topology: &RawTopologySnapshot,
) -> Result<WindowSnapshot, MacosNativeProbeError> {
    window_snapshots_from_topology(topology)
        .into_iter()
        .filter(|window| window.space_id == topology.active_space_id)
        .min_by_key(|window| window.order_index.unwrap_or(usize::MAX))
        .ok_or(MacosNativeProbeError::MissingFocusedWindow(
            topology.active_space_id,
        ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[derive(Debug, Clone)]
    struct FakeNativeApi {
        symbols: BTreeSet<&'static str>,
        ax_trusted: bool,
        minimal_topology_ready: bool,
        topology: RawTopologySnapshot,
    }

    impl Default for FakeNativeApi {
        fn default() -> Self {
            Self {
                symbols: REQUIRED_PRIVATE_SYMBOLS.iter().copied().collect(),
                ax_trusted: true,
                minimal_topology_ready: true,
                topology: Self::topology_fixture(41),
            }
        }
    }

    impl FakeNativeApi {
        fn topology_fixture(active_window_id: u64) -> RawTopologySnapshot {
            RawTopologySnapshot {
                spaces: vec![raw_desktop_space(1), raw_split_space(2, &[21, 22])],
                active_space_id: 1,
                active_space_windows: vec![raw_window(active_window_id)
                    .with_visible_index(0)
                    .with_pid(4242)
                    .with_app_id("com.example.focused")
                    .with_title("Focused window")],
                inactive_space_window_ids: HashMap::from([(2, vec![21, 22])]),
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

        fn current_space_id(&self) -> Result<u64, MacosNativeProbeError> {
            Ok(self.topology.active_space_id)
        }

        fn active_space_windows(
            &self,
            space_id: u64,
        ) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
            if space_id == self.topology.active_space_id {
                Ok(self.topology.active_space_windows.clone())
            } else {
                Ok(Vec::new())
            }
        }

        fn inactive_space_window_ids(
            &self,
        ) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
            Ok(self.topology.inactive_space_window_ids.clone())
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
    }

    fn raw_desktop_space(managed_space_id: u64) -> RawSpaceRecord {
        RawSpaceRecord {
            managed_space_id,
            space_type: DESKTOP_SPACE_TYPE,
            tile_spaces: Vec::new(),
            has_tile_layout_manager: false,
            stage_manager_managed: false,
        }
    }

    fn raw_fullscreen_space(managed_space_id: u64) -> RawSpaceRecord {
        RawSpaceRecord {
            managed_space_id,
            space_type: FULLSCREEN_SPACE_TYPE,
            tile_spaces: Vec::new(),
            has_tile_layout_manager: false,
            stage_manager_managed: false,
        }
    }

    fn raw_split_space(managed_space_id: u64, tile_spaces: &[u64]) -> RawSpaceRecord {
        RawSpaceRecord {
            managed_space_id,
            space_type: DESKTOP_SPACE_TYPE,
            tile_spaces: tile_spaces.to_vec(),
            has_tile_layout_manager: true,
            stage_manager_managed: false,
        }
    }

    fn raw_stage_manager_space(managed_space_id: u64) -> RawSpaceRecord {
        RawSpaceRecord {
            managed_space_id,
            space_type: DESKTOP_SPACE_TYPE,
            tile_spaces: Vec::new(),
            has_tile_layout_manager: false,
            stage_manager_managed: true,
        }
    }

    fn fake_context_with_spaces() -> MacosNativeContext<FakeNativeApi> {
        MacosNativeContext::connect_with_api(FakeNativeApi::default()).unwrap()
    }

    fn fake_context_with_active_window(window_id: u64) -> MacosNativeContext<FakeNativeApi> {
        let topology = FakeNativeApi::topology_fixture(window_id);
        let api = FakeNativeApi::default().with_topology(topology);
        MacosNativeContext::connect_with_api(api).unwrap()
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

        assert!(spaces
            .iter()
            .any(|space| space.kind == SpaceKind::Desktop && space.is_active));
        assert!(spaces
            .iter()
            .any(|space| space.kind == SpaceKind::SplitView));
    }

    #[test]
    fn focused_window_comes_from_active_space_snapshot() {
        let ctx = fake_context_with_active_window(42);
        let focused = ctx.focused_window().unwrap();
        assert_eq!(focused.id, 42);
        assert_eq!(focused.space_id, 1);
    }

    #[test]
    fn active_space_snapshot_ordered_window_ids_match_window_ordering_contract() {
        let topology = RawTopologySnapshot {
            spaces: vec![raw_desktop_space(1)],
            active_space_id: 1,
            active_space_windows: vec![
                raw_window(11).with_visible_index(1),
                raw_window(12).with_visible_index(0),
                raw_window(13).with_level(5),
            ],
            inactive_space_window_ids: HashMap::new(),
        };

        let spaces = space_snapshots_from_topology(&topology);
        let active = spaces.iter().find(|space| space.is_active).unwrap();
        let windows = window_snapshots_from_topology(&topology);
        let ordered_window_ids_from_windows = windows
            .iter()
            .filter(|window| window.space_id == topology.active_space_id)
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
}
