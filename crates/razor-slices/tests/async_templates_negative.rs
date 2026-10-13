use std::{path::Path, process::Command};

#[test]
fn declaration_grammar_model_lifetime_send_and_unused_await_contracts() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new(env!("CARGO"))
        .args(["test", "--doc", "--manifest-path"])
        .arg(root.join("tests/async-template-negative/Cargo.toml"))
        .arg("--target-dir")
        .arg(root.join("../../target/async-template-negative"))
        .output()
        .expect("run async declaration compile-fail doctests");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && stdout.contains("32 passed"),
        "async declaration compile-fail contract failed:\n{stdout}\n{stderr}"
    );
}
