use razor_slices::{async_template, AsyncHtmlWriter, AsyncSlice, AsyncSliceWriter};
use std::{
    io,
    path::Path,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Model(AtomicUsize);

impl Model {
    fn touch(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

async_template! {
    struct CodeBlocks<'a> { model: &'a Model } => {
        @{
            let label = "<local>";
            model.touch()
        }
        {label}
        if true {
            @{ #[allow(unused_must_use)] model.touch() }
        }
        for _ in 0..2 {
            @{ async { model.touch() }.await }
        }
        @{ model.touch(); }
        "done"
    }
}

#[derive(Default)]
struct Writer(Vec<u8>);

impl AsyncSliceWriter for Writer {
    async fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.0.extend_from_slice(bytes);
        Ok(())
    }

    async fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn unit_code_tails_keep_bindings_and_execute_in_nested_template_bodies() {
    let model = Model(AtomicUsize::new(0));
    let page = CodeBlocks::new(&model);
    assert_eq!(model.0.load(Ordering::SeqCst), 0);
    for calls in [5, 10] {
        let mut writer = Writer::default();
        page.render_into(&mut AsyncHtmlWriter::new(&mut writer))
            .await
            .unwrap();
        assert_eq!(writer.0, b"&lt;local&gt;done");
        assert_eq!(model.0.load(Ordering::SeqCst), calls);
    }
}

#[test]
fn code_tails_are_unit_valued_and_do_not_discard_results_or_futures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new(env!("CARGO"))
        .args(["test", "--doc", "--manifest-path"])
        .arg(root.join("tests/compiler-code-blocks-negative/Cargo.toml"))
        .arg("--target-dir")
        .arg(root.join("../../target/compiler-code-blocks-negative"))
        .output()
        .expect("run code-block tail compile-fail doctests");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && stdout.contains("4 passed"),
        "code-block tail compile-fail contract failed:\n{stdout}\n{stderr}"
    );
}
