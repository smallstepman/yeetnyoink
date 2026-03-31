use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=swift/Package.swift");
    println!("cargo:rerun-if-changed=swift/Sources");
    println!("cargo:rerun-if-changed=swift/Tests");
    println!("cargo:rerun-if-env-changed=TARGET");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_ARCH");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
    println!("cargo:rerun-if-env-changed=OUT_DIR");

    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS should exist");
    if target_os != "macos" {
        panic!("macos_window_manager only supports macOS targets (got {target_os})");
    }

    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir should exist"));
    let package_path = manifest_dir.join("swift");
    let scratch_path =
        PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR should exist")).join("swiftpm");
    let configuration = match env::var("PROFILE").as_deref() {
        Ok("release") => "release",
        _ => "debug",
    };
    let target_triple = swift_target_triple();

    fs::create_dir_all(&scratch_path).expect("swift scratch directory should be creatable");

    run_swift(
        &package_path,
        &scratch_path,
        configuration,
        &target_triple,
        &["--product", "MacosWindowManagerFFI"],
    );
    let bin_path = swift_output(
        &package_path,
        &scratch_path,
        configuration,
        &target_triple,
        &["--show-bin-path"],
    );

    println!("cargo:rustc-link-search=native={bin_path}");
    println!("cargo:rustc-link-lib=static=MacosWindowManagerFFI");
}

fn swift_target_triple() -> String {
    let target = env::var("TARGET").expect("TARGET should exist");
    let arch = match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("aarch64") => "arm64",
        Ok("x86_64") => "x86_64",
        Ok(other) => panic!("unsupported macOS target arch {other}"),
        Err(_) => panic!("CARGO_CFG_TARGET_ARCH should exist"),
    };

    if !target.ends_with("-apple-darwin") {
        panic!("unsupported target triple {target}");
    }

    format!("{arch}-apple-macosx")
}

fn swift_command(
    package_path: &PathBuf,
    scratch_path: &PathBuf,
    configuration: &str,
    target_triple: &str,
) -> Command {
    let mut command = Command::new("swift");
    command
        .arg("build")
        .arg("--package-path")
        .arg(package_path)
        .arg("--scratch-path")
        .arg(scratch_path)
        .arg("--configuration")
        .arg(configuration)
        .arg("--triple")
        .arg(target_triple);
    command
}

fn run_swift(
    package_path: &PathBuf,
    scratch_path: &PathBuf,
    configuration: &str,
    target_triple: &str,
    extra_args: &[&str],
) {
    let status = swift_command(package_path, scratch_path, configuration, target_triple)
        .args(extra_args)
        .status()
        .expect("swift build should start");

    if !status.success() {
        panic!("swift build failed with status {status}");
    }
}

fn swift_output(
    package_path: &PathBuf,
    scratch_path: &PathBuf,
    configuration: &str,
    target_triple: &str,
    extra_args: &[&str],
) -> String {
    let output = swift_command(package_path, scratch_path, configuration, target_triple)
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
