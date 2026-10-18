use razor_slices::{html, render_async, HtmlString, Slice, WriteHtmlValue};
use std::{
    fmt, io,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

const FAILED_DOCUMENT: &[u8] = b"P&lt;custom&amp;&gt;S";

struct Text<'a>(&'a str);

impl fmt::Display for Text<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

struct Long;

impl fmt::Display for Long {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for _ in 0..8192 {
            f.write_str("x")?;
        }
        f.write_str("<&>")
    }
}

#[derive(Debug)]
struct Original(Arc<()>);

impl fmt::Display for Original {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("original formatted value destination failure")
    }
}
impl std::error::Error for Original {}

#[derive(Default)]
struct Writer {
    bytes: Vec<u8>,
    fail_at: Option<usize>,
    failure: Arc<()>,
    failed: bool,
    after_failure: usize,
    flushes: usize,
    accepted: Arc<AtomicUsize>,
}

impl io::Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.failed {
            self.after_failure += 1;
        }
        let accepted = self.fail_at.map_or(bytes.len(), |limit| {
            bytes.len().min(limit.saturating_sub(self.bytes.len()))
        });
        if accepted == 0 && !bytes.is_empty() {
            self.failed = true;
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                Original(self.failure.clone()),
            ));
        }
        self.bytes.extend_from_slice(&bytes[..accepted]);
        self.accepted.fetch_add(accepted, Ordering::SeqCst);
        Ok(accepted)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flushes += 1;
        Ok(())
    }
}

fn assert_original(error: io::Error, writer: &Writer) {
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    let original = error.get_ref().unwrap().downcast_ref::<Original>().unwrap();
    assert!(Arc::ptr_eq(&original.0, &writer.failure));
    assert_eq!(writer.after_failure, 0);
}

fn write_case(kind: &str, writer: &mut dyn io::Write) -> io::Result<()> {
    match kind {
        "decimal" | "reuse" => html! { {format_args!("{:04}", 42)} }.render_to_writer(writer),
        "negative" => html! { {format_args!("{:05}", -42)} }.render_to_writer(writer),
        "hex" => html! { {format_args!("{:04X}", 255)} }.render_to_writer(writer),
        "fixed" => html! { {format_args!("{:.2}", 12.5)} }.render_to_writer(writer),
        "custom" => html! { {format_args!("{}", Text("<custom&>"))} }.render_to_writer(writer),
        "empty" => html! { {format_args!("{}", Text(""))} }.render_to_writer(writer),
        "markup" => {
            html! { {format_args!("{}", HtmlString::from("<b>"))} }.render_to_writer(writer)
        }
        "long" => html! { {format_args!("{Long}")} }.render_to_writer(writer),
        "failure" => {
            html! { "P" {format_args!("{}", Text("<custom&>"))} "S" }.render_to_writer(writer)
        }
        _ => panic!("unknown case: {kind}"),
    }
}

#[test]
fn explicit_formats_match_pinned_reference_and_preserve_every_error_prefix() {
    let mut rows = Vec::new();
    for kind in [
        "decimal", "negative", "hex", "fixed", "custom", "empty", "markup", "long", "reuse",
    ] {
        let mut writer = Writer::default();
        for _ in 0..if kind == "reuse" { 2 } else { 1 } {
            write_case(kind, &mut writer).unwrap();
        }
        let detail = if kind == "long" {
            let mut expected = vec![b'x'; 8192];
            expected.extend_from_slice(b"&lt;&amp;&gt;");
            assert_eq!(writer.bytes, expected);
            let hash = writer
                .bytes
                .iter()
                .fold(14695981039346656037_u64, |hash, byte| {
                    (hash ^ u64::from(*byte)).wrapping_mul(1099511628211)
                });
            format!("bytes={};fnv1a={hash:016x}", writer.bytes.len())
        } else {
            format!("output={}", std::str::from_utf8(&writer.bytes).unwrap())
        };
        rows.push(format!("common/{kind}: status=ok;{detail}"));
        assert_eq!(writer.flushes, 0);
    }
    for fail_at in 0..FAILED_DOCUMENT.len() {
        let mut writer = Writer {
            fail_at: Some(fail_at),
            ..Writer::default()
        };
        let error = write_case("failure", &mut writer).unwrap_err();
        assert_original(error, &writer);
        assert_eq!(writer.bytes, FAILED_DOCUMENT[..fail_at]);
        assert_eq!(writer.flushes, 0);
        rows.push(format!(
            "common/error-{fail_at}: status=same-error;output={}",
            std::str::from_utf8(&writer.bytes).unwrap()
        ));
    }
    let expected: Vec<_> = include_str!("fixtures/formatted-values.txt")
        .lines()
        .filter(|line| line.starts_with("common/"))
        .collect();
    assert_eq!(expected.len(), 30);
    assert_eq!(rows, expected);
}

#[test]
fn formatting_repeats_borrows_and_preserves_the_explicit_trust_boundary() {
    struct Counted<'a>(&'a AtomicUsize);
    impl fmt::Display for Counted<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            self.0.fetch_add(1, Ordering::SeqCst);
            f.write_str("<&\"'>")
        }
    }
    let calls = AtomicUsize::new(0);
    let value = Counted(&calls);
    let page = html! {
        {Some(&format_args!("{value}"))}
        {None::<fmt::Arguments<'_>>}
        {HtmlString::from("<b>")}
        {format_args!("{}", HtmlString::from("<b>"))}
    };
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    for round in 1..=2 {
        assert_eq!(
            page.render_to_bytes().unwrap(),
            b"&lt;&amp;&quot;&#39;&gt;<b>&lt;b&gt;"
        );
        assert_eq!(calls.load(Ordering::SeqCst), round);
    }
    let mut text = String::new();
    page.render(&mut text).unwrap();
    assert_eq!(text, "&lt;&amp;&quot;&#39;&gt;<b>&lt;b&gt;");
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[test]
fn formatter_chunks_reach_the_destination_before_formatting_continues() {
    struct Streaming(Arc<AtomicUsize>);
    impl fmt::Display for Streaming {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("prefix")?;
            assert_eq!(self.0.load(Ordering::SeqCst), 6);
            for _ in 0..8192 {
                f.write_str("x")?;
            }
            f.write_str("雪<&>")?;
            assert_eq!(self.0.load(Ordering::SeqCst), 6 + 8192 + 3 + 13);
            Ok(())
        }
    }
    let mut writer = Writer::default();
    let value = Streaming(writer.accepted.clone());
    format_args!("{value}").write_html_to(&mut writer).unwrap();
    assert_eq!(writer.bytes.len(), 8214);
    assert!(writer.bytes.ends_with("雪&lt;&amp;&gt;".as_bytes()));
    assert_eq!(writer.flushes, 0);
}

#[test]
fn formatter_errors_are_explicit_and_a_swallowed_sink_error_stays_original() {
    struct Bad;
    impl fmt::Display for Bad {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("prefix")?;
            Err(fmt::Error)
        }
    }
    let mut writer = Writer::default();
    let error = html! { "P" {format_args!("{Bad}")} "S" }
        .render_to_writer(&mut writer)
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Other);
    assert!(error.get_ref().unwrap().is::<fmt::Error>());
    assert_eq!(writer.bytes, b"Pprefix");

    struct IgnoresError;
    impl fmt::Display for IgnoresError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            let _ = f.write_str("a<&>");
            let _ = f.write_str("must-not-reach-the-sink");
            Ok(())
        }
    }
    let mut writer = Writer {
        fail_at: Some(2),
        ..Writer::default()
    };
    let error = format_args!("{IgnoresError}")
        .write_html_to(&mut writer)
        .unwrap_err();
    assert_original(error, &writer);
    assert_eq!(writer.bytes, b"a&");
    assert_eq!(writer.flushes, 0);
}

#[test]
fn explicit_rust_flags_and_utf8_are_encoded_without_changing_primitive_defaults() {
    let page = html! {
        {format_args!("{:*>6.3}", "雪<&>")}
        "|" {format_args!("{:08b}", 5)}
        "|" {format_args!("{:?}", "<&>")}
        "|" {true} "|" {12.5} "|" {"&lt;"}
        "|" {format_args!("{:<^5}", "x")}
        "|" {format_args!("<b>")}
    };
    assert_eq!(
        page.render_to_string().unwrap(),
        "***雪&lt;&amp;|00000101|&quot;&lt;&amp;&gt;&quot;|true|12.5|&amp;lt;|&lt;&lt;x&lt;&lt;|&lt;b&gt;"
    );
}

#[tokio::test]
async fn existing_buffered_bridge_accepts_formatted_sync_slices() {
    struct AsyncWriter(Vec<u8>, usize);
    impl razor_slices::AsyncSliceWriter for AsyncWriter {
        async fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
            self.0.extend_from_slice(bytes);
            Ok(())
        }
        async fn flush(&mut self) -> io::Result<()> {
            self.1 += 1;
            Ok(())
        }
    }
    let value = 42;
    let page = html! { {format_args!("{value:04}")} };
    let mut writer = AsyncWriter(Vec::new(), 0);
    render_async(&page, &mut writer).await.unwrap();
    assert_eq!(writer.0, b"0042");
    assert_eq!(writer.1, 1);
}
