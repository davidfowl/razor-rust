use std::{path::Path, process::Command};

#[test]
fn synchronous_block_tail_scope_and_capture_contracts_are_compile_checked() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new(env!("CARGO"))
        .args(["test", "--doc", "--manifest-path"])
        .arg(root.join("tests/sync-code-blocks-negative/Cargo.toml"))
        .arg("--target-dir")
        .arg(root.join("../../target/sync-code-blocks-negative"))
        .output()
        .expect("run synchronous block compile-fail doctests");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && stdout.contains("9 passed"),
        "synchronous block compile contract failed:\n{stdout}\n{stderr}"
    );
}
