pub(crate) use macos_window_manager::{
    ActiveSpaceFocusTargetHint, MacosNativeApi, MacosNativeConnectError, MacosNativeOperationError,
    MacosNativeProbeError, MissionControlHotkey, MissionControlModifiers, NativeBackendOptions,
    NativeBounds, NativeDesktopSnapshot, NativeDiagnostics, NativeDirection, NativeSpaceSnapshot,
    NativeWindowId, NativeWindowSnapshot,
};
use std::{
    collections::{HashMap, HashSet},
    ffi::{CString, c_void},
    time::Instant,
};

pub(super) mod foundation {
    use super::{
        MacosNativeOperationError, MacosNativeProbeError, MissionControlHotkey,
        MissionControlModifiers, NativeBackendOptions, NativeDirection,
    };
    use core_foundation::{
        array::CFArray,
        base::{CFType, TCFType},
        dictionary::CFDictionary,
        number::CFNumber,
        string::CFString,
    };
    use std::{
        ffi::{CStr, c_char, c_int, c_void},
        ptr::NonNull,
        time::Duration,
    };

    pub(crate) const REQUIRED_PRIVATE_SYMBOLS: &[&str] = &[
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

    pub(crate) const SKYLIGHT_FRAMEWORK_PATH: &CStr =
        c"/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight";
    pub(crate) const HISERVICES_FRAMEWORK_PATH: &CStr =
        c"/System/Library/Frameworks/ApplicationServices.framework/Frameworks/HIServices.framework/HIServices";
    pub(crate) const RTLD_LAZY: c_int = 0x1;

    pub(crate) type Boolean = u8;
    pub(crate) type CFTypeRef = *const c_void;
    pub(crate) type CFArrayRef = *const c_void;
    pub(crate) type CFDictionaryRef = *const c_void;
    pub(crate) type CFStringRef = *const c_void;
    pub(crate) type CGEventFlags = u64;
    pub(crate) type CGEventTapLocation = u32;
    pub(crate) type CGKeyCode = u16;
    pub(crate) type CGWindowID = u32;
    pub(crate) type CGWindowListOption = u32;
    pub(crate) type OSStatus = i32;
    type UntypedCFArray = CFArray;
    type UntypedCFDictionary = CFDictionary;

    pub(crate) const K_CG_NULL_WINDOW_ID: CGWindowID = 0;
    pub(crate) const K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY: CGWindowListOption = 1 << 0;
    pub(crate) const K_CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS: CGWindowListOption = 1 << 4;
    pub(crate) const K_CG_HID_EVENT_TAP: CGEventTapLocation = 0;
    pub(crate) const K_CG_EVENT_FLAG_MASK_SHIFT: CGEventFlags = 1 << 17;
    pub(crate) const K_CG_EVENT_FLAG_MASK_CONTROL: CGEventFlags = 1 << 18;
    pub(crate) const K_CG_EVENT_FLAG_MASK_ALTERNATE: CGEventFlags = 1 << 19;
    pub(crate) const K_CG_EVENT_FLAG_MASK_COMMAND: CGEventFlags = 1 << 20;
    pub(crate) const K_CG_EVENT_FLAG_MASK_SECONDARY_FN: CGEventFlags = 1 << 23;
    pub(crate) const CPS_USER_GENERATED: u32 = 0x200;
    pub(crate) const SPACE_SWITCH_SETTLE_TIMEOUT: Duration = Duration::from_millis(300);
    pub(crate) const SPACE_SWITCH_POLL_INTERVAL: Duration = Duration::from_millis(10);
    pub(crate) const SPACE_SWITCH_STABLE_TARGET_POLLS: usize = 3;
    pub(crate) const AX_RAISE_SETTLE_TIMEOUT: Duration = Duration::from_millis(300);
    pub(crate) const AX_RAISE_RETRY_INTERVAL: Duration = Duration::from_millis(10);

    pub(crate) type SlsMainConnectionIdFn = unsafe extern "C" fn() -> u32;
    pub(crate) type SlsCopyManagedDisplaySpacesFn = unsafe extern "C" fn(u32) -> CFArrayRef;
    pub(crate) type SlsManagedDisplayGetCurrentSpaceFn =
        unsafe extern "C" fn(u32, CFStringRef) -> u64;
    pub(crate) type SlsManagedDisplaySetCurrentSpaceFn =
        unsafe extern "C" fn(u32, CFStringRef, u64);
    pub(crate) type SlsCopyManagedDisplayForSpaceFn = unsafe extern "C" fn(u32, u64) -> CFStringRef;
    pub(crate) type SlsCopyWindowsWithOptionsAndTagsFn =
        unsafe extern "C" fn(u32, u32, CFArrayRef, i32, *mut i64, *mut i64) -> CFArrayRef;
    pub(crate) type SlpsSetFrontProcessWithOptionsFn =
        unsafe extern "C" fn(*const ProcessSerialNumber, CGWindowID, u32) -> OSStatus;
    pub(crate) type SlpsPostEventRecordToFn =
        unsafe extern "C" fn(*const ProcessSerialNumber, *const c_void) -> OSStatus;
    pub(crate) type GetProcessForPidFn =
        unsafe extern "C" fn(c_int, *mut ProcessSerialNumber) -> OSStatus;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        pub(crate) fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
        pub(crate) fn CFRelease(cf: CFTypeRef);
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        pub(crate) fn CGEventCreateKeyboardEvent(
            source: CFTypeRef,
            virtual_key: CGKeyCode,
            key_down: Boolean,
        ) -> CFTypeRef;
        pub(crate) fn CGEventSetFlags(event: CFTypeRef, flags: CGEventFlags);
        pub(crate) fn CGEventPost(tap: CGEventTapLocation, event: CFTypeRef);
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        pub(crate) fn CGWindowListCreateDescriptionFromArray(
            window_array: CFArrayRef,
        ) -> CFArrayRef;
    }

    #[repr(C)]
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub(crate) struct ProcessSerialNumber {
        pub(crate) high_long_of_psn: u32,
        pub(crate) low_long_of_psn: u32,
    }

    unsafe extern "C" {
        pub(crate) fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
        pub(crate) fn dlclose(handle: *mut c_void) -> c_int;
        pub(crate) fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    }

    #[derive(Debug)]
    pub(crate) struct DylibHandle {
        raw: *mut c_void,
    }

    // The handle is only used behind immutable method calls and closed on drop.
    // We do not share aliasing Rust references into the loaded dylib state itself.
    unsafe impl Send for DylibHandle {}

    impl DylibHandle {
        pub(crate) fn open(path: &CStr) -> Option<Self> {
            let raw = unsafe { dlopen(path.as_ptr(), RTLD_LAZY) };
            if raw.is_null() {
                None
            } else {
                Some(Self { raw })
            }
        }

        pub(crate) fn resolve(&self, symbol: &CStr) -> Option<*mut c_void> {
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

    pub(crate) struct CfOwned {
        raw: NonNull<c_void>,
    }

    impl CfOwned {
        pub(crate) unsafe fn from_create_rule(raw: CFTypeRef) -> Option<Self> {
            NonNull::new(raw.cast_mut()).map(|raw| Self { raw })
        }

        pub(crate) fn from_servo<T: TCFType>(value: T) -> Self {
            // Transfer ownership from the Servo wrapper into our generic CF owner.
            let raw = value.as_CFTypeRef();
            std::mem::forget(value);
            unsafe { Self::from_create_rule(raw) }.expect("Servo CF wrappers should never be null")
        }

        pub(crate) fn as_type_ref(&self) -> CFTypeRef {
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

    pub(crate) struct ArrayIter {
        array: Option<CFArray<CFType>>,
        index: usize,
    }

    impl Iterator for ArrayIter {
        type Item = CFTypeRef;

        fn next(&mut self) -> Option<Self::Item> {
            let array = self.array.as_ref()?;
            let value = array.get(self.index as _)?;
            self.index += 1;
            Some(value.as_CFTypeRef())
        }
    }
    fn mission_control_shortcut_flags(shortcut: &MissionControlModifiers) -> CGEventFlags {
        let mut flags = 0;
        if shortcut.shift {
            flags |= K_CG_EVENT_FLAG_MASK_SHIFT;
        }
        if shortcut.control {
            flags |= K_CG_EVENT_FLAG_MASK_CONTROL;
        }
        if shortcut.option {
            flags |= K_CG_EVENT_FLAG_MASK_ALTERNATE;
        }
        if shortcut.command {
            flags |= K_CG_EVENT_FLAG_MASK_COMMAND;
        }
        if shortcut.function {
            flags |= K_CG_EVENT_FLAG_MASK_SECONDARY_FN;
        }
        flags
    }

    fn mission_control_shortcut(
        options: &NativeBackendOptions,
        direction: NativeDirection,
    ) -> Result<MissionControlHotkey, MacosNativeOperationError> {
        match direction {
            NativeDirection::West => Ok(options.west_space_hotkey),
            NativeDirection::East => Ok(options.east_space_hotkey),
            NativeDirection::North | NativeDirection::South => Err(
                MacosNativeOperationError::CallFailed("adjacent_space_hotkey_direction"),
            ),
        }
    }

    fn configured_mission_control_shortcut(
        options: &NativeBackendOptions,
        direction: NativeDirection,
    ) -> Result<(CGKeyCode, CGEventFlags), MacosNativeOperationError> {
        let shortcut = mission_control_shortcut(options, direction)?;
        Ok((
            shortcut.key_code as CGKeyCode,
            mission_control_shortcut_flags(&shortcut.mission_control),
        ))
    }

    pub(crate) fn switch_adjacent_space_via_hotkey<PostKeyEvent>(
        options: &NativeBackendOptions,
        direction: NativeDirection,
        mut post_key_event: PostKeyEvent,
    ) -> Result<(), MacosNativeOperationError>
    where
        PostKeyEvent: FnMut(CGKeyCode, bool, CGEventFlags) -> Result<(), MacosNativeOperationError>,
    {
        let (key_code, flags) = configured_mission_control_shortcut(options, direction)?;

        post_key_event(key_code, true, flags)?;
        post_key_event(key_code, false, flags)
    }

    fn cf_type(value: CFTypeRef) -> Option<CFType> {
        (!value.is_null()).then(|| unsafe { CFType::wrap_under_get_rule(value) })
    }

    fn typed_array(array: CFArrayRef) -> Option<CFArray<CFType>> {
        let cf_type = cf_type(array as CFTypeRef)?;
        cf_type
            .instance_of::<UntypedCFArray>()
            .then(|| unsafe { CFArray::<CFType>::wrap_under_get_rule(array as _) })
    }

    fn typed_dictionary(dictionary: CFDictionaryRef) -> Option<CFDictionary<CFType, CFType>> {
        let cf_type = cf_type(dictionary as CFTypeRef)?;
        cf_type
            .instance_of::<UntypedCFDictionary>()
            .then(|| unsafe {
                CFDictionary::<CFType, CFType>::wrap_under_get_rule(dictionary as _)
            })
    }

    pub(crate) fn array_len(array: CFArrayRef) -> usize {
        typed_array(array)
            .map(|array| array.len() as usize)
            .unwrap_or_default()
    }

    pub(crate) fn array_iter(array: CFArrayRef) -> ArrayIter {
        ArrayIter {
            array: typed_array(array),
            index: 0,
        }
    }

    pub(crate) fn as_dictionary(value: CFTypeRef) -> Option<CFDictionaryRef> {
        let cf_type = cf_type(value)?;
        cf_type
            .instance_of::<UntypedCFDictionary>()
            .then_some(value as CFDictionaryRef)
    }

    pub(crate) fn string(value: &str) -> CFString {
        CFString::new(value)
    }

    pub(crate) fn number_from_u64(value: u64) -> Result<CFNumber, MacosNativeProbeError> {
        let value = i64::try_from(value).map_err(|_| {
            MacosNativeProbeError::MissingTopology("SLSCopyWindowsWithOptionsAndTags")
        })?;
        Ok(CFNumber::from(value))
    }

    pub(crate) fn array_from_u64s(
        values: &[u64],
    ) -> Result<CFArray<CFNumber>, MacosNativeProbeError> {
        let numbers = values
            .iter()
            .map(|value| number_from_u64(*value))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(CFArray::from_CFTypes(&numbers))
    }

    pub(crate) fn array_from_type_refs(values: &[CFTypeRef]) -> CFArray<CFType> {
        let values = values
            .iter()
            .map(|value| cf_type(*value).expect("array_from_type_refs expects non-null CFTypeRef"))
            .collect::<Vec<_>>();
        CFArray::from_CFTypes(&values)
    }

    pub(crate) fn number_to_i64(value: CFTypeRef) -> Option<i64> {
        cf_type(value)?
            .downcast::<CFNumber>()
            .and_then(|number| number.to_i64())
    }

    fn dictionary_value(dictionary: CFDictionaryRef, key: &CFString) -> Option<CFType> {
        let dictionary = typed_dictionary(dictionary)?;
        dictionary
            .find(key.as_CFTypeRef())
            .map(|value| value.clone())
    }

    pub(crate) fn dictionary_string(dictionary: CFDictionaryRef, key: &CFString) -> Option<String> {
        dictionary_value(dictionary, key)?
            .downcast::<CFString>()
            .map(|value| value.to_string())
    }

    pub(crate) fn dictionary_u64(dictionary: CFDictionaryRef, key: &CFString) -> Option<u64> {
        dictionary_value(dictionary, key)?
            .downcast::<CFNumber>()
            .and_then(|number| number.to_i64())
            .and_then(|value| u64::try_from(value).ok())
    }

    pub(crate) fn dictionary_u32(dictionary: CFDictionaryRef, key: &CFString) -> Option<u32> {
        dictionary_value(dictionary, key)?
            .downcast::<CFNumber>()
            .and_then(|number| number.to_i64())
            .and_then(|value| u32::try_from(value).ok())
    }

    pub(crate) fn dictionary_i32(dictionary: CFDictionaryRef, key: &CFString) -> Option<i32> {
        dictionary_value(dictionary, key)?
            .downcast::<CFNumber>()
            .and_then(|number| number.to_i64())
            .and_then(|value| i32::try_from(value).ok())
    }

    pub(crate) fn dictionary_array(
        dictionary: CFDictionaryRef,
        key: &CFString,
    ) -> Option<CFArrayRef> {
        let value = dictionary_value(dictionary, key)?;
        value
            .instance_of::<UntypedCFArray>()
            .then_some(value.as_CFTypeRef() as CFArrayRef)
    }

    pub(crate) fn dictionary_dictionary(
        dictionary: CFDictionaryRef,
        key: &CFString,
    ) -> Option<CFDictionaryRef> {
        let value = dictionary_value(dictionary, key)?;
        value
            .instance_of::<UntypedCFDictionary>()
            .then_some(value.as_CFTypeRef() as CFDictionaryRef)
    }

    pub(crate) fn cf_array_count(array: CFArrayRef) -> usize {
        array_len(array)
    }

    pub(crate) fn cf_array_iter(array: CFArrayRef) -> impl Iterator<Item = CFTypeRef> {
        array_iter(array)
    }

    pub(crate) fn cf_as_dictionary(value: CFTypeRef) -> Option<CFDictionaryRef> {
        as_dictionary(value)
    }

    pub(crate) fn cf_string(value: &str) -> Result<CfOwned, MacosNativeProbeError> {
        if value.as_bytes().contains(&0) {
            return Err(MacosNativeProbeError::MissingTopology(
                "CFStringCreateWithCString",
            ));
        }

        Ok(CfOwned::from_servo(string(value)))
    }

    pub(crate) fn cf_number_from_u64(value: u64) -> Result<CfOwned, MacosNativeProbeError> {
        number_from_u64(value).map(CfOwned::from_servo)
    }

    pub(crate) fn cf_number_to_i64(number: CFTypeRef) -> Option<i64> {
        number_to_i64(number)
    }

    pub(crate) fn cf_number_to_u64(number: CFTypeRef) -> Option<u64> {
        cf_number_to_i64(number).and_then(|value| u64::try_from(value).ok())
    }

    pub(crate) fn cf_dictionary_string(
        dictionary: CFDictionaryRef,
        key: CFStringRef,
    ) -> Option<String> {
        let key = unsafe {
            core_foundation::string::CFString::wrap_under_get_rule(
                key as core_foundation::string::CFStringRef,
            )
        };
        dictionary_string(dictionary, &key)
    }

    pub(crate) fn cf_dictionary_u64(dictionary: CFDictionaryRef, key: CFStringRef) -> Option<u64> {
        let key = unsafe {
            core_foundation::string::CFString::wrap_under_get_rule(
                key as core_foundation::string::CFStringRef,
            )
        };
        dictionary_u64(dictionary, &key)
    }

    pub(crate) fn cf_dictionary_u32(dictionary: CFDictionaryRef, key: CFStringRef) -> Option<u32> {
        let key = unsafe {
            core_foundation::string::CFString::wrap_under_get_rule(
                key as core_foundation::string::CFStringRef,
            )
        };
        dictionary_u32(dictionary, &key)
    }

    pub(crate) fn cf_dictionary_i32(dictionary: CFDictionaryRef, key: CFStringRef) -> Option<i32> {
        let key = unsafe {
            core_foundation::string::CFString::wrap_under_get_rule(
                key as core_foundation::string::CFStringRef,
            )
        };
        dictionary_i32(dictionary, &key)
    }

    pub(crate) fn cf_dictionary_array(
        dictionary: CFDictionaryRef,
        key: CFStringRef,
    ) -> Option<CFArrayRef> {
        let key = unsafe {
            core_foundation::string::CFString::wrap_under_get_rule(
                key as core_foundation::string::CFStringRef,
            )
        };
        dictionary_array(dictionary, &key)
    }

    pub(crate) fn cf_dictionary_dictionary(
        dictionary: CFDictionaryRef,
        key: CFStringRef,
    ) -> Option<CFDictionaryRef> {
        let key = unsafe {
            core_foundation::string::CFString::wrap_under_get_rule(
                key as core_foundation::string::CFStringRef,
            )
        };
        dictionary_dictionary(dictionary, &key)
    }

    #[cfg(test)]
    pub(super) mod tests {
        use super::*;

        pub(crate) fn dictionary_from_type_refs(
            entries: &[(CFTypeRef, CFTypeRef)],
        ) -> CFDictionary<CFType, CFType> {
            let entries = entries
                .iter()
                .map(|(key, value)| {
                    (
                        cf_type(*key).expect("dictionary_from_type_refs expects non-null keys"),
                        cf_type(*value).expect("dictionary_from_type_refs expects non-null values"),
                    )
                })
                .collect::<Vec<_>>();
            CFDictionary::from_CFType_pairs(&entries)
        }
    }
}

pub(super) mod ax {
    use super::foundation::{
        CFArrayRef, CFRetain, CFTypeRef, CfOwned, OSStatus, cf_array_iter, cf_string,
    };
    use super::window_server;
    use super::{
        MacosNativeApi, MacosNativeOperationError, MacosNativeProbeError, NativeBounds,
        RealNativeApi,
    };
    use std::{
        ffi::{c_int, c_void},
        ptr,
    };

    pub(crate) type AXUIElementRef = *const c_void;
    pub(crate) type AXValueType = u32;

    pub(crate) const K_AX_VALUE_TYPE_CGPOINT: AXValueType = 1;
    pub(crate) const K_AX_VALUE_TYPE_CGSIZE: AXValueType = 2;

    type AxIsProcessTrustedFn = unsafe extern "C" fn() -> u8;
    type AxUiElementGetWindowFn = unsafe extern "C" fn(AXUIElementRef, *mut u32) -> OSStatus;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub(crate) struct CGPoint {
        pub(crate) x: f64,
        pub(crate) y: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub(crate) struct CGSize {
        pub(crate) width: f64,
        pub(crate) height: f64,
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        pub(crate) fn AXUIElementCreateApplication(pid: c_int) -> AXUIElementRef;
        pub(crate) fn AXUIElementCreateSystemWide() -> AXUIElementRef;
        pub(crate) fn AXUIElementCopyAttributeValue(
            element: AXUIElementRef,
            attribute: *const c_void,
            value: *mut CFTypeRef,
        ) -> OSStatus;
        pub(crate) fn AXUIElementPerformAction(
            element: AXUIElementRef,
            action: *const c_void,
        ) -> OSStatus;
        pub(crate) fn AXUIElementSetAttributeValue(
            element: AXUIElementRef,
            attribute: *const c_void,
            value: CFTypeRef,
        ) -> OSStatus;
        #[allow(dead_code)]
        pub(crate) fn AXUIElementGetPid(element: AXUIElementRef, pid: *mut c_int) -> OSStatus;
        pub(crate) fn AXValueCreate(value_type: AXValueType, value_ptr: *const c_void)
        -> CFTypeRef;
    }

    pub(crate) fn is_process_trusted(api: &RealNativeApi) -> bool {
        let Some(symbol) = api.resolve_symbol("AXIsProcessTrusted") else {
            return false;
        };

        let ax_is_process_trusted: AxIsProcessTrustedFn = unsafe { std::mem::transmute(symbol) };
        unsafe { ax_is_process_trusted() != 0 }
    }

    pub(crate) fn focused_window_id<App, Window, FocusedApplication, FocusedWindow, WindowId>(
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

    pub(crate) fn copy_system_wide_ax_element(
        _api: &RealNativeApi,
    ) -> Result<CfOwned, MacosNativeProbeError> {
        let _span = tracing::debug_span!("macos_native.ax.system_wide_element").entered();
        unsafe { CfOwned::from_create_rule(AXUIElementCreateSystemWide() as CFTypeRef) }.ok_or(
            MacosNativeProbeError::MissingTopology("AXUIElementCreateSystemWide"),
        )
    }

    pub(crate) fn copy_ax_attribute_value(
        _api: &RealNativeApi,
        element: AXUIElementRef,
        attribute_name: &str,
    ) -> Result<Option<CfOwned>, MacosNativeProbeError> {
        let _span = tracing::debug_span!(
            "macos_native.ax.copy_attribute_value",
            attribute = attribute_name
        )
        .entered();
        let attribute = cf_string(attribute_name)?;
        let mut value = ptr::null();
        let status =
            unsafe { AXUIElementCopyAttributeValue(element, attribute.as_type_ref(), &mut value) };

        if status != 0 {
            return Ok(None);
        }

        Ok(unsafe { CfOwned::from_create_rule(value) })
    }

    pub(crate) fn copy_focused_application_ax(
        api: &RealNativeApi,
    ) -> Result<Option<CfOwned>, MacosNativeProbeError> {
        let system = copy_system_wide_ax_element(api)?;
        copy_ax_attribute_value(
            api,
            system.as_type_ref() as AXUIElementRef,
            "AXFocusedApplication",
        )
    }

    pub(crate) fn copy_focused_window_ax(
        api: &RealNativeApi,
        application: &CfOwned,
    ) -> Result<Option<CfOwned>, MacosNativeProbeError> {
        copy_ax_attribute_value(
            api,
            application.as_type_ref() as AXUIElementRef,
            "AXFocusedWindow",
        )
    }

    #[allow(dead_code)]
    pub(crate) fn ax_pid(
        _api: &RealNativeApi,
        element: &CfOwned,
    ) -> Result<u32, MacosNativeProbeError> {
        let mut pid = 0;
        let status =
            unsafe { AXUIElementGetPid(element.as_type_ref() as AXUIElementRef, &mut pid) };
        if status != 0 || pid <= 0 {
            return Err(MacosNativeProbeError::MissingFocusedWindow);
        }
        Ok(pid as u32)
    }

    pub(crate) fn ax_window_id(
        api: &RealNativeApi,
        element: &CfOwned,
    ) -> Result<u64, MacosNativeProbeError> {
        let Some(symbol) = api.resolve_symbol("_AXUIElementGetWindow") else {
            return Err(MacosNativeProbeError::MissingTopology(
                "_AXUIElementGetWindow",
            ));
        };
        let ax_ui_element_get_window: AxUiElementGetWindowFn =
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

    pub(crate) fn probe_focused_window_id(
        api: &RealNativeApi,
    ) -> Result<Option<u64>, MacosNativeProbeError> {
        focused_window_id(
            || {
                let _span = tracing::debug_span!("macos_native.ax.focused_application").entered();
                copy_focused_application_ax(api)
            },
            |application| {
                let _span = tracing::debug_span!("macos_native.ax.focused_window").entered();
                copy_focused_window_ax(api, application)
            },
            |window| {
                let _span = tracing::debug_span!("macos_native.ax.window_id").entered();
                ax_window_id(api, window)
            },
        )
    }

    pub(crate) fn copy_application_ax_element(
        _api: &RealNativeApi,
        pid: u32,
    ) -> Result<CfOwned, MacosNativeOperationError> {
        unsafe {
            CfOwned::from_create_rule(AXUIElementCreateApplication(pid as c_int) as CFTypeRef)
        }
        .ok_or(MacosNativeOperationError::CallFailed(
            "AXUIElementCreateApplication",
        ))
    }

    pub(crate) fn copy_window_ax_for_id(
        api: &RealNativeApi,
        pid: u32,
        window_id: u64,
    ) -> Result<CfOwned, MacosNativeOperationError> {
        let application = copy_application_ax_element(api, pid)?;
        let windows = copy_ax_attribute_value(
            api,
            application.as_type_ref() as AXUIElementRef,
            "AXWindows",
        )
        .map_err(MacosNativeOperationError::from)?
        .ok_or(MacosNativeOperationError::MissingWindow(window_id))?;
        let windows = windows.as_type_ref() as CFArrayRef;

        for candidate in cf_array_iter(windows) {
            let Some(candidate) = (unsafe { CfOwned::from_create_rule(CFRetain(candidate)) })
            else {
                continue;
            };
            if ax_window_id(api, &candidate).ok() == Some(window_id) {
                return Ok(candidate);
            }
        }

        let ax_window_ids = cf_array_iter(windows)
            .filter_map(|candidate| {
                let candidate = unsafe { CfOwned::from_create_rule(CFRetain(candidate)) }?;
                ax_window_id(api, &candidate).ok()
            })
            .collect::<Vec<_>>();
        api.debug(&format!(
            "macos_native: target window {window_id} missing from AXWindows for pid {pid}; ax_window_ids={ax_window_ids:?} focused_window_id={:?}",
            MacosNativeApi::focused_window_id(api).ok().flatten()
        ));
        Err(MacosNativeOperationError::MissingWindow(window_id))
    }

    pub(crate) fn ax_window_ids_for_pid(
        api: &RealNativeApi,
        pid: u32,
    ) -> Result<Vec<u64>, MacosNativeOperationError> {
        let application = copy_application_ax_element(api, pid)?;
        let Some(windows) = copy_ax_attribute_value(
            api,
            application.as_type_ref() as AXUIElementRef,
            "AXWindows",
        )
        .map_err(MacosNativeOperationError::from)?
        else {
            return Ok(Vec::new());
        };
        let windows = windows.as_type_ref() as CFArrayRef;

        Ok(cf_array_iter(windows)
            .filter_map(|candidate| {
                let candidate = unsafe { CfOwned::from_create_rule(CFRetain(candidate)) }?;
                ax_window_id(api, &candidate).ok()
            })
            .collect())
    }

    pub(crate) fn raise_window_via_ax(
        api: &RealNativeApi,
        window_id: u64,
        pid: u32,
    ) -> Result<(), MacosNativeOperationError> {
        let window = copy_window_ax_for_id(api, pid, window_id)?;
        let action = cf_string("AXRaise").map_err(MacosNativeOperationError::from)?;
        let status = unsafe {
            AXUIElementPerformAction(window.as_type_ref() as AXUIElementRef, action.as_type_ref())
        };

        (status == 0)
            .then_some(())
            .ok_or(MacosNativeOperationError::CallFailed(
                "AXUIElementPerformAction",
            ))
    }

    pub(crate) fn set_window_frame_via_ax(
        api: &RealNativeApi,
        window_id: u64,
        pid: u32,
        frame: NativeBounds,
    ) -> Result<(), MacosNativeOperationError> {
        let window = copy_window_ax_for_id(api, pid, window_id)?;
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
            width: f64::from(frame.width),
            height: f64::from(frame.height),
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

    pub(crate) fn swap_window_frames(
        api: &RealNativeApi,
        source_window_id: u64,
        source_frame: NativeBounds,
        target_window_id: u64,
        target_frame: NativeBounds,
    ) -> Result<(), MacosNativeOperationError> {
        let source = window_server::window_description(api, source_window_id)?;
        let source_pid = source
            .pid
            .ok_or(MacosNativeOperationError::MissingWindowPid(
                source_window_id,
            ))?;
        let target = window_server::window_description(api, target_window_id)?;
        let target_pid = target
            .pid
            .ok_or(MacosNativeOperationError::MissingWindowPid(
                target_window_id,
            ))?;

        set_window_frame_via_ax(api, source_window_id, source_pid, target_frame)?;
        set_window_frame_via_ax(api, target_window_id, target_pid, source_frame)
    }
}

pub(super) mod error {
    use super::NativeDirection;
    use thiserror::Error;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
    pub(crate) enum MacosNativeConnectError {
        #[error("required macOS private symbol is unavailable: {0}")]
        MissingRequiredSymbol(&'static str),
        #[error("Accessibility permission is required for macOS native support")]
        MissingAccessibilityPermission,
        #[error("macOS native topology precondition is unavailable: {0}")]
        MissingTopologyPrecondition(&'static str),
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
    pub(crate) enum MacosNativeProbeError {
        #[error("macOS native topology query is unavailable: {0}")]
        MissingTopology(&'static str),
        #[error("no focused window was found for any active Space")]
        MissingFocusedWindow,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
    pub(crate) enum MacosNativeOperationError {
        #[error(transparent)]
        Probe(#[from] MacosNativeProbeError),
        #[error("macOS native space {0} was not found in the current topology")]
        MissingSpace(u64),
        #[error("macOS native window {0} was not found in the current topology")]
        MissingWindow(u64),
        #[error("macOS native window {0} has no frame")]
        MissingWindowFrame(u64),
        #[error("macOS native window {0} does not expose an owner pid")]
        MissingWindowPid(u64),
        #[error("macOS native Stage Manager space {0} is intentionally unsupported")]
        UnsupportedStageManagerSpace(u64),
        #[error("macos_native: no window to focus {0}")]
        NoDirectionalFocusTarget(NativeDirection),
        #[error("macos_native: no window to move {0}")]
        NoDirectionalMoveTarget(NativeDirection),
        #[error("macOS native operation failed: {0}")]
        CallFailed(&'static str),
    }
}

pub(super) mod skylight {
    use super::foundation::{
        CFArrayRef, CFDictionaryRef, CFStringRef, CfOwned, SlsCopyManagedDisplayForSpaceFn,
        SlsCopyManagedDisplaySpacesFn, SlsCopyWindowsWithOptionsAndTagsFn, SlsMainConnectionIdFn,
        SlsManagedDisplayGetCurrentSpaceFn, SlsManagedDisplaySetCurrentSpaceFn,
        array_from_type_refs, cf_array_iter, cf_as_dictionary, cf_dictionary_array,
        cf_dictionary_dictionary, cf_dictionary_i32, cf_dictionary_string, cf_dictionary_u64,
        cf_number_from_u64, cf_number_to_u64, cf_string,
    };
    use super::{MacosNativeOperationError, MacosNativeProbeError, RawSpaceRecord, RealNativeApi};
    use std::collections::HashSet;

    pub(crate) fn main_connection_id(api: &RealNativeApi) -> Result<u32, MacosNativeProbeError> {
        let Some(symbol) = api.resolve_symbol("SLSMainConnectionID") else {
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

    pub(crate) fn copy_managed_display_spaces_raw(
        api: &RealNativeApi,
    ) -> Result<CfOwned, MacosNativeProbeError> {
        let Some(symbol) = api.resolve_symbol("SLSCopyManagedDisplaySpaces") else {
            return Err(MacosNativeProbeError::MissingTopology(
                "SLSCopyManagedDisplaySpaces",
            ));
        };

        let copy_managed_display_spaces: SlsCopyManagedDisplaySpacesFn =
            unsafe { std::mem::transmute(symbol) };
        let connection_id = main_connection_id(api)?;
        let payload =
            unsafe { CfOwned::from_create_rule(copy_managed_display_spaces(connection_id)) }
                .ok_or(MacosNativeProbeError::MissingTopology(
                    "SLSCopyManagedDisplaySpaces",
                ))?;

        Ok(payload)
    }

    pub(crate) fn current_space_for_display(
        api: &RealNativeApi,
        display_identifier: &str,
    ) -> Result<u64, MacosNativeProbeError> {
        let Some(symbol) = api.resolve_symbol("SLSManagedDisplayGetCurrentSpace") else {
            return Err(MacosNativeProbeError::MissingTopology(
                "SLSManagedDisplayGetCurrentSpace",
            ));
        };

        let current_space_for_display: SlsManagedDisplayGetCurrentSpaceFn =
            unsafe { std::mem::transmute(symbol) };
        let connection_id = main_connection_id(api)?;
        let display_identifier = cf_string(display_identifier)?;
        let space_id =
            unsafe { current_space_for_display(connection_id, display_identifier.as_type_ref()) };

        (space_id != 0)
            .then_some(space_id)
            .ok_or(MacosNativeProbeError::MissingTopology(
                "SLSManagedDisplayGetCurrentSpace",
            ))
    }

    pub(crate) fn copy_windows_for_space_raw(
        api: &RealNativeApi,
        space_id: u64,
    ) -> Result<CfOwned, MacosNativeProbeError> {
        let Some(symbol) = api.resolve_symbol("SLSCopyWindowsWithOptionsAndTags") else {
            return Err(MacosNativeProbeError::MissingTopology(
                "SLSCopyWindowsWithOptionsAndTags",
            ));
        };

        let copy_windows_with_options_and_tags: SlsCopyWindowsWithOptionsAndTagsFn =
            unsafe { std::mem::transmute(symbol) };
        let connection_id = main_connection_id(api)?;
        let space_number = cf_number_from_u64(space_id)?;
        let space_list = CfOwned::from_servo(array_from_type_refs(&[space_number.as_type_ref()]));
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

    pub(crate) fn copy_managed_display_for_space_raw(
        api: &RealNativeApi,
        space_id: u64,
    ) -> Result<CfOwned, MacosNativeOperationError> {
        let Some(symbol) = api.resolve_symbol("SLSCopyManagedDisplayForSpace") else {
            return Err(MacosNativeOperationError::CallFailed(
                "SLSCopyManagedDisplayForSpace",
            ));
        };

        let copy_managed_display_for_space: SlsCopyManagedDisplayForSpaceFn =
            unsafe { std::mem::transmute(symbol) };
        let connection_id = main_connection_id(api)?;
        let payload = unsafe {
            CfOwned::from_create_rule(copy_managed_display_for_space(connection_id, space_id))
        }
        .ok_or(MacosNativeOperationError::CallFailed(
            "SLSCopyManagedDisplayForSpace",
        ))?;

        Ok(payload)
    }

    pub(crate) fn switch_space(
        api: &RealNativeApi,
        space_id: u64,
    ) -> Result<(), MacosNativeOperationError> {
        let Some(symbol) = api.resolve_symbol("SLSManagedDisplaySetCurrentSpace") else {
            return Err(MacosNativeOperationError::CallFailed(
                "SLSManagedDisplaySetCurrentSpace",
            ));
        };

        let set_current_space: SlsManagedDisplaySetCurrentSpaceFn =
            unsafe { std::mem::transmute(symbol) };
        let connection_id = main_connection_id(api)?;
        let display_identifier = copy_managed_display_for_space_raw(api, space_id)?;

        unsafe {
            set_current_space(
                connection_id,
                display_identifier.as_type_ref() as CFStringRef,
                space_id,
            );
        }

        Ok(())
    }

    pub(crate) fn move_window_to_space(
        api: &RealNativeApi,
        window_id: u64,
        space_id: u64,
    ) -> Result<(), MacosNativeOperationError> {
        let Some(symbol) = api.resolve_symbol("SLSMoveWindowsToManagedSpace") else {
            return Err(MacosNativeOperationError::CallFailed(
                "SLSMoveWindowsToManagedSpace",
            ));
        };

        let move_windows_to_managed_space: unsafe extern "C" fn(u32, CFArrayRef, u64) =
            unsafe { std::mem::transmute(symbol) };
        let connection_id = main_connection_id(api)?;
        let window_number =
            cf_number_from_u64(window_id).map_err(MacosNativeOperationError::from)?;
        let window_list = CfOwned::from_servo(array_from_type_refs(&[window_number.as_type_ref()]));

        unsafe {
            move_windows_to_managed_space(
                connection_id,
                window_list.as_type_ref() as CFArrayRef,
                space_id,
            );
        }

        Ok(())
    }

    pub(crate) fn parse_display_identifiers(
        payload: CFArrayRef,
    ) -> Result<Vec<String>, MacosNativeProbeError> {
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

    pub(crate) fn parse_active_space_ids(
        payload: CFArrayRef,
    ) -> Result<HashSet<u64>, MacosNativeProbeError> {
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
                    .or_else(|| {
                        cf_dictionary_u64(display, current_managed_space_id_key.as_type_ref())
                    })
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

    pub(crate) fn parse_managed_spaces(
        payload: CFArrayRef,
    ) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
        let spaces_key = cf_string("Spaces")?;
        let mut spaces = Vec::new();

        for (display_index, display) in cf_array_iter(payload).enumerate() {
            let display = cf_as_dictionary(display).ok_or(
                MacosNativeProbeError::MissingTopology("SLSCopyManagedDisplaySpaces"),
            )?;
            let display_spaces =
                cf_dictionary_array(display, spaces_key.as_type_ref() as CFStringRef).ok_or(
                    MacosNativeProbeError::MissingTopology("SLSCopyManagedDisplaySpaces"),
                )?;

            for space in cf_array_iter(display_spaces) {
                let space = cf_as_dictionary(space).ok_or(
                    MacosNativeProbeError::MissingTopology("SLSCopyManagedDisplaySpaces"),
                )?;
                spaces.push(parse_raw_space_record(space, display_index)?);
            }
        }

        Ok(spaces)
    }

    pub(crate) fn parse_raw_space_record(
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

    pub(crate) fn parse_window_ids(payload: CFArrayRef) -> Result<Vec<u64>, MacosNativeProbeError> {
        cf_array_iter(payload)
            .map(|window_id| {
                cf_number_to_u64(window_id).ok_or(MacosNativeProbeError::MissingTopology(
                    "SLSCopyWindowsWithOptionsAndTags",
                ))
            })
            .collect()
    }
}

pub(super) mod window_server {
    use super::ax;
    use super::foundation::{
        CFArrayRef, CFDictionaryRef, CFStringRef, CGEventCreateKeyboardEvent, CGEventFlags,
        CGEventPost, CGEventSetFlags, CGKeyCode, CGWindowID,
        CGWindowListCreateDescriptionFromArray, CPS_USER_GENERATED, CfOwned, GetProcessForPidFn,
        K_CG_HID_EVENT_TAP, K_CG_NULL_WINDOW_ID, K_CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS,
        K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY, ProcessSerialNumber, SlpsPostEventRecordToFn,
        SlpsSetFrontProcessWithOptionsFn, array_from_type_refs, cf_array_count, cf_array_iter,
        cf_as_dictionary, cf_dictionary_dictionary, cf_dictionary_i32, cf_dictionary_string,
        cf_dictionary_u32, cf_dictionary_u64, cf_number_from_u64, cf_string,
    };
    use super::skylight;
    use super::{
        MacosNativeOperationError, MacosNativeProbeError, NativeBounds, RawWindow, RealNativeApi,
        enrich_real_window_app_ids, focus_window_via_process_and_raise,
    };
    use std::{
        collections::{HashMap, HashSet},
        ffi::{c_int, c_void},
        ptr,
    };

    pub(crate) fn process_serial_number_for_pid(
        api: &RealNativeApi,
        pid: u32,
    ) -> Result<ProcessSerialNumber, MacosNativeOperationError> {
        let Some(get_process_for_pid_symbol) = api.resolve_symbol("GetProcessForPID") else {
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

    pub(crate) fn front_process_window(
        api: &RealNativeApi,
        psn: &ProcessSerialNumber,
        window_id: u64,
    ) -> Result<(), MacosNativeOperationError> {
        let Some(front_process_symbol) = api.resolve_symbol("_SLPSSetFrontProcessWithOptions")
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

    pub(crate) fn make_key_window(
        api: &RealNativeApi,
        psn: &ProcessSerialNumber,
        window_id: u64,
    ) -> Result<(), MacosNativeOperationError> {
        let Some(post_event_symbol) = api.resolve_symbol("SLPSPostEventRecordTo") else {
            return Err(MacosNativeOperationError::CallFailed(
                "SLPSPostEventRecordTo",
            ));
        };
        let post_event_record_to: SlpsPostEventRecordToFn =
            unsafe { std::mem::transmute(post_event_symbol) };
        let window_id = u32::try_from(window_id)
            .map_err(|_| MacosNativeOperationError::MissingWindow(window_id))?;
        let mut event_bytes = [0u8; 0xf8];
        event_bytes[0x04] = 0xf8;
        event_bytes[0x3a] = 0x10;
        event_bytes[0x3c..0x40].copy_from_slice(&window_id.to_ne_bytes());
        event_bytes[0x20..0x30].fill(0xff);

        event_bytes[0x08] = 0x01;
        let press_status =
            unsafe { post_event_record_to(psn, event_bytes.as_ptr().cast::<c_void>()) };
        if press_status != 0 {
            return Err(MacosNativeOperationError::CallFailed(
                "SLPSPostEventRecordTo",
            ));
        }

        event_bytes[0x08] = 0x02;
        let release_status =
            unsafe { post_event_record_to(psn, event_bytes.as_ptr().cast::<c_void>()) };
        if release_status != 0 {
            return Err(MacosNativeOperationError::CallFailed(
                "SLPSPostEventRecordTo",
            ));
        }

        Ok(())
    }

    pub(crate) fn post_keyboard_event(
        _api: &RealNativeApi,
        key_code: CGKeyCode,
        key_down: bool,
        flags: CGEventFlags,
    ) -> Result<(), MacosNativeOperationError> {
        let event = unsafe {
            CfOwned::from_create_rule(CGEventCreateKeyboardEvent(
                ptr::null(),
                key_code,
                if key_down { 1 } else { 0 },
            ))
        }
        .ok_or(MacosNativeOperationError::CallFailed(
            "CGEventCreateKeyboardEvent",
        ))?;

        unsafe {
            CGEventSetFlags(event.as_type_ref(), flags);
            CGEventPost(K_CG_HID_EVENT_TAP, event.as_type_ref());
        }

        Ok(())
    }

    pub(crate) fn copy_window_descriptions_raw(
        api: &RealNativeApi,
        window_ids: CFArrayRef,
    ) -> Result<CfOwned, MacosNativeProbeError> {
        // Keep this raw for now: current callers build CFNumber-object arrays for this flow,
        // while Servo models create_description_from_array as CFArray<CGWindowID> copyables.
        let descriptions = unsafe {
            CfOwned::from_create_rule(CGWindowListCreateDescriptionFromArray(window_ids))
        }
        .ok_or(MacosNativeProbeError::MissingTopology(
            "CGWindowListCreateDescriptionFromArray",
        ))?;

        if cf_array_count(descriptions.as_type_ref() as CFArrayRef) > 0 {
            return Ok(descriptions);
        }

        let target_window_ids = skylight::parse_window_ids(window_ids)?;
        let fallback = copy_matching_onscreen_window_descriptions_raw(&target_window_ids)?;
        api.debug(&format!(
            "macos_native: falling back to onscreen descriptions requested_ids={} fallback_descriptions={}",
            target_window_ids.len(),
            cf_array_count(fallback.as_type_ref() as CFArrayRef)
        ));
        Ok(fallback)
    }

    pub(crate) fn active_space_windows_without_app_ids(
        api: &RealNativeApi,
        space_id: u64,
    ) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
        let payload = skylight::copy_windows_for_space_raw(api, space_id)?;
        let raw_window_ids = skylight::parse_window_ids(payload.as_type_ref() as CFArrayRef)?;
        let visible_order = query_visible_window_order(&raw_window_ids)?;
        let descriptions = copy_window_descriptions_raw(api, payload.as_type_ref() as CFArrayRef)?;
        parse_window_descriptions(descriptions.as_type_ref() as CFArrayRef, &visible_order)
    }

    pub(crate) fn focus_window(
        api: &RealNativeApi,
        window_id: u64,
    ) -> Result<(), MacosNativeOperationError> {
        focus_window_via_process_and_raise(
            window_id,
            |target_window_id| {
                let window = window_description(api, target_window_id)?;
                window
                    .pid
                    .ok_or(MacosNativeOperationError::MissingWindowPid(
                        target_window_id,
                    ))
            },
            |pid| process_serial_number_for_pid(api, pid),
            |psn, target_window_id| front_process_window(api, psn, target_window_id),
            |psn, target_window_id| make_key_window(api, psn, target_window_id),
            |target_window_id, pid| ax::raise_window_via_ax(api, target_window_id, pid),
        )
    }

    pub(crate) fn window_description(
        api: &RealNativeApi,
        window_id: u64,
    ) -> Result<RawWindow, MacosNativeOperationError> {
        let window_number =
            cf_number_from_u64(window_id).map_err(MacosNativeOperationError::from)?;
        let window_list = CfOwned::from_servo(array_from_type_refs(&[window_number.as_type_ref()]));
        let descriptions =
            copy_window_descriptions_raw(api, window_list.as_type_ref() as CFArrayRef)?;
        let visible_order = HashMap::new();

        parse_window_descriptions(descriptions.as_type_ref() as CFArrayRef, &visible_order)?
            .into_iter()
            .find(|window| window.id == window_id)
            .ok_or(MacosNativeOperationError::MissingWindow(window_id))
    }

    pub(crate) fn query_visible_window_order(
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

    pub(crate) fn copy_onscreen_window_descriptions_raw() -> Result<CfOwned, MacosNativeProbeError>
    {
        core_graphics::window::copy_window_info(
            K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY | K_CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS,
            K_CG_NULL_WINDOW_ID,
        )
        .map(CfOwned::from_servo)
        .ok_or(MacosNativeProbeError::MissingTopology(
            "CGWindowListCopyWindowInfo",
        ))
    }

    pub(crate) fn onscreen_window_ids_from_descriptions(
        payload: CFArrayRef,
    ) -> Result<HashSet<u64>, MacosNativeProbeError> {
        let window_number_key = cg_window_number_key();
        Ok(cf_array_iter(payload)
            .filter_map(|description| {
                let description = cf_as_dictionary(description)?;
                cf_dictionary_u64(description, window_number_key)
            })
            .collect())
    }

    pub(crate) fn filter_window_descriptions_raw(
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

        Ok(CfOwned::from_servo(array_from_type_refs(&matching)))
    }

    fn copy_matching_onscreen_window_descriptions_raw(
        target_window_ids: &[u64],
    ) -> Result<CfOwned, MacosNativeProbeError> {
        let onscreen_descriptions = copy_onscreen_window_descriptions_raw()?;
        filter_window_descriptions_raw(
            onscreen_descriptions.as_type_ref() as CFArrayRef,
            target_window_ids,
        )
    }

    pub(crate) fn parse_window_descriptions(
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

    pub(crate) fn assemble_real_active_space_windows(
        payload: CFArrayRef,
        visible_order: &HashMap<u64, usize>,
    ) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
        parse_window_descriptions(payload, visible_order).map(enrich_real_window_app_ids)
    }

    pub(crate) fn cg_window_number_key() -> CFStringRef {
        unsafe { core_graphics::window::kCGWindowNumber as CFStringRef }
    }

    pub(crate) fn cg_window_owner_pid_key() -> CFStringRef {
        unsafe { core_graphics::window::kCGWindowOwnerPID as CFStringRef }
    }

    pub(crate) fn cg_window_name_key() -> CFStringRef {
        unsafe { core_graphics::window::kCGWindowName as CFStringRef }
    }

    pub(crate) fn cg_window_layer_key() -> CFStringRef {
        unsafe { core_graphics::window::kCGWindowLayer as CFStringRef }
    }

    pub(crate) fn cg_window_bounds_key() -> CFStringRef {
        unsafe { core_graphics::window::kCGWindowBounds as CFStringRef }
    }

    fn cg_window_bounds(description: CFDictionaryRef) -> Option<NativeBounds> {
        let bounds = cf_dictionary_dictionary(description, cg_window_bounds_key())?;
        let x_key = cf_string("X").ok()?;
        let y_key = cf_string("Y").ok()?;
        let width_key = cf_string("Width").ok()?;
        let height_key = cf_string("Height").ok()?;

        Some(NativeBounds {
            x: cf_dictionary_i32(bounds, x_key.as_type_ref() as CFStringRef)?,
            y: cf_dictionary_i32(bounds, y_key.as_type_ref() as CFStringRef)?,
            width: cf_dictionary_i32(bounds, width_key.as_type_ref() as CFStringRef)?,
            height: cf_dictionary_i32(bounds, height_key.as_type_ref() as CFStringRef)?,
        })
    }
}

mod desktop_topology_snapshot {
    use super::{
        ActiveSpaceFocusTargetHint, MacosNativeOperationError, MacosNativeProbeError, NativeBounds,
        NativeDesktopSnapshot, NativeDirection, NativeSpaceSnapshot, NativeWindowSnapshot,
    };
    use std::collections::{HashMap, HashSet};

    pub(crate) use macos_window_manager::{
        RawSpaceRecord, RawTopologySnapshot, RawWindow, SpaceKind, WindowSnapshot,
    };

    pub(crate) const DESKTOP_SPACE_TYPE: i32 = 0;
    pub(crate) const FULLSCREEN_SPACE_TYPE: i32 = 4;

    pub(crate) fn classify_space(raw_space: &RawSpaceRecord) -> SpaceKind {
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

    pub(crate) fn stable_app_id_from_real_window(
        pid: Option<u32>,
        _owner_name: Option<&str>,
    ) -> Option<String> {
        pid.and_then(stable_app_id_from_pid)
    }

    pub(crate) fn enrich_real_window_app_ids(windows: Vec<RawWindow>) -> Vec<RawWindow> {
        enrich_real_window_app_ids_with(windows, stable_app_id_from_pid)
    }

    pub(crate) fn enrich_real_window_app_ids_with<F>(
        windows: Vec<RawWindow>,
        mut resolve_app_id: F,
    ) -> Vec<RawWindow>
    where
        F: FnMut(u32) -> Option<String>,
    {
        let mut app_ids_by_pid = HashMap::<u32, Option<String>>::new();
        windows
            .into_iter()
            .map(|mut window| {
                if window.app_id.is_none() {
                    window.app_id = window.pid.and_then(|pid| {
                        app_ids_by_pid
                            .entry(pid)
                            .or_insert_with(|| resolve_app_id(pid))
                            .clone()
                    });
                }
                window
            })
            .collect()
    }

    pub(crate) fn stable_app_id_from_pid(pid: u32) -> Option<String> {
        let _span = tracing::debug_span!("macos_native.app_id_from_pid", pid).entered();
        let lsappinfo_output = lsappinfo_bundle_identifier_output(pid)?;
        parse_lsappinfo_bundle_identifier(&lsappinfo_output)
    }

    fn lsappinfo_bundle_identifier_output(pid: u32) -> Option<String> {
        let _span = tracing::debug_span!("macos_native.app_id_from_pid.lsappinfo", pid).entered();
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

    pub(crate) fn parse_lsappinfo_bundle_identifier(output: &str) -> Option<String> {
        output.lines().find_map(|line| {
            line.strip_prefix("\"CFBundleIdentifier\"=")
                .and_then(|value| {
                    let bundle_identifier = value.trim().trim_matches('"');
                    (!bundle_identifier.is_empty()).then(|| bundle_identifier.to_string())
                })
        })
    }

    pub(crate) fn compare_active_windows(
        left: &RawWindow,
        right: &RawWindow,
    ) -> std::cmp::Ordering {
        match (left.visible_index, right.visible_index) {
            (Some(left_index), Some(right_index)) => left_index.cmp(&right_index),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
        .then_with(|| right.level.cmp(&left.level))
        .then_with(|| left.id.cmp(&right.id))
    }

    pub(crate) fn order_active_space_windows(windows: &[RawWindow]) -> Vec<RawWindow> {
        let mut ordered = windows.to_vec();
        ordered.sort_by(compare_active_windows);
        ordered
    }

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

    pub(crate) fn active_window_snapshot(
        space_id: u64,
        windows: &[RawWindow],
        window_id: u64,
    ) -> Option<WindowSnapshot> {
        order_active_space_windows(windows)
            .into_iter()
            .enumerate()
            .find_map(|(index, window)| {
                (window.id == window_id).then_some(WindowSnapshot {
                    id: window.id,
                    pid: window.pid,
                    app_id: window.app_id,
                    title: window.title,
                    space_id,
                    order_index: Some(index),
                })
            })
    }

    pub(crate) fn snapshots_for_inactive_space(
        space_id: u64,
        window_ids: &[u64],
    ) -> Vec<WindowSnapshot> {
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

    #[allow(dead_code)]
    pub(crate) fn native_desktop_snapshot_from_topology(
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

    pub(crate) fn window_snapshots_from_topology(
        topology: &RawTopologySnapshot,
    ) -> Vec<WindowSnapshot> {
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

    pub(crate) fn focused_window_from_active_space_windows(
        active_space_windows: &HashMap<u64, Vec<RawWindow>>,
        focused_window_id: Option<u64>,
    ) -> Result<WindowSnapshot, MacosNativeProbeError> {
        if let Some(target_window_id) = focused_window_id {
            if let Some(snapshot) = active_space_windows.iter().find_map(|(space_id, windows)| {
                active_window_snapshot(*space_id, windows, target_window_id)
            }) {
                return Ok(snapshot);
            }
        }

        active_space_windows
            .iter()
            .flat_map(|(space_id, windows)| {
                windows
                    .iter()
                    .cloned()
                    .map(move |window| (*space_id, window))
            })
            .min_by(|(_, left), (_, right)| compare_active_windows(left, right))
            .and_then(|(space_id, window)| {
                active_window_snapshot(space_id, active_space_windows.get(&space_id)?, window.id)
            })
            .ok_or(MacosNativeProbeError::MissingFocusedWindow)
    }

    pub(crate) fn space_id_for_window(
        topology: &RawTopologySnapshot,
        window_id: u64,
    ) -> Option<u64> {
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
                    .find_map(|(space_id, windows)| {
                        windows.contains(&window_id).then_some(*space_id)
                    })
            })
    }

    pub(crate) fn display_index_for_space(
        topology: &RawTopologySnapshot,
        space_id: u64,
    ) -> Option<usize> {
        topology
            .spaces
            .iter()
            .find(|space| space.managed_space_id == space_id)
            .map(|space| space.display_index)
    }

    pub(crate) fn active_space_on_display(
        topology: &RawTopologySnapshot,
        display_index: usize,
    ) -> Option<u64> {
        topology
            .active_space_ids
            .iter()
            .copied()
            .find(|space_id| display_index_for_space(topology, *space_id) == Some(display_index))
    }

    pub(crate) fn window_ids_for_space(
        topology: &RawTopologySnapshot,
        space_id: u64,
    ) -> HashSet<u64> {
        if topology.active_space_ids.contains(&space_id) {
            return topology
                .active_space_windows
                .get(&space_id)
                .into_iter()
                .flat_map(|windows| windows.iter().map(|window| window.id))
                .collect();
        }

        topology
            .inactive_space_window_ids
            .get(&space_id)
            .into_iter()
            .flat_map(|window_ids| window_ids.iter().copied())
            .collect()
    }

    pub(crate) fn best_window_id_from_windows(
        direction: NativeDirection,
        windows: &[RawWindow],
    ) -> Option<u64> {
        let focusable_windows = windows
            .iter()
            .filter(|window| is_directional_focus_window(window))
            .cloned()
            .collect::<Vec<_>>();
        edge_window_id_in_direction(&focusable_windows, direction).or_else(|| {
            focusable_windows
                .iter()
                .min_by(|left, right| compare_active_windows(left, right))
                .map(|window| window.id)
        })
    }

    pub(crate) fn is_directional_focus_window(window: &RawWindow) -> bool {
        window.level == 0
    }

    pub(crate) fn edge_window_id_in_direction(
        windows: &[RawWindow],
        direction: NativeDirection,
    ) -> Option<u64> {
        windows
            .iter()
            .filter(|window| window.frame.is_some())
            .max_by(|left, right| compare_windows_for_edge(left, right, direction))
            .map(|window| window.id)
    }

    pub(crate) fn compare_windows_for_edge(
        left: &RawWindow,
        right: &RawWindow,
        direction: NativeDirection,
    ) -> std::cmp::Ordering {
        let left_frame = left.frame.expect("frame should be present");
        let right_frame = right.frame.expect("frame should be present");

        match direction {
            NativeDirection::East => {
                (left_frame.x + left_frame.width).cmp(&(right_frame.x + right_frame.width))
            }
            NativeDirection::West => right_frame.x.cmp(&left_frame.x),
            NativeDirection::North => right_frame.y.cmp(&left_frame.y),
            NativeDirection::South => {
                (left_frame.y + left_frame.height).cmp(&(right_frame.y + right_frame.height))
            }
        }
        .then_with(|| compare_active_windows(right, left))
    }

    pub(crate) fn space_transition_window_ids(
        topology: &RawTopologySnapshot,
        target_space_id: u64,
    ) -> (Option<u64>, HashSet<u64>) {
        let source_space_id = display_index_for_space(topology, target_space_id)
            .and_then(|display_index| active_space_on_display(topology, display_index))
            .filter(|source_space_id| *source_space_id != target_space_id);
        let source_focus_window_id = topology
            .focused_window_id
            .filter(|window_id| space_id_for_window(topology, *window_id) == source_space_id);
        let target_window_ids = window_ids_for_space(topology, target_space_id);

        (source_focus_window_id, target_window_ids)
    }

    pub(crate) fn ensure_supported_target_space(
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

    pub(crate) fn active_window_pid_from_topology(
        topology: &RawTopologySnapshot,
        window_id: u64,
    ) -> Option<u32> {
        topology
            .active_space_windows
            .values()
            .flat_map(|windows| windows.iter())
            .find(|window| window.id == window_id)
            .and_then(|window| window.pid)
    }

    pub(crate) fn active_space_focus_target_hint_from_topology(
        topology: &RawTopologySnapshot,
        window_id: u64,
    ) -> Option<ActiveSpaceFocusTargetHint> {
        let space_id = space_id_for_window(topology, window_id)?;
        let bounds = topology
            .active_space_windows
            .get(&space_id)?
            .iter()
            .find(|window| window.id == window_id)?
            .frame?;
        Some(ActiveSpaceFocusTargetHint { space_id, bounds })
    }

    #[cfg(test)]
    pub(super) mod tests {
        use super::*;

        #[derive(Debug, Clone, PartialEq, Eq)]
        pub(crate) struct SpaceSnapshot {
            pub(crate) id: u64,
            pub(crate) kind: SpaceKind,
            pub(crate) is_active: bool,
            pub(crate) ordered_window_ids: Option<Vec<u64>>,
        }

        pub(crate) fn space_snapshots_from_topology(
            topology: &RawTopologySnapshot,
        ) -> Vec<SpaceSnapshot> {
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
    }

    pub(crate) fn focused_window_from_topology(
        topology: &RawTopologySnapshot,
    ) -> Result<WindowSnapshot, MacosNativeProbeError> {
        focused_window_from_active_space_windows(
            &topology.active_space_windows,
            topology.focused_window_id,
        )
    }
}

pub(crate) fn validate_environment_with_api<A: MacosNativeApi + ?Sized>(
    api: &A,
) -> Result<(), MacosNativeConnectError> {
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

    Ok(())
}

pub(super) fn wait_for_space_presentation<A: MacosNativeApi + ?Sized>(
    api: &A,
    space_id: u64,
    source_focus_window_id: Option<u64>,
    target_window_ids: &HashSet<u64>,
) -> Result<(), MacosNativeOperationError> {
    let deadline = Instant::now() + SPACE_SWITCH_SETTLE_TIMEOUT;
    let mut polls = 0usize;
    let mut stable_target_polls = 0usize;

    loop {
        polls += 1;
        let active_space_ids = api.active_space_ids()?;
        let onscreen_window_ids = api.onscreen_window_ids()?;
        let target_active = active_space_ids.contains(&space_id);
        let source_focus_hidden = source_focus_window_id
            .is_none_or(|window_id| !onscreen_window_ids.contains(&window_id));
        let target_visible =
            target_window_ids.is_empty() || !target_window_ids.is_disjoint(&onscreen_window_ids);
        if target_active && target_visible {
            stable_target_polls += 1;
        } else {
            stable_target_polls = 0;
        }

        if target_active
            && target_visible
            && (source_focus_hidden || stable_target_polls >= SPACE_SWITCH_STABLE_TARGET_POLLS)
        {
            api.debug(&format!(
                "macos_native: space {space_id} presentation settled after {polls} poll(s)"
            ));
            return Ok(());
        }

        if Instant::now() >= deadline {
            api.debug(&format!(
                "macos_native: space {space_id} did not settle after {polls} poll(s) target_active={target_active} source_focus_hidden={source_focus_hidden} target_visible={target_visible}"
            ));
            return Err(MacosNativeOperationError::CallFailed(
                "wait_for_active_space",
            ));
        }

        std::thread::sleep(SPACE_SWITCH_POLL_INTERVAL);
    }
}

fn switch_space_in_snapshot<A: MacosNativeApi + ?Sized>(
    api: &A,
    snapshot: &NativeDesktopSnapshot,
    space_id: u64,
    adjacent_direction: Option<NativeDirection>,
) -> Result<(), MacosNativeOperationError> {
    let Some(target_space) = snapshot.spaces.iter().find(|space| space.id == space_id) else {
        return Err(MacosNativeOperationError::MissingSpace(space_id));
    };
    if target_space.kind == SpaceKind::StageManagerOpaque {
        return Err(MacosNativeOperationError::UnsupportedStageManagerSpace(
            space_id,
        ));
    }
    if snapshot.active_space_ids.contains(&space_id) {
        return Ok(());
    }

    let (source_focus_window_id, target_window_ids) =
        outer_space_transition_window_ids(snapshot, space_id);
    api.debug(&format!(
        "macos_native: switching to space {space_id} source_focus={:?} target_windows={}",
        source_focus_window_id,
        target_window_ids.len()
    ));
    if let Some(direction) = adjacent_direction {
        if target_window_ids.is_empty() {
            api.debug(&format!(
                "macos_native: using exact space switch for empty adjacent space {space_id}"
            ));
            api.switch_space(space_id)?;
            return wait_for_space_presentation(
                api,
                space_id,
                source_focus_window_id,
                &target_window_ids,
            );
        }

        api.switch_adjacent_space(direction, space_id)?;
        match wait_for_space_presentation(api, space_id, source_focus_window_id, &target_window_ids)
        {
            Ok(()) => Ok(()),
            Err(err) => {
                let target_still_inactive = match api.active_space_ids() {
                    Ok(active_space_ids) => !active_space_ids.contains(&space_id),
                    Err(probe_err) => {
                        api.debug(&format!(
                            "macos_native: failed to re-check active spaces after adjacent hotkey switch failure for space {space_id} ({probe_err}); retrying exact space switch"
                        ));
                        true
                    }
                };

                if !target_still_inactive {
                    return Err(err);
                }

                let retry_target_window_ids = match api.onscreen_window_ids() {
                    Ok(onscreen_window_ids)
                        if !target_window_ids.is_empty()
                            && !target_window_ids.is_disjoint(&onscreen_window_ids) =>
                    {
                        api.debug(&format!(
                            "macos_native: adjacent hotkey left target-space window ids visible while target space {space_id} is still inactive; treating target ids as unreliable for exact-switch retry"
                        ));
                        HashSet::new()
                    }
                    Ok(_) => target_window_ids.clone(),
                    Err(probe_err) => {
                        api.debug(&format!(
                            "macos_native: failed to inspect onscreen windows after adjacent hotkey switch failure for space {space_id} ({probe_err}); preserving target ids for exact-switch retry"
                        ));
                        target_window_ids.clone()
                    }
                };

                api.debug(&format!(
                    "macos_native: adjacent hotkey did not activate target space {space_id}; retrying exact space switch"
                ));
                api.switch_space(space_id)?;
                wait_for_space_presentation(
                    api,
                    space_id,
                    source_focus_window_id,
                    &retry_target_window_ids,
                )
            }
        }
    } else {
        api.switch_space(space_id)?;
        wait_for_space_presentation(api, space_id, source_focus_window_id, &target_window_ids)
    }
}

fn outer_space_transition_window_ids(
    snapshot: &NativeDesktopSnapshot,
    target_space_id: u64,
) -> (Option<u64>, HashSet<u64>) {
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

fn native_window(
    snapshot: &NativeDesktopSnapshot,
    window_id: u64,
) -> Option<&NativeWindowSnapshot> {
    snapshot
        .windows
        .iter()
        .find(|window| window.id == window_id)
}

fn native_space(snapshot: &NativeDesktopSnapshot, space_id: u64) -> Option<&NativeSpaceSnapshot> {
    snapshot.spaces.iter().find(|space| space.id == space_id)
}

fn native_candidate_extends_in_direction(
    source: NativeBounds,
    candidate: NativeBounds,
    direction: NativeDirection,
) -> bool {
    match direction {
        NativeDirection::West => candidate.x < source.x,
        NativeDirection::East => candidate.x + candidate.width > source.x + source.width,
        NativeDirection::North => candidate.y < source.y,
        NativeDirection::South => candidate.y + candidate.height > source.y + source.height,
    }
}

fn is_directional_focus_window(window: &NativeWindowSnapshot) -> bool {
    window.level == 0
}

fn native_overlap_len(start_a: i32, len_a: i32, start_b: i32, len_b: i32) -> i64 {
    let end_a = start_a + len_a;
    let end_b = start_b + len_b;
    i64::from((end_a.min(end_b) - start_a.max(start_b)).max(0))
}

fn native_overlap_area(left: NativeBounds, right: NativeBounds) -> i64 {
    native_overlap_len(left.x, left.width, right.x, right.width)
        * native_overlap_len(left.y, left.height, right.y, right.height)
}

fn native_center_distance_sq(left: NativeBounds, right: NativeBounds) -> i128 {
    let left_center_x = left.x as i128 + left.width as i128 / 2;
    let left_center_y = left.y as i128 + left.height as i128 / 2;
    let right_center_x = right.x as i128 + right.width as i128 / 2;
    let right_center_y = right.y as i128 + right.height as i128 / 2;
    let delta_x = left_center_x - right_center_x;
    let delta_y = left_center_y - right_center_y;
    delta_x * delta_x + delta_y * delta_y
}

fn compare_native_windows_for_target_match(
    target_bounds: NativeBounds,
    left: &NativeWindowSnapshot,
    right: &NativeWindowSnapshot,
) -> std::cmp::Ordering {
    let left_bounds = left.bounds.expect("bounds should be present");
    let right_bounds = right.bounds.expect("bounds should be present");
    let left_overlap = native_overlap_area(target_bounds, left_bounds);
    let right_overlap = native_overlap_area(target_bounds, right_bounds);
    let left_distance = native_center_distance_sq(target_bounds, left_bounds);
    let right_distance = native_center_distance_sq(target_bounds, right_bounds);

    left_overlap
        .cmp(&right_overlap)
        .then_with(|| right_distance.cmp(&left_distance))
        .then_with(|| super::compare_native_active_windows(right, left))
}

fn compare_native_windows_for_edge(
    left: &NativeWindowSnapshot,
    right: &NativeWindowSnapshot,
    direction: NativeDirection,
) -> std::cmp::Ordering {
    let left_bounds = left.bounds.expect("bounds should be present");
    let right_bounds = right.bounds.expect("bounds should be present");

    match direction {
        NativeDirection::East => {
            (left_bounds.x + left_bounds.width).cmp(&(right_bounds.x + right_bounds.width))
        }
        NativeDirection::West => right_bounds.x.cmp(&left_bounds.x),
        NativeDirection::North => right_bounds.y.cmp(&left_bounds.y),
        NativeDirection::South => {
            (left_bounds.y + left_bounds.height).cmp(&(right_bounds.y + right_bounds.height))
        }
    }
    .then_with(|| super::compare_native_active_windows(right, left))
}

fn native_ax_backed_same_pid_target(
    snapshot: &NativeDesktopSnapshot,
    direction: NativeDirection,
    pid: u32,
    ax_window_ids: &HashSet<u64>,
) -> Option<u64> {
    let focused = super::resolved_focused_native_window(snapshot).ok()?;
    let focused_space = native_space(snapshot, focused.space_id)?;
    if focused.pid != Some(pid) || focused_space.kind != SpaceKind::SplitView {
        return None;
    }

    let source_bounds = focused.bounds?;
    snapshot
        .windows
        .iter()
        .filter(|window| window.id != focused.id)
        .filter(|window| window.space_id == focused.space_id)
        .filter(|window| is_directional_focus_window(window))
        .filter(|window| window.pid == Some(pid))
        .filter(|window| ax_window_ids.contains(&window.id))
        .filter(|window| {
            window.bounds.is_some_and(|bounds| {
                native_candidate_extends_in_direction(source_bounds, bounds, direction)
            })
        })
        .max_by(|left, right| compare_native_windows_for_edge(left, right, direction))
        .map(|window| window.id)
}

fn active_space_ax_backed_same_pid_target<A: MacosNativeApi + ?Sized>(
    api: &A,
    snapshot: &NativeDesktopSnapshot,
    target_window_id: u64,
    pid: u32,
    target_hint: Option<ActiveSpaceFocusTargetHint>,
) -> Result<Option<u64>, MacosNativeOperationError> {
    let target = native_window(snapshot, target_window_id);
    let Some(target_bounds) = target
        .and_then(|window| window.bounds)
        .or(target_hint.map(|hint| hint.bounds))
    else {
        api.debug(&format!(
            "macos_native: active-space stale-target remap skipped; target window {target_window_id} has no bounds"
        ));
        return Ok(None);
    };
    let Some(target_space_id) = target
        .map(|window| window.space_id)
        .or(target_hint.map(|hint| hint.space_id))
    else {
        api.debug(&format!(
            "macos_native: active-space stale-target remap skipped; target window {target_window_id} missing from snapshot"
        ));
        return Ok(None);
    };
    let Some(target_space) = native_space(snapshot, target_space_id) else {
        api.debug(&format!(
            "macos_native: active-space stale-target remap skipped; target space {} missing from snapshot",
            target_space_id
        ));
        return Ok(None);
    };
    if target_space.kind != SpaceKind::SplitView
        || target.is_some_and(|window| window.pid != Some(pid))
    {
        return Ok(None);
    }

    let ax_window_ids = api
        .ax_window_ids_for_pid(pid)?
        .into_iter()
        .collect::<HashSet<_>>();
    let candidates = snapshot
        .windows
        .iter()
        .filter(|window| window.id != target_window_id)
        .filter(|window| window.space_id == target_space_id)
        .filter(|window| window.pid == Some(pid))
        .filter(|window| is_directional_focus_window(window))
        .filter(|window| window.bounds.is_some())
        .filter(|window| ax_window_ids.contains(&window.id))
        .collect::<Vec<_>>();

    api.debug(&format!(
        "macos_native: active-space stale-target remap target={} pid={} candidates={:?}",
        target_window_id,
        pid,
        candidates
            .iter()
            .map(|candidate| candidate.id)
            .collect::<Vec<_>>()
    ));

    Ok(candidates
        .into_iter()
        .max_by(|left, right| compare_native_windows_for_target_match(target_bounds, left, right))
        .map(|window| window.id))
}

fn split_view_same_space_focus_target_from_source(
    snapshot: &NativeDesktopSnapshot,
    source_window_id: u64,
    direction: NativeDirection,
) -> Option<u64> {
    let focused = native_window(snapshot, source_window_id)?;
    let focused_space = native_space(snapshot, focused.space_id)?;
    if focused_space.kind != SpaceKind::SplitView {
        return None;
    }

    let source_bounds = focused.bounds?;
    snapshot
        .windows
        .iter()
        .filter(|window| window.id != focused.id)
        .filter(|window| window.space_id == focused.space_id)
        .filter(|window| is_directional_focus_window(window))
        .filter(|window| {
            window.bounds.is_some_and(|bounds| {
                native_candidate_extends_in_direction(source_bounds, bounds, direction)
            })
        })
        .max_by(|left, right| compare_native_windows_for_edge(left, right, direction))
        .map(|window| window.id)
}

fn split_view_same_space_focus_target(
    snapshot: &NativeDesktopSnapshot,
    direction: NativeDirection,
) -> Option<u64> {
    let focused = super::resolved_focused_native_window(snapshot).ok()?;
    split_view_same_space_focus_target_from_source(snapshot, focused.id, direction)
}

fn focusable_same_app_split_view_peer_from_source<A: MacosNativeApi + ?Sized>(
    api: &A,
    snapshot: &NativeDesktopSnapshot,
    source_window_id: u64,
    direction: NativeDirection,
    target_window_id: u64,
) -> Result<Option<(u64, u32)>, MacosNativeOperationError> {
    let Some(focused) = native_window(snapshot, source_window_id) else {
        api.debug(&format!(
            "macos_native: split-view peer remap skipped; source window {source_window_id} missing from snapshot"
        ));
        return Ok(None);
    };
    let Some(source_bounds) = focused.bounds else {
        api.debug(&format!(
            "macos_native: split-view peer remap skipped; source window {source_window_id} has no bounds"
        ));
        return Ok(None);
    };
    let Some(target) = native_window(snapshot, target_window_id) else {
        api.debug(&format!(
            "macos_native: split-view peer remap skipped; target window {target_window_id} missing from snapshot"
        ));
        return Ok(None);
    };
    let Some(target_app_id) = target.app_id.as_deref() else {
        api.debug(&format!(
            "macos_native: split-view peer remap skipped; target window {target_window_id} has no app_id"
        ));
        return Ok(None);
    };
    let mut candidates = snapshot
        .windows
        .iter()
        .filter(|window| window.id != focused.id && window.id != target_window_id)
        .filter(|window| window.space_id == focused.space_id)
        .filter(|window| is_directional_focus_window(window))
        .filter(|window| window.app_id.as_deref() == Some(target_app_id))
        .filter(|window| window.pid.is_some())
        .filter(|window| {
            window.bounds.is_some_and(|bounds| {
                native_candidate_extends_in_direction(source_bounds, bounds, direction)
            })
        })
        .collect::<Vec<_>>();
    candidates
        .sort_by(|left, right| compare_native_windows_for_edge(left, right, direction).reverse());

    api.debug(&format!(
        "macos_native: split-view peer remap source={} target={} target_pid={:?} target_app_id={} candidates={:?}",
        source_window_id,
        target_window_id,
        target.pid,
        target_app_id,
        candidates
            .iter()
            .map(|candidate| (candidate.id, candidate.pid))
            .collect::<Vec<_>>()
    ));

    let fallback_candidate = candidates
        .first()
        .and_then(|candidate| candidate.pid.map(|pid| (candidate.id, pid)));

    let mut ax_window_ids_by_pid = HashMap::<u32, HashSet<u64>>::new();
    for candidate in candidates {
        let Some(pid) = candidate.pid else {
            continue;
        };
        let ax_window_ids = match ax_window_ids_by_pid.entry(pid) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => entry.insert(
                api.ax_window_ids_for_pid(pid)?
                    .into_iter()
                    .collect::<HashSet<_>>(),
            ),
        };
        api.debug(&format!(
            "macos_native: split-view peer remap candidate={} pid={} ax_window_ids={:?}",
            candidate.id, pid, ax_window_ids
        ));
        if ax_window_ids.contains(&candidate.id) {
            api.debug(&format!(
                "macos_native: split-view peer remap chose AX-backed candidate={} pid={}",
                candidate.id, pid
            ));
            return Ok(Some((candidate.id, pid)));
        }
    }

    if let Some((candidate_id, candidate_pid)) = fallback_candidate {
        api.debug(&format!(
            "macos_native: split-view peer remap falling back to directional candidate={} pid={} despite empty AX preflight",
            candidate_id, candidate_pid
        ));
    } else {
        api.debug("macos_native: split-view peer remap found no same-app directional candidates");
    }

    Ok(fallback_candidate)
}

fn focusable_same_app_split_view_peer<A: MacosNativeApi + ?Sized>(
    api: &A,
    snapshot: &NativeDesktopSnapshot,
    direction: NativeDirection,
    target_window_id: u64,
) -> Result<Option<(u64, u32)>, MacosNativeOperationError> {
    let Some(focused) = super::resolved_focused_native_window(snapshot).ok() else {
        return Ok(None);
    };
    focusable_same_app_split_view_peer_from_source(
        api,
        snapshot,
        focused.id,
        direction,
        target_window_id,
    )
}

fn refreshed_split_view_focus_target<A: MacosNativeApi + ?Sized>(
    api: &A,
    snapshot: &NativeDesktopSnapshot,
    direction: NativeDirection,
    target_window_id: u64,
    pid: u32,
) -> Result<Option<(u64, Option<u32>)>, MacosNativeOperationError> {
    let Some(original_focused) = super::resolved_focused_native_window(snapshot).ok() else {
        api.debug(
            "macos_native: refreshed split-view retarget skipped; no focused source window in planning snapshot",
        );
        return Ok(None);
    };
    let refreshed_snapshot = api.desktop_snapshot()?;
    let Some(refreshed_target_id) = split_view_same_space_focus_target_from_source(
        &refreshed_snapshot,
        original_focused.id,
        direction,
    ) else {
        api.debug(&format!(
            "macos_native: refreshed split-view retarget found no directional target from source {}",
            original_focused.id
        ));
        return Ok(None);
    };
    let refreshed_pid =
        native_window(&refreshed_snapshot, refreshed_target_id).and_then(|window| window.pid);
    api.debug(&format!(
        "macos_native: refreshed split-view retarget source={} stale_target={} stale_pid={} refreshed_target={} refreshed_pid={:?}",
        original_focused.id, target_window_id, pid, refreshed_target_id, refreshed_pid
    ));
    if refreshed_target_id == target_window_id && refreshed_pid == Some(pid) {
        if let Some((peer_target_id, peer_pid)) = focusable_same_app_split_view_peer_from_source(
            api,
            &refreshed_snapshot,
            original_focused.id,
            direction,
            refreshed_target_id,
        )? {
            api.debug(&format!(
                "macos_native: refreshed split-view retarget remapped stale target {} to peer {} pid={}",
                refreshed_target_id, peer_target_id, peer_pid
            ));
            return Ok(Some((peer_target_id, Some(peer_pid))));
        }
        api.debug(&format!(
            "macos_native: refreshed split-view retarget still stale after peer probing target={} pid={}",
            refreshed_target_id, pid
        ));
        return Ok(None);
    }
    Ok(Some((refreshed_target_id, refreshed_pid)))
}

fn focus_same_space_target_in_snapshot<A: MacosNativeApi + ?Sized>(
    api: &A,
    snapshot: &NativeDesktopSnapshot,
    direction: NativeDirection,
    target_window_id: u64,
) -> Result<(), MacosNativeOperationError> {
    let focus_target_id =
        split_view_same_space_focus_target(snapshot, direction).unwrap_or(target_window_id);
    let Some(pid) = native_window(snapshot, focus_target_id).and_then(|window| window.pid) else {
        return api.focus_window(focus_target_id);
    };

    focus_same_space_target_with_known_pid(api, snapshot, direction, focus_target_id, pid)
}

fn focus_same_space_target_with_known_pid<A: MacosNativeApi + ?Sized>(
    api: &A,
    snapshot: &NativeDesktopSnapshot,
    direction: NativeDirection,
    target_window_id: u64,
    pid: u32,
) -> Result<(), MacosNativeOperationError> {
    let focused = super::resolved_focused_native_window(snapshot)
        .ok()
        .filter(|focused| focused.pid == Some(pid));
    let same_pid_split_view = focused
        .and_then(|focused| native_space(snapshot, focused.space_id))
        .is_some_and(|space| space.kind == SpaceKind::SplitView);
    let mut ax_window_ids = None;
    let mut focus_target_id = target_window_id;

    api.debug(&format!(
        "macos_native: split-view focus target preflight target={} pid={} same_pid_split_view={} focused_same_pid={:?}",
        target_window_id,
        pid,
        same_pid_split_view,
        focused.as_ref().map(|window| window.id)
    ));

    if same_pid_split_view {
        let ids = api
            .ax_window_ids_for_pid(pid)?
            .into_iter()
            .collect::<HashSet<_>>();
        if !ids.contains(&target_window_id) {
            if let Some(remapped_target_id) =
                native_ax_backed_same_pid_target(snapshot, direction, pid, &ids)
                    .filter(|candidate| *candidate != target_window_id)
            {
                api.debug(&format!(
                    "macos_native: split-view focus remapped same-pid stale target {} to {}",
                    target_window_id, remapped_target_id
                ));
                focus_target_id = remapped_target_id;
            }
        }
        ax_window_ids = Some(ids);
    }

    match api.focus_window_with_known_pid(focus_target_id, pid) {
        Err(MacosNativeOperationError::MissingWindow(missing_window_id))
            if missing_window_id == focus_target_id =>
        {
            if same_pid_split_view {
                let ax_window_ids = match ax_window_ids {
                    Some(ids) => ids,
                    None => api
                        .ax_window_ids_for_pid(pid)?
                        .into_iter()
                        .collect::<HashSet<_>>(),
                };
                if let Some(remapped_target_id) =
                    native_ax_backed_same_pid_target(snapshot, direction, pid, &ax_window_ids)
                        .filter(|candidate| *candidate != focus_target_id)
                {
                    api.debug(&format!(
                        "macos_native: split-view focus retry remapped same-pid stale target {} to {}",
                        focus_target_id, remapped_target_id
                    ));
                    return api.focus_window_with_known_pid(remapped_target_id, pid);
                }
            }

            if let Some((remapped_target_id, remapped_pid)) =
                focusable_same_app_split_view_peer(api, snapshot, direction, focus_target_id)?
            {
                api.debug(&format!(
                    "macos_native: split-view focus remapped stale target {} to same-app peer {} pid={}",
                    focus_target_id, remapped_target_id, remapped_pid
                ));
                return api.focus_window_with_known_pid(remapped_target_id, remapped_pid);
            }

            if let Some((refreshed_target_id, refreshed_pid)) =
                refreshed_split_view_focus_target(api, snapshot, direction, focus_target_id, pid)?
            {
                if let Some(refreshed_pid) = refreshed_pid {
                    api.debug(&format!(
                        "macos_native: split-view focus retrying with refreshed target {} pid={}",
                        refreshed_target_id, refreshed_pid
                    ));
                    return api.focus_window_with_known_pid(refreshed_target_id, refreshed_pid);
                }
                api.debug(&format!(
                    "macos_native: split-view focus retrying with refreshed target {} via generic focus",
                    refreshed_target_id
                ));
                return api.focus_window(refreshed_target_id);
            }

            if !same_pid_split_view {
                api.debug(&format!(
                    "macos_native: split-view focus falling back to generic focus for stale target {}",
                    focus_target_id
                ));
                return api.focus_window(focus_target_id);
            }

            Err(MacosNativeOperationError::MissingWindow(focus_target_id))
        }
        other => other,
    }
}

pub(crate) struct RealNativeApi {
    skylight: Option<DylibHandle>,
    hiservices: Option<DylibHandle>,
    options: NativeBackendOptions,
}

impl RealNativeApi {
    pub(crate) fn new(options: NativeBackendOptions) -> Self {
        Self {
            skylight: DylibHandle::open(SKYLIGHT_FRAMEWORK_PATH),
            hiservices: DylibHandle::open(HISERVICES_FRAMEWORK_PATH),
            options,
        }
    }

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

    fn debug(&self, message: impl AsRef<str>) {
        if let Some(diagnostics) = self.options.diagnostics.as_ref() {
            diagnostics.debug(message.as_ref());
        }
    }
}

impl MacosNativeApi for RealNativeApi {
    fn has_symbol(&self, symbol: &'static str) -> bool {
        self.resolve_symbol(symbol).is_some()
    }

    fn debug(&self, message: &str) {
        RealNativeApi::debug(self, message);
    }

    fn ax_is_trusted(&self) -> bool {
        ax::is_process_trusted(self)
    }

    fn minimal_topology_ready(&self) -> bool {
        let Some(symbol) = self.resolve_symbol("SLSMainConnectionID") else {
            return false;
        };

        let main_connection_id: SlsMainConnectionIdFn = unsafe { std::mem::transmute(symbol) };
        unsafe { main_connection_id() != 0 }
    }

    fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
        Ok(native_desktop_snapshot_from_topology(
            &self.topology_snapshot()?,
        ))
    }

    fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
        let payload = skylight::copy_managed_display_spaces_raw(self)?;
        parse_managed_spaces(payload.as_type_ref() as CFArrayRef)
    }

    fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
        let payload = skylight::copy_managed_display_spaces_raw(self)?;
        let display_identifiers = parse_display_identifiers(payload.as_type_ref() as CFArrayRef)?;
        let active_space_ids = display_identifiers
            .into_iter()
            .map(|display_identifier| {
                skylight::current_space_for_display(self, &display_identifier)
            })
            .collect::<Result<HashSet<_>, _>>()?;

        (!active_space_ids.is_empty())
            .then_some(active_space_ids)
            .ok_or(MacosNativeProbeError::MissingTopology(
                "SLSManagedDisplayGetCurrentSpace",
            ))
    }

    fn active_space_windows(&self, space_id: u64) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
        let payload = skylight::copy_windows_for_space_raw(self, space_id)?;
        let visible_order =
            query_visible_window_order(&parse_window_ids(payload.as_type_ref() as CFArrayRef)?)?;
        let descriptions =
            window_server::copy_window_descriptions_raw(self, payload.as_type_ref() as CFArrayRef)?;

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

            let payload = skylight::copy_windows_for_space_raw(self, space.managed_space_id)?;
            inactive_space_window_ids.insert(
                space.managed_space_id,
                parse_window_ids(payload.as_type_ref() as CFArrayRef)?,
            );
        }

        Ok(inactive_space_window_ids)
    }

    fn onscreen_window_ids(&self) -> Result<HashSet<NativeWindowId>, MacosNativeProbeError> {
        let descriptions = copy_onscreen_window_descriptions_raw()?;
        onscreen_window_ids_from_descriptions(descriptions.as_type_ref() as CFArrayRef)
    }

    fn switch_space(&self, space_id: u64) -> Result<(), MacosNativeOperationError> {
        skylight::switch_space(self, space_id)
    }

    fn switch_adjacent_space(
        &self,
        direction: NativeDirection,
        _space_id: u64,
    ) -> Result<(), MacosNativeOperationError> {
        self.debug(&format!(
            "macos_native: switching adjacent space via mission-control hotkey direction={direction}"
        ));
        switch_adjacent_space_via_hotkey(&self.options, direction, |key_code, key_down, flags| {
            window_server::post_keyboard_event(self, key_code, key_down, flags)
        })
    }

    fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
        window_server::focus_window(self, window_id)
    }

    fn focus_window_with_known_pid(
        &self,
        window_id: u64,
        pid: u32,
    ) -> Result<(), MacosNativeOperationError> {
        match focus_window_via_process_and_raise(
            window_id,
            |_| Ok(pid),
            |resolved_pid| window_server::process_serial_number_for_pid(self, resolved_pid),
            |psn, target_window_id| {
                window_server::front_process_window(self, psn, target_window_id)
            },
            |psn, target_window_id| window_server::make_key_window(self, psn, target_window_id),
            |target_window_id, resolved_pid| {
                ax::raise_window_via_ax(self, target_window_id, resolved_pid)
            },
        ) {
            Err(MacosNativeOperationError::MissingWindow(missing_window_id))
                if missing_window_id == window_id =>
            {
                let deadline = Instant::now() + AX_RAISE_SETTLE_TIMEOUT;
                loop {
                    if self.focused_window_id().ok() == Some(Some(window_id)) {
                        self.debug(&format!(
                            "macos_native: treating missing AX raise target {window_id} as success after focus confirmation"
                        ));
                        return Ok(());
                    }
                    if Instant::now() >= deadline {
                        break;
                    }
                    std::thread::sleep(AX_RAISE_RETRY_INTERVAL);
                }
                self.debug(&format!(
                    "macos_native: AX raise still missing target {window_id} after retries; focused_window_id={:?}",
                    self.focused_window_id().ok().flatten()
                ));
                Err(MacosNativeOperationError::MissingWindow(window_id))
            }
            other => other,
        }
    }

    fn ax_window_ids_for_pid(&self, pid: u32) -> Result<Vec<u64>, MacosNativeOperationError> {
        ax::ax_window_ids_for_pid(self, pid)
    }

    fn focus_window_in_active_space_with_known_pid(
        &self,
        window_id: u64,
        pid: u32,
        target_hint: Option<ActiveSpaceFocusTargetHint>,
    ) -> Result<(), MacosNativeOperationError> {
        match focus_window_via_make_key_and_raise(
            window_id,
            |_| Ok(pid),
            |resolved_pid| window_server::process_serial_number_for_pid(self, resolved_pid),
            |psn, target_window_id| window_server::make_key_window(self, psn, target_window_id),
            |target_window_id, resolved_pid| {
                ax::raise_window_via_ax(self, target_window_id, resolved_pid)
            },
        ) {
            Err(MacosNativeOperationError::MissingWindow(missing_window_id))
                if missing_window_id == window_id =>
            {
                let deadline = Instant::now() + AX_RAISE_SETTLE_TIMEOUT;
                loop {
                    if self.focused_window_id().ok() == Some(Some(window_id)) {
                        self.debug(&format!(
                            "macos_native: treating missing active-space AX raise target {window_id} as success after focus confirmation"
                        ));
                        return Ok(());
                    }
                    if Instant::now() >= deadline {
                        break;
                    }
                    std::thread::sleep(AX_RAISE_RETRY_INTERVAL);
                }
                if let Some(remapped_target_id) = active_space_ax_backed_same_pid_target(
                    self,
                    &self.desktop_snapshot()?,
                    window_id,
                    pid,
                    target_hint,
                )? {
                    self.debug(&format!(
                        "macos_native: active-space focus remapped stale same-pid target {} to {}",
                        window_id, remapped_target_id
                    ));
                    return self.focus_window_with_known_pid(remapped_target_id, pid);
                }
                self.debug(&format!(
                    "macos_native: active-space AX raise still missing target {window_id} after retries; focused_window_id={:?}",
                    self.focused_window_id().ok().flatten()
                ));
                Err(MacosNativeOperationError::MissingWindow(window_id))
            }
            other => other,
        }
    }

    fn move_window_to_space(
        &self,
        window_id: u64,
        space_id: u64,
    ) -> Result<(), MacosNativeOperationError> {
        skylight::move_window_to_space(self, window_id, space_id)
    }

    fn swap_window_frames(
        &self,
        source_window_id: u64,
        source_frame: NativeBounds,
        target_window_id: u64,
        target_frame: NativeBounds,
    ) -> Result<(), MacosNativeOperationError> {
        ax::swap_window_frames(
            self,
            source_window_id,
            source_frame,
            target_window_id,
            target_frame,
        )
    }

    fn focused_window_id(&self) -> Result<Option<NativeWindowId>, MacosNativeProbeError> {
        ax::probe_focused_window_id(self)
    }

    fn focused_window_snapshot(&self) -> Result<WindowSnapshot, MacosNativeProbeError> {
        let focused_window_id = ax::probe_focused_window_id(self)?;
        let active_space_ids = self.active_space_ids()?;
        let mut active_space_windows = HashMap::new();

        for space_id in active_space_ids {
            let windows = window_server::active_space_windows_without_app_ids(self, space_id)?;
            if let Some(target_window_id) = focused_window_id {
                if let Some(mut snapshot) =
                    active_window_snapshot(space_id, &windows, target_window_id)
                {
                    snapshot.app_id = snapshot
                        .app_id
                        .or_else(|| stable_app_id_from_real_window(snapshot.pid, None));
                    return Ok(snapshot);
                }
            }
            active_space_windows.insert(space_id, windows);
        }

        let mut snapshot =
            focused_window_from_active_space_windows(&active_space_windows, focused_window_id)?;
        snapshot.app_id = snapshot
            .app_id
            .or_else(|| stable_app_id_from_real_window(snapshot.pid, None));
        Ok(snapshot)
    }

    fn topology_snapshot(&self) -> Result<RawTopologySnapshot, MacosNativeProbeError> {
        let mut topology = self.topology_snapshot_without_focus()?;
        topology.focused_window_id = self.focused_window_id()?;
        Ok(topology)
    }

    fn topology_snapshot_without_focus(
        &self,
    ) -> Result<RawTopologySnapshot, MacosNativeProbeError> {
        let payload = skylight::copy_managed_display_spaces_raw(self)?;
        let payload = payload.as_type_ref() as CFArrayRef;
        let spaces = parse_managed_spaces(payload)?;
        let active_space_ids = parse_active_space_ids(payload)?;
        let mut active_space_windows = HashMap::new();
        let mut inactive_space_window_ids = HashMap::new();

        for space in &spaces {
            let payload = skylight::copy_windows_for_space_raw(self, space.managed_space_id)?;
            let raw_window_ids = parse_window_ids(payload.as_type_ref() as CFArrayRef)?;

            if active_space_ids.contains(&space.managed_space_id) {
                let visible_order = query_visible_window_order(&raw_window_ids)?;
                let descriptions = window_server::copy_window_descriptions_raw(
                    self,
                    payload.as_type_ref() as CFArrayRef,
                )?;
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
            focused_window_id: None,
        })
    }
}

pub(crate) fn focus_window_via_process_and_raise<
    WindowPid,
    ProcessSerial,
    FrontProcessWindow,
    MakeKeyWindow,
    RaiseWindow,
>(
    window_id: u64,
    mut window_pid: WindowPid,
    mut process_serial_number: ProcessSerial,
    mut front_process_window: FrontProcessWindow,
    mut make_key_window: MakeKeyWindow,
    mut raise_window: RaiseWindow,
) -> Result<(), MacosNativeOperationError>
where
    WindowPid: FnMut(u64) -> Result<u32, MacosNativeOperationError>,
    ProcessSerial: FnMut(u32) -> Result<ProcessSerialNumber, MacosNativeOperationError>,
    FrontProcessWindow: FnMut(&ProcessSerialNumber, u64) -> Result<(), MacosNativeOperationError>,
    MakeKeyWindow: FnMut(&ProcessSerialNumber, u64) -> Result<(), MacosNativeOperationError>,
    RaiseWindow: FnMut(u64, u32) -> Result<(), MacosNativeOperationError>,
{
    let pid = window_pid(window_id)?;
    let psn = process_serial_number(pid)?;
    front_process_window(&psn, window_id)?;
    make_key_window(&psn, window_id)?;
    let deadline = Instant::now() + AX_RAISE_SETTLE_TIMEOUT;
    loop {
        match raise_window(window_id, pid) {
            Err(MacosNativeOperationError::MissingWindow(missing_window_id))
                if missing_window_id == window_id && Instant::now() < deadline =>
            {
                std::thread::sleep(AX_RAISE_RETRY_INTERVAL);
            }
            result => return result,
        }
    }
}

pub(crate) fn focus_window_via_make_key_and_raise<
    WindowPid,
    ProcessSerial,
    MakeKeyWindow,
    RaiseWindow,
>(
    window_id: u64,
    mut window_pid: WindowPid,
    mut process_serial_number: ProcessSerial,
    mut make_key_window: MakeKeyWindow,
    mut raise_window: RaiseWindow,
) -> Result<(), MacosNativeOperationError>
where
    WindowPid: FnMut(u64) -> Result<u32, MacosNativeOperationError>,
    ProcessSerial: FnMut(u32) -> Result<ProcessSerialNumber, MacosNativeOperationError>,
    MakeKeyWindow: FnMut(&ProcessSerialNumber, u64) -> Result<(), MacosNativeOperationError>,
    RaiseWindow: FnMut(u64, u32) -> Result<(), MacosNativeOperationError>,
{
    let pid = window_pid(window_id)?;
    let psn = process_serial_number(pid)?;
    make_key_window(&psn, window_id)?;
    let deadline = Instant::now() + AX_RAISE_SETTLE_TIMEOUT;
    loop {
        match raise_window(window_id, pid) {
            Err(MacosNativeOperationError::MissingWindow(missing_window_id))
                if missing_window_id == window_id && Instant::now() < deadline =>
            {
                std::thread::sleep(AX_RAISE_RETRY_INTERVAL);
            }
            result => return result,
        }
    }
}

use desktop_topology_snapshot::*;
use foundation::*;
use skylight::*;
use window_server::*;

pub(crate) use desktop_topology_snapshot::{
    DESKTOP_SPACE_TYPE, FULLSCREEN_SPACE_TYPE, RawSpaceRecord, RawTopologySnapshot, RawWindow,
    SpaceKind, WindowSnapshot, best_window_id_from_windows, classify_space,
    enrich_real_window_app_ids, enrich_real_window_app_ids_with, ensure_supported_target_space,
    focused_window_from_active_space_windows, focused_window_from_topology,
    native_desktop_snapshot_from_topology, order_active_space_windows,
    parse_lsappinfo_bundle_identifier, snapshots_for_inactive_space, space_id_for_window,
    space_transition_window_ids, stable_app_id_from_real_window, window_ids_for_space,
    window_snapshots_from_topology,
};
pub(crate) use foundation::{
    CfOwned, REQUIRED_PRIVATE_SYMBOLS, SPACE_SWITCH_POLL_INTERVAL, SPACE_SWITCH_SETTLE_TIMEOUT,
    SPACE_SWITCH_STABLE_TARGET_POLLS, array_from_type_refs, array_from_u64s, cf_number_to_u64,
    dictionary_i32, dictionary_string, number_from_u64, string,
};
pub(crate) use skylight::{
    parse_display_identifiers, parse_managed_spaces, parse_raw_space_record, parse_window_ids,
};
pub(crate) use window_server::{
    assemble_real_active_space_windows, filter_window_descriptions_raw, parse_window_descriptions,
};

#[cfg(test)]
pub(crate) mod tests {
    pub(crate) use super::desktop_topology_snapshot::tests::{
        SpaceSnapshot, space_snapshots_from_topology,
    };
    pub(crate) use super::foundation::tests::dictionary_from_type_refs;
    use super::*;

    pub(crate) fn focused_window_id_via_ax<
        App,
        Window,
        FocusedApplication,
        FocusedWindow,
        WindowId,
    >(
        focused_application: FocusedApplication,
        focused_window: FocusedWindow,
        window_id: WindowId,
    ) -> Result<Option<u64>, MacosNativeProbeError>
    where
        FocusedApplication: FnMut() -> Result<Option<App>, MacosNativeProbeError>,
        FocusedWindow: FnMut(&App) -> Result<Option<Window>, MacosNativeProbeError>,
        WindowId: FnMut(&Window) -> Result<u64, MacosNativeProbeError>,
    {
        ax::focused_window_id(focused_application, focused_window, window_id)
    }
}
