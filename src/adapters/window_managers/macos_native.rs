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
#[allow(dead_code, unused_imports)]
mod macos_window_manager_test_support {
    use std::{
        collections::{HashMap, HashSet},
        ffi::{CString, c_void},
        time::Instant,
    };
    pub(crate) use macos_window_manager::{
        ActiveSpaceFocusTargetHint, MacosNativeApi, MacosNativeConnectError,
        MacosNativeOperationError, MacosNativeProbeError, MissionControlHotkey,
        MissionControlModifiers, NativeBackendOptions, NativeBounds, NativeDesktopSnapshot,
        NativeDiagnostics, NativeDirection, NativeSpaceSnapshot, NativeWindowId,
        NativeWindowSnapshot,
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
        pub(crate) type SlsCopyManagedDisplayForSpaceFn =
            unsafe extern "C" fn(u32, u64) -> CFStringRef;
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
                unsafe { Self::from_create_rule(raw) }
                    .expect("Servo CF wrappers should never be null")
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
            PostKeyEvent:
                FnMut(CGKeyCode, bool, CGEventFlags) -> Result<(), MacosNativeOperationError>,
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
                .map(|value| {
                    cf_type(*value).expect("array_from_type_refs expects non-null CFTypeRef")
                })
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

        pub(crate) fn dictionary_string(
            dictionary: CFDictionaryRef,
            key: &CFString,
        ) -> Option<String> {
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

        pub(crate) fn cf_dictionary_u64(
            dictionary: CFDictionaryRef,
            key: CFStringRef,
        ) -> Option<u64> {
            let key = unsafe {
                core_foundation::string::CFString::wrap_under_get_rule(
                    key as core_foundation::string::CFStringRef,
                )
            };
            dictionary_u64(dictionary, &key)
        }

        pub(crate) fn cf_dictionary_u32(
            dictionary: CFDictionaryRef,
            key: CFStringRef,
        ) -> Option<u32> {
            let key = unsafe {
                core_foundation::string::CFString::wrap_under_get_rule(
                    key as core_foundation::string::CFStringRef,
                )
            };
            dictionary_u32(dictionary, &key)
        }

        pub(crate) fn cf_dictionary_i32(
            dictionary: CFDictionaryRef,
            key: CFStringRef,
        ) -> Option<i32> {
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
                            cf_type(*value)
                                .expect("dictionary_from_type_refs expects non-null values"),
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
            pub(crate) fn AXValueCreate(
                value_type: AXValueType,
                value_ptr: *const c_void,
            ) -> CFTypeRef;
        }

        pub(crate) fn is_process_trusted(api: &RealNativeApi) -> bool {
            let Some(symbol) = api.resolve_symbol("AXIsProcessTrusted") else {
                return false;
            };

            let ax_is_process_trusted: AxIsProcessTrustedFn =
                unsafe { std::mem::transmute(symbol) };
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
            let status = unsafe {
                AXUIElementCopyAttributeValue(element, attribute.as_type_ref(), &mut value)
            };

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
                    let _span =
                        tracing::debug_span!("macos_native.ax.focused_application").entered();
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
            SlsCopyManagedDisplaySpacesFn, SlsCopyWindowsWithOptionsAndTagsFn,
            SlsMainConnectionIdFn, SlsManagedDisplayGetCurrentSpaceFn,
            SlsManagedDisplaySetCurrentSpaceFn, array_from_type_refs, cf_array_iter,
            cf_as_dictionary, cf_dictionary_array, cf_dictionary_dictionary, cf_dictionary_i32,
            cf_dictionary_string, cf_dictionary_u64, cf_number_from_u64, cf_number_to_u64,
            cf_string,
        };
        use super::{
            MacosNativeOperationError, MacosNativeProbeError, RawSpaceRecord, RealNativeApi,
        };
        use std::collections::HashSet;

        pub(crate) fn main_connection_id(
            api: &RealNativeApi,
        ) -> Result<u32, MacosNativeProbeError> {
            let Some(symbol) = api.resolve_symbol("SLSMainConnectionID") else {
                return Err(MacosNativeProbeError::MissingTopology(
                    "SLSMainConnectionID",
                ));
            };

            let main_connection_id: SlsMainConnectionIdFn = unsafe { std::mem::transmute(symbol) };
            let connection_id = unsafe { main_connection_id() };

            (connection_id != 0).then_some(connection_id).ok_or(
                MacosNativeProbeError::MissingTopology("SLSMainConnectionID"),
            )
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
            let space_id = unsafe {
                current_space_for_display(connection_id, display_identifier.as_type_ref())
            };

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
            let space_list =
                CfOwned::from_servo(array_from_type_refs(&[space_number.as_type_ref()]));
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
            let window_list =
                CfOwned::from_servo(array_from_type_refs(&[window_number.as_type_ref()]));

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
                            cf_dictionary_dictionary(display, current_space_key.as_type_ref())
                                .and_then(|current_space| {
                                    cf_dictionary_u64(
                                        current_space,
                                        managed_space_id_key.as_type_ref(),
                                    )
                                    .or_else(|| {
                                        cf_dictionary_u64(current_space, id64_key.as_type_ref())
                                    })
                                })
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

            let managed_space_id = cf_dictionary_u64(space, managed_space_id_key.as_type_ref())
                .ok_or(MacosNativeProbeError::MissingTopology(
                    "SLSCopyManagedDisplaySpaces",
                ))?;
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
                                    .or_else(|| {
                                        cf_dictionary_u64(tile_space, id64_key.as_type_ref())
                                    })
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

        pub(crate) fn parse_window_ids(
            payload: CFArrayRef,
        ) -> Result<Vec<u64>, MacosNativeProbeError> {
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
            CGWindowListCreateDescriptionFromArray, CPS_USER_GENERATED, CfOwned,
            GetProcessForPidFn, K_CG_HID_EVENT_TAP, K_CG_NULL_WINDOW_ID,
            K_CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS, K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY,
            ProcessSerialNumber, SlpsPostEventRecordToFn, SlpsSetFrontProcessWithOptionsFn,
            array_from_type_refs, cf_array_count, cf_array_iter, cf_as_dictionary,
            cf_dictionary_dictionary, cf_dictionary_i32, cf_dictionary_string, cf_dictionary_u32,
            cf_dictionary_u64, cf_number_from_u64, cf_string,
        };
        use super::skylight;
        use super::{
            MacosNativeOperationError, MacosNativeProbeError, NativeBounds, RawWindow,
            RealNativeApi, enrich_real_window_app_ids, focus_window_via_process_and_raise,
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
            let status = unsafe {
                front_process_with_options(psn, window_id as CGWindowID, CPS_USER_GENERATED)
            };

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
            let descriptions =
                copy_window_descriptions_raw(api, payload.as_type_ref() as CFArrayRef)?;
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
            let window_list =
                CfOwned::from_servo(array_from_type_refs(&[window_number.as_type_ref()]));
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

        pub(crate) fn copy_onscreen_window_descriptions_raw()
        -> Result<CfOwned, MacosNativeProbeError> {
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
                let description =
                    cf_as_dictionary(description).ok_or(MacosNativeProbeError::MissingTopology(
                        "CGWindowListCreateDescriptionFromArray",
                    ))?;
                let id = cf_dictionary_u64(description, window_number_key).ok_or(
                    MacosNativeProbeError::MissingTopology(
                        "CGWindowListCreateDescriptionFromArray",
                    ),
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
            ActiveSpaceFocusTargetHint, MacosNativeOperationError, MacosNativeProbeError,
            NativeBounds, NativeDesktopSnapshot, NativeDirection, NativeSpaceSnapshot,
            NativeWindowSnapshot,
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
            let _span =
                tracing::debug_span!("macos_native.app_id_from_pid.lsappinfo", pid).entered();
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
                if let Some(snapshot) =
                    active_space_windows.iter().find_map(|(space_id, windows)| {
                        active_window_snapshot(*space_id, windows, target_window_id)
                    })
                {
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
                    active_window_snapshot(
                        space_id,
                        active_space_windows.get(&space_id)?,
                        window.id,
                    )
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
            topology.active_space_ids.iter().copied().find(|space_id| {
                display_index_for_space(topology, *space_id) == Some(display_index)
            })
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
            let target_visible = target_window_ids.is_empty()
                || !target_window_ids.is_disjoint(&onscreen_window_ids);
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
            super::outer_space_transition_window_ids(snapshot, space_id);
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
            match wait_for_space_presentation(
                api,
                space_id,
                source_focus_window_id,
                &target_window_ids,
            ) {
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

    fn native_window(
        snapshot: &NativeDesktopSnapshot,
        window_id: u64,
    ) -> Option<&NativeWindowSnapshot> {
        snapshot
            .windows
            .iter()
            .find(|window| window.id == window_id)
    }

    fn native_space(
        snapshot: &NativeDesktopSnapshot,
        space_id: u64,
    ) -> Option<&NativeSpaceSnapshot> {
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
            .max_by(|left, right| {
                compare_native_windows_for_target_match(target_bounds, left, right)
            })
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
        candidates.sort_by(|left, right| {
            compare_native_windows_for_edge(left, right, direction).reverse()
        });

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
            api.debug(
                "macos_native: split-view peer remap found no same-app directional candidates",
            );
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
            if let Some((peer_target_id, peer_pid)) =
                focusable_same_app_split_view_peer_from_source(
                    api,
                    &refreshed_snapshot,
                    original_focused.id,
                    direction,
                    refreshed_target_id,
                )?
            {
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
        let Some(pid) = native_window(snapshot, focus_target_id).and_then(|window| window.pid)
        else {
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
                    refreshed_split_view_focus_target(
                        api,
                        snapshot,
                        direction,
                        focus_target_id,
                        pid,
                    )?
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
            let symbol =
                CString::new(symbol).expect("required symbol names should not contain NULs");

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
            let display_identifiers =
                parse_display_identifiers(payload.as_type_ref() as CFArrayRef)?;
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

        fn active_space_windows(
            &self,
            space_id: u64,
        ) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
            let payload = skylight::copy_windows_for_space_raw(self, space_id)?;
            let visible_order = query_visible_window_order(&parse_window_ids(
                payload.as_type_ref() as CFArrayRef,
            )?)?;
            let descriptions = window_server::copy_window_descriptions_raw(
                self,
                payload.as_type_ref() as CFArrayRef,
            )?;

            assemble_real_active_space_windows(
                descriptions.as_type_ref() as CFArrayRef,
                &visible_order,
            )
        }

        fn inactive_space_window_ids(
            &self,
        ) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
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
            switch_adjacent_space_via_hotkey(
                &self.options,
                direction,
                |key_code, key_down, flags| {
                    window_server::post_keyboard_event(self, key_code, key_down, flags)
                },
            )
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
        FrontProcessWindow:
            FnMut(&ProcessSerialNumber, u64) -> Result<(), MacosNativeOperationError>,
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
        best_window_id_from_windows, classify_space, ensure_supported_target_space,
        enrich_real_window_app_ids, enrich_real_window_app_ids_with,
        focused_window_from_active_space_windows, focused_window_from_topology,
        native_desktop_snapshot_from_topology, order_active_space_windows,
        parse_lsappinfo_bundle_identifier, space_id_for_window, space_transition_window_ids,
        snapshots_for_inactive_space, stable_app_id_from_real_window,
        window_ids_for_space, window_snapshots_from_topology, DESKTOP_SPACE_TYPE,
        FULLSCREEN_SPACE_TYPE, RawSpaceRecord, RawTopologySnapshot, RawWindow, SpaceKind,
        WindowSnapshot,
    };
    pub(crate) use foundation::{
        array_from_type_refs, array_from_u64s, cf_number_to_u64, dictionary_i32,
        dictionary_string, number_from_u64, string, CfOwned, REQUIRED_PRIVATE_SYMBOLS,
        SPACE_SWITCH_POLL_INTERVAL, SPACE_SWITCH_SETTLE_TIMEOUT,
        SPACE_SWITCH_STABLE_TARGET_POLLS,
    };
    pub(crate) use skylight::{
        parse_display_identifiers, parse_managed_spaces, parse_raw_space_record, parse_window_ids,
    };
    pub(crate) use window_server::{
        assemble_real_active_space_windows, filter_window_descriptions_raw,
        parse_window_descriptions,
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
}

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
    use super::macos_window_manager_test_support::tests::{
        SpaceSnapshot, dictionary_from_type_refs, focused_window_id_via_ax,
        space_snapshots_from_topology,
    };
    use super::macos_window_manager_test_support::{
        CfOwned, DESKTOP_SPACE_TYPE, FULLSCREEN_SPACE_TYPE, REQUIRED_PRIVATE_SYMBOLS,
        RawSpaceRecord, RawTopologySnapshot, RawWindow, SPACE_SWITCH_POLL_INTERVAL,
        SPACE_SWITCH_SETTLE_TIMEOUT, SPACE_SWITCH_STABLE_TARGET_POLLS, WindowSnapshot,
        array_from_type_refs, array_from_u64s, best_window_id_from_windows, classify_space, dictionary_i32,
        dictionary_string, ensure_supported_target_space, focused_window_from_topology,
        enrich_real_window_app_ids_with, focus_window_via_make_key_and_raise,
        focus_window_via_process_and_raise, native_desktop_snapshot_from_topology,
        number_from_u64, order_active_space_windows,
        parse_lsappinfo_bundle_identifier, parse_managed_spaces, parse_raw_space_record,
        snapshots_for_inactive_space, space_id_for_window,
        space_transition_window_ids, stable_app_id_from_real_window, string,
        validate_environment_with_api, window_ids_for_space, window_snapshots_from_topology,
    };
    use super::macos_window_manager_test_support::foundation::{
        CFArrayRef, CFDictionaryRef, CFTypeRef, K_CG_EVENT_FLAG_MASK_ALTERNATE,
        K_CG_EVENT_FLAG_MASK_COMMAND, K_CG_EVENT_FLAG_MASK_SHIFT,
        ProcessSerialNumber, cf_number_from_u64, cf_string, switch_adjacent_space_via_hotkey,
    };
    use super::macos_window_manager_test_support::window_server::{
        cg_window_bounds_key, cg_window_layer_key, cg_window_name_key, cg_window_number_key,
        cg_window_owner_pid_key, filter_window_descriptions_raw, parse_window_descriptions,
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
            assert!(
                next.is_some_and(|line| {
                    line.ends_with("mod tests {")
                        || line.ends_with("mod macos_window_manager_test_support {")
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
