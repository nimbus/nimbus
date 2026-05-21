use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[test]
#[ignore = "requires local Bun checkout and external Bun build prerequisites"]
fn bun_jsc_build_gate_reproduces_from_bun_build_graph() {
    let bun_repo = bun_repo();
    assert!(
        bun_repo.join("src/jsc/Cargo.toml").is_file(),
        "NIMBUS_BUN_REPO must point at a Bun checkout; missing src/jsc/Cargo.toml under {}",
        bun_repo.display()
    );

    let build_dir = proof_path("NIMBUS_BUN_BUILD_DIR", "nimbus-bun-rust-only");
    let cache_dir = proof_path("NIMBUS_BUN_CACHE_DIR", "nimbus-bun-cache");
    let cargo_target_dir = proof_path("NIMBUS_BUN_CARGO_TARGET_DIR", "nimbus-bun-proof-target");

    let setup_output = Command::new("bun")
        .current_dir(&bun_repo)
        .arg("scripts/build.ts")
        .arg("--profile=ci-rust-only")
        .arg(format!("--build-dir={}", build_dir.display()))
        .arg(format!("--cache-dir={}", cache_dir.display()))
        .arg("--target=clone-lolhtml")
        .arg("--target=codegen")
        .output()
        .expect("failed to spawn Bun build setup command");
    assert_success("Bun ci-rust-only setup", &setup_output);

    let codegen_dir = build_dir.join("codegen");
    assert_required_file(&bun_repo.join("vendor/lolhtml/c-api/Cargo.toml"));
    assert_required_file(&codegen_dir.join("generated_classes.rs"));
    assert_required_file(&codegen_dir.join("generated_host_exports.rs"));
    assert_required_file(&codegen_dir.join("cpp.rs"));

    let mut cargo = Command::new("cargo");
    sanitize_nested_cargo(&mut cargo);
    let cargo_output = cargo
        .current_dir(&bun_repo)
        .env("CARGO_TARGET_DIR", &cargo_target_dir)
        .env("BUN_CODEGEN_DIR", &codegen_dir)
        // Bun's generated cargo config currently passes `-fuse-ld=lld` on
        // macOS; clearing rustflags matches the documented one-off proof gate.
        .env("CARGO_ENCODED_RUSTFLAGS", "")
        .arg("check")
        .arg("-p")
        .arg("bun_jsc")
        .arg("--lib")
        .output()
        .expect("failed to spawn Bun cargo check command");
    assert_success("Bun bun_jsc cargo check", &cargo_output);
}

fn bun_repo() -> PathBuf {
    if let Some(path) = env::var_os("NIMBUS_BUN_REPO") {
        return PathBuf::from(path);
    }

    let home = env::var_os("HOME").expect("HOME must be set or NIMBUS_BUN_REPO must be provided");
    PathBuf::from(home).join("src/github.com/oven-sh/bun")
}

fn proof_path(env_key: &str, leaf: &str) -> PathBuf {
    env::var_os(env_key)
        .map(PathBuf::from)
        .unwrap_or_else(|| proof_root().join(leaf))
}

fn proof_root() -> PathBuf {
    let private_tmp = PathBuf::from("/private/tmp");
    if private_tmp.is_dir() {
        return private_tmp;
    }

    env::temp_dir()
}

fn assert_required_file(path: &Path) {
    assert!(
        path.is_file(),
        "required Bun proof artifact missing: {}",
        path.display()
    );
}

fn assert_success(label: &str, output: &Output) {
    if output.status.success() {
        return;
    }

    panic!(
        "{label} failed with status {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn sanitize_nested_cargo(command: &mut Command) {
    for key in [
        "CARGO",
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO_MAKEFLAGS",
        "CARGO_TARGET_DIR",
        "RUSTC",
        "RUSTC_WORKSPACE_WRAPPER",
        "RUSTC_WRAPPER",
        "RUSTDOC",
        "RUSTFLAGS",
        "RUSTUP_TOOLCHAIN",
    ] {
        command.env_remove(key);
    }
}
