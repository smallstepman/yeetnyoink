//! Adapter-owned window manager glue.
//!
//! The shared capability/planning contract is engine-owned and must not be
//! imported from this adapter module.
//!
//! ```compile_fail
//! use yeet_and_yoink::adapters::window_managers::WindowManagerCapabilities;
//! ```
//!
//! ```compile_fail
//! use yeet_and_yoink::adapters::window_managers::plan_tear_out;
//! ```
//!
#[cfg(target_os = "linux")]
pub mod i3;
#[cfg(any(test, target_os = "linux"))]
pub mod niri;
#[cfg(target_os = "macos")]
pub mod paneru;
#[cfg(target_os = "macos")]
pub mod yabai;

#[cfg(any(test, target_os = "linux"))]
pub use self::niri::NiriAdapter;

#[cfg(target_os = "linux")]
use crate::adapters::window_managers::i3::I3_SPEC;
#[cfg(target_os = "linux")]
use crate::adapters::window_managers::niri::NIRI_SPEC;
#[cfg(target_os = "macos")]
use crate::adapters::window_managers::paneru::PANERU_SPEC;
#[cfg(target_os = "macos")]
use crate::adapters::window_managers::yabai::YABAI_SPEC;
use crate::config::WmBackend;
use crate::engine::wm::configured::WindowManagerSpec;
#[cfg(not(target_os = "linux"))]
pub(crate) use crate::engine::wm::configured::UNSUPPORTED_I3_SPEC;
#[cfg(not(target_os = "linux"))]
pub(crate) use crate::engine::wm::configured::UNSUPPORTED_NIRI_SPEC;
#[cfg(not(target_os = "macos"))]
pub(crate) use crate::engine::wm::configured::UNSUPPORTED_PANERU_SPEC;
#[cfg(not(target_os = "macos"))]
pub(crate) use crate::engine::wm::configured::UNSUPPORTED_YABAI_SPEC;

pub fn spec_for_backend(backend: WmBackend) -> &'static dyn WindowManagerSpec {
    match backend {
        WmBackend::Niri => {
            #[cfg(target_os = "linux")]
            {
                &NIRI_SPEC
            }
            #[cfg(not(target_os = "linux"))]
            {
                &UNSUPPORTED_NIRI_SPEC
            }
        }
        WmBackend::I3 => {
            #[cfg(target_os = "linux")]
            {
                &I3_SPEC
            }
            #[cfg(not(target_os = "linux"))]
            {
                &UNSUPPORTED_I3_SPEC
            }
        }
        WmBackend::Paneru => {
            #[cfg(target_os = "macos")]
            {
                &PANERU_SPEC
            }
            #[cfg(not(target_os = "macos"))]
            {
                &UNSUPPORTED_PANERU_SPEC
            }
        }
        WmBackend::Yabai => {
            #[cfg(target_os = "macos")]
            {
                &YABAI_SPEC
            }
            #[cfg(not(target_os = "macos"))]
            {
                &UNSUPPORTED_YABAI_SPEC
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::WindowManagerSpec;
    use crate::config::WmBackend;
    use crate::engine::window_manager::{
        CapabilitySupport, WindowCycleProvider, WindowManagerCapabilityDescriptor,
        WindowManagerSession, WindowTearOutComposer,
    };

    #[test]
    fn built_in_specs_match_window_manager_contract() {
        fn assert_spec(_spec: &'static dyn WindowManagerSpec) {}

        assert_spec(super::spec_for_backend(WmBackend::Niri));
        assert_spec(super::spec_for_backend(WmBackend::I3));
        assert_spec(super::spec_for_backend(WmBackend::Paneru));
        assert_spec(super::spec_for_backend(WmBackend::Yabai));
    }

    #[test]
    fn niri_backend_wrapper_remains_available_from_adapter_boundary() {
        fn assert_niri_traits<T>()
        where
            T: WindowManagerCapabilityDescriptor
                + WindowManagerSession
                + WindowCycleProvider
                + WindowTearOutComposer,
        {
        }

        type Adapter = crate::adapters::window_managers::NiriAdapter;

        assert_niri_traits::<Adapter>();

        let spec = super::spec_for_backend(WmBackend::Niri);
        let capabilities = <Adapter as WindowManagerCapabilityDescriptor>::CAPABILITIES;

        assert_eq!(spec.backend(), WmBackend::Niri);
        assert_eq!(
            spec.name(),
            <Adapter as WindowManagerCapabilityDescriptor>::NAME
        );
        capabilities
            .validate()
            .expect("re-exported niri adapter capabilities should stay valid after relocation");
        assert_eq!(capabilities.tear_out.east, CapabilitySupport::Native);
        assert_eq!(capabilities.tear_out.west, CapabilitySupport::Composed);
        assert!(capabilities.primitives.move_column);
        assert!(capabilities.primitives.consume_into_column_and_move);
    }
}
