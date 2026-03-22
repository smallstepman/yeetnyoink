use std::{
    collections::{HashMap, HashSet},
    ffi::{c_char, c_int, c_void, CStr, CString},
    fmt,
    ptr::{self, NonNull},
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
type CGWindowID = u32;
type CGWindowListOption = u32;
type CFArrayRetainCallBack = unsafe extern "C" fn(CFAllocatorRef, *const c_void) -> *const c_void;
type CFArrayReleaseCallBack = unsafe extern "C" fn(CFAllocatorRef, *const c_void);
type CFArrayCopyDescriptionCallBack = unsafe extern "C" fn(*const c_void) -> CFStringRef;
type CFArrayEqualCallBack = unsafe extern "C" fn(*const c_void, *const c_void) -> Boolean;

const K_CF_NUMBER_SINT64_TYPE: CFNumberType = 4;
const K_CF_STRING_ENCODING_UTF8: CFStringEncoding = 0x0800_0100;
const K_CG_NULL_WINDOW_ID: CGWindowID = 0;
const K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY: CGWindowListOption = 1 << 0;
const K_CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS: CGWindowListOption = 1 << 4;

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

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    static kCGWindowNumber: CFStringRef;
    static kCGWindowOwnerPID: CFStringRef;
    static kCGWindowName: CFStringRef;
    static kCGWindowLayer: CFStringRef;

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
type SlsCopyManagedDisplaySpacesFn = unsafe extern "C" fn(u32) -> CFArrayRef;
#[allow(dead_code)]
type SlsManagedDisplayGetCurrentSpaceFn = unsafe extern "C" fn(u32, CFStringRef) -> u64;
#[allow(dead_code)]
type SlsCopyWindowsWithOptionsAndTagsFn =
    unsafe extern "C" fn(u32, u32, CFArrayRef, i32, *mut i64, *mut i64) -> CFArrayRef;

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

#[allow(dead_code)]
pub(crate) trait MacosNativeApi {
    fn has_symbol(&self, symbol: &'static str) -> bool;
    fn ax_is_trusted(&self) -> bool;
    fn minimal_topology_ready(&self) -> bool;
    fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError>;
    fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError>;
    fn active_space_windows(&self, space_id: u64) -> Result<Vec<RawWindow>, MacosNativeProbeError>;
    fn inactive_space_window_ids(&self) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError>;

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
        let descriptions = unsafe {
            CfOwned::from_create_rule(CGWindowListCreateDescriptionFromArray(
                payload.as_type_ref() as CFArrayRef,
            ))
        }
        .ok_or(MacosNativeProbeError::MissingTopology(
            "CGWindowListCreateDescriptionFromArray",
        ))?;

        parse_window_descriptions(descriptions.as_type_ref() as CFArrayRef, &visible_order)
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

    fn topology_snapshot(&self) -> Result<RawTopologySnapshot, MacosNativeProbeError> {
        let payload = self.copy_managed_display_spaces_raw()?;
        let payload = payload.as_type_ref() as CFArrayRef;
        let spaces = parse_managed_spaces(payload)?;
        let active_space_ids = parse_active_space_ids(payload)?;
        let mut active_space_windows = HashMap::new();
        let mut inactive_space_window_ids = HashMap::new();

        for space in &spaces {
            let payload = self.copy_windows_for_space_raw(space.managed_space_id)?;

            if active_space_ids.contains(&space.managed_space_id) {
                let visible_order = query_visible_window_order(&parse_window_ids(
                    payload.as_type_ref() as CFArrayRef,
                )?)?;
                let descriptions = unsafe {
                    CfOwned::from_create_rule(CGWindowListCreateDescriptionFromArray(
                        payload.as_type_ref() as CFArrayRef,
                    ))
                }
                .ok_or(MacosNativeProbeError::MissingTopology(
                    "CGWindowListCreateDescriptionFromArray",
                ))?;

                active_space_windows.insert(
                    space.managed_space_id,
                    parse_window_descriptions(
                        descriptions.as_type_ref() as CFArrayRef,
                        &visible_order,
                    )?,
                );
            } else {
                inactive_space_window_ids.insert(
                    space.managed_space_id,
                    parse_window_ids(payload.as_type_ref() as CFArrayRef)?,
                );
            }
        }

        Ok(RawTopologySnapshot {
            spaces,
            active_space_ids,
            active_space_windows,
            inactive_space_window_ids,
        })
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
pub(crate) struct RawTopologySnapshot {
    spaces: Vec<RawSpaceRecord>,
    active_space_ids: HashSet<u64>,
    active_space_windows: HashMap<u64, Vec<RawWindow>>,
    inactive_space_window_ids: HashMap<u64, Vec<u64>>,
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

    for display in cf_array_iter(payload) {
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
            spaces.push(parse_raw_space_record(space)?);
        }
    }

    Ok(spaces)
}

fn parse_raw_space_record(space: CFDictionaryRef) -> Result<RawSpaceRecord, MacosNativeProbeError> {
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
    let onscreen_descriptions = unsafe {
        CfOwned::from_create_rule(CGWindowListCopyWindowInfo(
            K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY | K_CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS,
            K_CG_NULL_WINDOW_ID,
        ))
    }
    .ok_or(MacosNativeProbeError::MissingTopology(
        "CGWindowListCopyWindowInfo",
    ))?;
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
            app_id: stable_app_id_from_real_window(pid, None),
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
fn stable_app_id_from_real_window(pid: Option<u32>, _owner_name: Option<&str>) -> Option<String> {
    pid.and_then(stable_app_id_from_pid)
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
        .active_space_windows
        .iter()
        .flat_map(|(space_id, windows)| windows.iter().cloned().map(|window| (*space_id, window)))
        .min_by(|(_, left), (_, right)| compare_active_windows(left, right))
        .map(|(space_id, window)| (space_id, window.id))
        .ok_or(MacosNativeProbeError::MissingFocusedWindow)?;

    window_snapshots_from_topology(topology)
        .into_iter()
        .find(|window| (window.space_id, window.id) == focused_window_id)
        .ok_or(MacosNativeProbeError::MissingFocusedWindow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

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
                active_space_ids: HashSet::from([1]),
                active_space_windows: HashMap::from([(
                    1,
                    vec![raw_window(active_window_id)
                        .with_visible_index(0)
                        .with_pid(4242)
                        .with_app_id("com.example.focused")
                        .with_title("Focused window")],
                )]),
                inactive_space_window_ids: HashMap::from([(2, vec![21, 22])]),
            }
        }

        fn multi_display_topology_fixture() -> RawTopologySnapshot {
            RawTopologySnapshot {
                spaces: vec![
                    raw_desktop_space(1),
                    raw_split_space(2, &[21, 22]),
                    raw_fullscreen_space(3),
                ],
                active_space_ids: HashSet::from([1, 3]),
                active_space_windows: HashMap::from([
                    (
                        1,
                        vec![raw_window(11)
                            .with_visible_index(2)
                            .with_pid(1111)
                            .with_app_id("com.example.left")
                            .with_title("Left display")],
                    ),
                    (
                        3,
                        vec![raw_window(31)
                            .with_visible_index(0)
                            .with_pid(3333)
                            .with_app_id("com.example.right")
                            .with_title("Right display")],
                    ),
                ]),
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

        fn topology_snapshot(&self) -> Result<RawTopologySnapshot, MacosNativeProbeError> {
            Ok(self.topology.clone())
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

        let parsed = parse_raw_space_record(raw_space.as_type_ref() as CFDictionaryRef).unwrap();

        assert_eq!(parsed.managed_space_id, 7);
        assert_eq!(parsed.tile_spaces, vec![11, 12]);
        assert!(parsed.has_tile_layout_manager);
    }
}
