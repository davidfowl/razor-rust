use std::{path::Path, process::Command};

#[test]
fn helper_method_and_delegate_composition_is_compile_checked() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new(env!("CARGO"))
        .args(["test", "--doc", "--manifest-path"])
        .arg(root.join("tests/helpers-negative/Cargo.toml"))
        .arg("--target-dir")
        .arg(root.join("../../target/helpers-negative"))
        .output()
        .expect("run helper compile-contract doctests");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && stdout.contains("9 passed"),
        "helper compile contract failed:\n{stdout}\n{stderr}"
    );
}
