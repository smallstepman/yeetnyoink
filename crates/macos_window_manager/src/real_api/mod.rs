#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(target_os = "macos"))]
mod stub;

#[cfg(target_os = "macos")]
pub use macos::RealNativeApi;
#[cfg(not(target_os = "macos"))]
pub use stub::RealNativeApi;
