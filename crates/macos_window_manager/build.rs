use std::{
    env,
    path::PathBuf,
    process::Command,
};

fn main() {
    println!("cargo:rerun-if-changed=swift/Package.swift");
    println!("cargo:rerun-if-changed=swift/Sources");
    println!("cargo:rerun-if-changed=swift/Tests");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir should exist"));
    let package_path = manifest_dir.join("swift");
    let configuration = match env::var("PROFILE").as_deref() {
        Ok("release") => "release",
        _ => "debug",
    };

    run_swift(&package_path, configuration, &["--product", "MacosWindowManagerFFI"]);
    let bin_path = swift_output(&package_path, configuration, &["--show-bin-path"]);

    println!("cargo:rustc-link-search=native={bin_path}");
    println!("cargo:rustc-link-lib=static=MacosWindowManagerFFI");
}

fn run_swift(package_path: &PathBuf, configuration: &str, extra_args: &[&str]) {
    let status = Command::new("swift")
        .arg("build")
        .arg("--package-path")
        .arg(package_path)
        .arg("--configuration")
        .arg(configuration)
        .args(extra_args)
        .status()
        .expect("swift build should start");

    if !status.success() {
        panic!("swift build failed with status {status}");
    }
}

fn swift_output(package_path: &PathBuf, configuration: &str, extra_args: &[&str]) -> String {
    let output = Command::new("swift")
        .arg("build")
        .arg("--package-path")
        .arg(package_path)
        .arg("--configuration")
        .arg(configuration)
        .args(extra_args)
        .output()
        .expect("swift build output command should start");

    if !output.status.success() {
        panic!(
            "swift build output command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    String::from_utf8(output.stdout)
        .expect("swift build output should be utf-8")
        .trim()
        .to_owned()
}
