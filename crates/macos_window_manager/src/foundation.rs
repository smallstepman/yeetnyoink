#![allow(dead_code)]

use crate::{
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
