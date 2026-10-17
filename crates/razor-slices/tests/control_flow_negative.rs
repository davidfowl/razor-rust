use std::{path::Path, process::Command};

#[test]
fn invalid_grammar_types_capture_and_lifetimes_fail_to_compile() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new(env!("CARGO"))
        .args(["test", "--doc", "--manifest-path"])
        .arg(root.join("tests/control-flow-negative/Cargo.toml"))
        .arg("--target-dir")
        .arg(root.join("../../target/control-flow-negative"))
        .output()
        .expect("run control-flow compile-fail doctests");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && stdout.contains("9 passed"),
        "control-flow compile-fail contract failed:\n{stdout}\n{stderr}"
    );
}
