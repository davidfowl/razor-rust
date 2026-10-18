use std::{path::Path, process::Command};

#[test]
fn match_exhaustiveness_types_scopes_and_captures_are_compile_checked() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new(env!("CARGO"))
        .args(["test", "--doc", "--manifest-path"])
        .arg(root.join("tests/template-match-negative/Cargo.toml"))
        .arg("--target-dir")
        .arg(root.join("../../target/template-match-negative"))
        .output()
        .expect("run template match compile-contract doctests");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && stdout.contains("10 passed"),
        "template match compile contract failed:\n{stdout}\n{stderr}"
    );
}
