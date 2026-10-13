use std::{
    error::Error,
    future::Future,
    io,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Poll, Wake, Waker},
};

use razor_slices::{
    async_template, render_incremental, with_async_section, AsyncHtmlWriter, AsyncRenderError,
    AsyncSlice, AsyncSliceWriter,
};
use tokio::sync::Notify;

#[derive(Debug)]
struct Original(Arc<()>);

impl std::fmt::Display for Original {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("original failure")
    }
}

impl Error for Original {}

fn failure(marker: &Arc<()>) -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, Original(marker.clone()))
}

#[derive(Default)]
struct Writer {
    bytes: Vec<u8>,
    lengths: Vec<usize>,
    flushes: usize,
    fail_write: Option<usize>,
    fail_flush: bool,
    marker: Arc<()>,
}

impl AsyncSliceWriter for Writer {
    async fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        let index = self.lengths.len();
        self.lengths.push(bytes.len());
        if self.fail_write == Some(index) {
            return Err(failure(&self.marker));
        }
        self.bytes.extend(bytes);
        Ok(())
    }

    async fn flush(&mut self) -> io::Result<()> {
        self.flushes += 1;
        if self.fail_flush {
            return Err(failure(&self.marker));
        }
        Ok(())
    }
}

fn assert_original(error: AsyncRenderError, marker: &Arc<()>) {
    assert!(error.source().is_some());
    let AsyncRenderError::Write(error) = error else {
        panic!("expected original I/O error");
    };
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    let original = error
        .get_ref()
        .unwrap()
        .downcast_ref::<Original>()
        .expect("original error payload");
    assert!(Arc::ptr_eq(&original.0, marker));
}

async_template! {
    #[derive(Debug)]
    struct Owned { model: String } => {
        "<p>" {model} "</p>"
    }
}

async_template! {
    struct Generic<'a, __RazorWriter>
    where __RazorWriter: AsRef<str> + Sync
    { model: &'a __RazorWriter } => {
        @{ let __razor_writer = "local"; }
        {__razor_writer} {model.as_ref()}
    }
}

async_template! {
    struct Empty { model: bool } => {}
}

#[tokio::test]
async fn nested_awaits_bind_locals_and_only_execute_selected_iterations() {
    struct Rows<'a> {
        values: &'a [&'a str],
        calls: &'a AtomicUsize,
    }
    async_template! {
        struct LoopPage<'a> { model: Rows<'a> } => {
            for (index, value) in model.values.iter().enumerate() {
                if value.is_empty() {
                    @await { async {} };
                } else if index == 1 {
                    @{ let label = async { "<second>" }.await; }
                    {label} {value}
                    @await { async { model.calls.fetch_add(1, Ordering::SeqCst); } };
                    @flush;
                } else {
                    {value}
                    @await { async { model.calls.fetch_add(1, Ordering::SeqCst); } };
                }
            }
        }
    }
    let calls = AtomicUsize::new(0);
    let page = LoopPage::new(Rows {
        values: &["", "<item>", "&last"],
        calls: &calls,
    });
    for count in [2, 4] {
        let mut writer = Writer::default();
        render_incremental(&page, &mut writer).await.unwrap();
        assert_eq!(writer.bytes, b"&lt;second&gt;&lt;item&gt;&amp;last");
        assert_eq!(writer.flushes, 2);
        assert_eq!(calls.load(Ordering::SeqCst), count);
    }
}

#[tokio::test]
async fn errors_in_nested_awaits_skip_remaining_iterations_and_keep_render_variant() {
    async_template! {
        struct FailLoop<'a> { model: &'a AtomicUsize } => {
            "prefix"
            for _ in 0..3 {
                @await { async {
                    model.fetch_add(1, Ordering::SeqCst);
                    Err::<(), _>(razor_slices::RenderError(std::fmt::Error))
                } }?;
                "never"
            }
            "suffix"
        }
    }
    let calls = AtomicUsize::new(0);
    let mut writer = Writer::default();
    let error = render_incremental(&FailLoop::new(&calls), &mut writer)
        .await
        .unwrap_err();
    assert!(matches!(error, AsyncRenderError::Render(_)));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(writer.bytes, b"prefix");
    assert_eq!(writer.flushes, 0);
}

#[tokio::test]
async fn owned_borrowed_generic_models_and_hygienic_locals_are_reusable() {
    let owned = Owned::new(String::from("<&>\"'"));
    for _ in 0..2 {
        let mut writer = Writer::default();
        let mut output = AsyncHtmlWriter::new(&mut writer);
        owned.render_into(&mut output).await.unwrap();
        assert_eq!(writer.bytes, b"<p>&lt;&amp;&gt;&quot;&#39;</p>");
        assert_eq!(writer.flushes, 0);
    }
    let borrowed = String::from("<borrowed>");
    let mut writer = Writer::default();
    render_incremental(&Generic::new(&borrowed), &mut writer)
        .await
        .unwrap();
    assert_eq!(writer.bytes, b"local&lt;borrowed&gt;");
    assert_eq!(writer.flushes, 1);
    render_incremental(&Empty::new(false), &mut writer)
        .await
        .unwrap();
    assert_eq!(writer.flushes, 2);
}

struct Work<'a> {
    calls: &'a AtomicUsize,
    marker: &'a Arc<()>,
    fail: bool,
}

impl Work<'_> {
    async fn step(&self) -> io::Result<()> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            return Err(failure(self.marker));
        }
        Ok(())
    }

    async fn value(&self) -> io::Result<String> {
        self.step().await?;
        Ok(String::from("<value>"))
    }
}

async_template! {
    struct Steps<'a> { model: Work<'a> } => {
        "prefix"
        @flush;
        @await { model.step() }?;
        @{ let value = model.value().await?; }
        {value}
        @await { async {} };
        "suffix"
    }
}

#[tokio::test]
async fn application_work_is_lazy_recreated_and_await_results_are_encoded() {
    let calls = AtomicUsize::new(0);
    let marker = Arc::new(());
    let page = Steps::new(Work {
        calls: &calls,
        marker: &marker,
        fail: false,
    });
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    for count in [2, 4] {
        let mut writer = Writer::default();
        render_incremental(&page, &mut writer).await.unwrap();
        assert_eq!(writer.bytes, b"prefix&lt;value&gt;suffix");
        assert_eq!(writer.flushes, 2);
        assert_eq!(calls.load(Ordering::SeqCst), count);
    }
}

#[tokio::test]
async fn original_write_flush_and_application_errors_stop_later_work() {
    for (fail_write, fail_flush, fail_work, expected_calls, expected_flushes, prefix) in [
        (Some(0), false, false, 0, 0, b"".as_slice()),
        (None, true, false, 0, 1, b"prefix".as_slice()),
        (None, false, true, 1, 1, b"prefix".as_slice()),
        (Some(1), false, false, 2, 1, b"prefix".as_slice()),
    ] {
        let calls = AtomicUsize::new(0);
        let marker = Arc::new(());
        let page = Steps::new(Work {
            calls: &calls,
            marker: &marker,
            fail: fail_work,
        });
        let mut writer = Writer {
            fail_write,
            fail_flush,
            marker: marker.clone(),
            ..Writer::default()
        };
        let error = render_incremental(&page, &mut writer).await.unwrap_err();
        assert_original(error, &marker);
        assert_eq!(calls.load(Ordering::SeqCst), expected_calls);
        assert_eq!(writer.flushes, expected_flushes);
        assert_eq!(writer.bytes, prefix);
    }
}

async_template! {
    struct AwaitValue<'a> { model: Work<'a> } => {
        "prefix" {model.value().await?} "suffix"
    }
}

#[tokio::test]
async fn value_expression_errors_propagate_without_finalization() {
    let calls = AtomicUsize::new(0);
    let marker = Arc::new(());
    let mut writer = Writer::default();
    let page = AwaitValue::new(Work {
        calls: &calls,
        marker: &marker,
        fail: true,
    });
    assert_original(
        render_incremental(&page, &mut writer).await.unwrap_err(),
        &marker,
    );
    assert_eq!(writer.bytes, b"prefix");
    assert_eq!(writer.flushes, 0);
}

async_template! {
    struct Nested<S: AsyncSlice> { model: S } => {
        "<main>"
        @section { model }, { "header" };
        @render { model };
        @section { model }, { "unknown" };
        "</main>"
    }
}

#[tokio::test]
async fn nested_declared_templates_and_sections_do_not_add_flushes_or_reencode() {
    let body = Owned::new(String::from("<body>"));
    let header = Owned::new(String::from("<header>"));
    let page = Nested::new(with_async_section(body, "header", header).unwrap());
    for _ in 0..2 {
        let mut writer = Writer::default();
        render_incremental(&page, &mut writer).await.unwrap();
        assert_eq!(
            writer.bytes,
            b"<main><p>&lt;header&gt;</p><p>&lt;body&gt;</p></main>"
        );
        assert_eq!(writer.flushes, 1);
    }
}

#[tokio::test]
async fn section_name_and_child_failures_stop_parent_suffix() {
    async_template! {
        struct BadName<S: AsyncSlice> { model: S } => {
            "prefix" @section { model }, { "" }; "suffix"
        }
    }
    let mut writer = Writer::default();
    let error = render_incremental(&BadName::new(Owned::new(String::new())), &mut writer)
        .await
        .unwrap_err();
    let AsyncRenderError::Write(error) = error else {
        panic!("expected invalid section name");
    };
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(writer.bytes, b"prefix");
    assert_eq!(writer.flushes, 0);

    let marker = Arc::new(());
    let mut writer = Writer {
        fail_write: Some(2),
        marker: marker.clone(),
        ..Writer::default()
    };
    let page = Nested::new(
        with_async_section(
            Owned::new(String::from("body")),
            "header",
            Owned::new(String::from("header")),
        )
        .unwrap(),
    );
    assert_original(
        render_incremental(&page, &mut writer).await.unwrap_err(),
        &marker,
    );
    assert_eq!(writer.bytes, b"<main><p>");
    assert_eq!(writer.flushes, 0);
}

#[test]
fn unused_associated_sections_are_not_executed() {
    let entered = AtomicUsize::new(0);
    let dropped = AtomicUsize::new(0);
    let later = AtomicUsize::new(0);
    let release = Notify::new();
    let section = PendingPage::new(Waiting {
        entered: &entered,
        dropped: &dropped,
        later: &later,
        release: &release,
    });
    let body = with_async_section(Owned::new(String::from("body")), "header", section).unwrap();
    let mut writer = Writer::default();
    let mut future = Box::pin(render_incremental(&body, &mut writer));
    assert!(matches!(poll_once(future.as_mut()), Poll::Ready(Ok(()))));
    drop(future);
    assert_eq!(writer.bytes, b"<p>body</p>");
    assert_eq!(entered.load(Ordering::SeqCst), 0);
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
}

#[test]
fn dropping_pending_declared_section_cancels_it_and_skips_parent_body() {
    let entered = AtomicUsize::new(0);
    let dropped = AtomicUsize::new(0);
    let later = AtomicUsize::new(0);
    let release = Notify::new();
    let section = PendingPage::new(Waiting {
        entered: &entered,
        dropped: &dropped,
        later: &later,
        release: &release,
    });
    let page = Nested::new(
        with_async_section(Owned::new(String::from("unreached")), "header", section).unwrap(),
    );
    let mut writer = Writer::default();
    let mut future = Box::pin(render_incremental(&page, &mut writer));
    assert!(poll_once(future.as_mut()).is_pending());
    assert_eq!(entered.load(Ordering::SeqCst), 1);
    drop(future);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert_eq!(later.load(Ordering::SeqCst), 0);
    assert_eq!(writer.bytes, b"<main>prefix");
    assert_eq!(writer.flushes, 1);
}

#[tokio::test]
async fn long_values_preserve_utf8_and_bounded_encoding() {
    let value = format!("{}{}", "x".repeat(1_023), "🦀<&>\"'".repeat(2_000));
    let page = Owned::new(value.clone());
    let mut writer = Writer::default();
    render_incremental(&page, &mut writer).await.unwrap();
    let mut expected = String::from("<p>");
    razor_slices::encode_html_to(&value, &mut expected).unwrap();
    expected.push_str("</p>");
    assert_eq!(writer.bytes, expected.as_bytes());
    assert!(writer.lengths.len() > 10);
    assert!(writer.lengths.iter().all(|&len| len <= 6 * 1_024));
}

struct Waiting<'a> {
    entered: &'a AtomicUsize,
    dropped: &'a AtomicUsize,
    later: &'a AtomicUsize,
    release: &'a Notify,
}

struct OnDrop<'a>(&'a AtomicUsize);

impl Drop for OnDrop<'_> {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

async fn wait(model: &Waiting<'_>) {
    let _guard = OnDrop(model.dropped);
    model.entered.fetch_add(1, Ordering::SeqCst);
    model.release.notified().await;
    model.later.fetch_add(1, Ordering::SeqCst);
}

async_template! {
    struct PendingPage<'a> { model: Waiting<'a> } => {
        "prefix" @flush; @await { wait(model) }; "suffix"
    }
}

struct Noop;

impl Wake for Noop {
    fn wake(self: Arc<Self>) {}
}

fn poll_once<F: Future>(future: std::pin::Pin<&mut F>) -> Poll<F::Output> {
    future.poll(&mut Context::from_waker(&Waker::from(Arc::new(Noop))))
}

#[test]
fn dropping_pending_application_await_drops_its_future_and_skips_suffix() {
    let entered = AtomicUsize::new(0);
    let dropped = AtomicUsize::new(0);
    let later = AtomicUsize::new(0);
    let release = Notify::new();
    let page = PendingPage::new(Waiting {
        entered: &entered,
        dropped: &dropped,
        later: &later,
        release: &release,
    });
    let mut writer = Writer::default();
    let mut future = Box::pin(render_incremental(&page, &mut writer));
    assert!(poll_once(future.as_mut()).is_pending());
    assert_eq!(entered.load(Ordering::SeqCst), 1);
    drop(future);
    release.notify_one();
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert_eq!(later.load(Ordering::SeqCst), 0);
    assert_eq!(writer.bytes, b"prefix");
    assert_eq!(writer.flushes, 1);
}

struct GatedWriter {
    writer: Writer,
    allow: Arc<AtomicBool>,
    at_flush: bool,
}

impl AsyncSliceWriter for GatedWriter {
    async fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        if !self.at_flush {
            std::future::poll_fn(|_| {
                if self.allow.load(Ordering::SeqCst) {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            })
            .await;
        }
        self.writer.write_all(bytes).await
    }

    async fn flush(&mut self) -> io::Result<()> {
        if self.at_flush {
            std::future::poll_fn(|_| {
                if self.allow.load(Ordering::SeqCst) {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            })
            .await;
        }
        self.writer.flush().await
    }
}

#[test]
fn pending_writes_and_flushes_block_later_work_and_can_resume_or_be_dropped() {
    for at_flush in [false, true] {
        for resume in [false, true] {
            let calls = AtomicUsize::new(0);
            let marker = Arc::new(());
            let page = Steps::new(Work {
                calls: &calls,
                marker: &marker,
                fail: false,
            });
            let allow = Arc::new(AtomicBool::new(false));
            let mut writer = GatedWriter {
                writer: Writer::default(),
                allow: allow.clone(),
                at_flush,
            };
            let mut future = Box::pin(render_incremental(&page, &mut writer));
            assert!(poll_once(future.as_mut()).is_pending());
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            if resume {
                allow.store(true, Ordering::SeqCst);
                assert!(matches!(poll_once(future.as_mut()), Poll::Ready(Ok(()))));
            }
            drop(future);
            if resume {
                assert_eq!(calls.load(Ordering::SeqCst), 2);
                assert_eq!(writer.writer.bytes, b"prefix&lt;value&gt;suffix");
                assert_eq!(writer.writer.flushes, 2);
            } else {
                assert_eq!(calls.load(Ordering::SeqCst), 0);
                assert_eq!(
                    writer.writer.bytes,
                    if at_flush { b"prefix".as_slice() } else { b"" }
                );
                assert_eq!(writer.writer.flushes, 0);
            }
        }
    }
}

#[cfg(feature = "tokio")]
#[test]
fn real_executable_prefix_handshake_matches_exact_stdout() {
    let output = std::process::Command::new(env!("CARGO"))
        .args([
            "run",
            "--quiet",
            "-p",
            "razor-slices",
            "--example",
            "async_template",
            "--features",
            "tokio",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run async declaration example");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, include_bytes!("fixtures/async_template.txt"));
}
