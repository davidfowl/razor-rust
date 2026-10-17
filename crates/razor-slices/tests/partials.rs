#[path = "../examples/support/todos.rs"]
mod templates;

use razor_slices::{html, partial, HtmlString, Slice, WriteHtmlValue};
use std::{cell::Cell, fmt, io};

const EXPECTED: &str = include_str!("fixtures/todos.html");
const FOOTER: &str = "<footer>Rendered with reusable partials.</footer>\n";

#[test]
fn typed_model_list_matches_sample_output_and_is_reusable() {
    let models = templates::sample_models();
    let page = templates::todos_page(&models);
    let output = html! { {partial(&page)} {HtmlString::from(FOOTER)} };
    for _ in 0..2 {
        assert_eq!(output.render_to_string().unwrap(), EXPECTED);
        assert_eq!(output.render_to_bytes().unwrap(), EXPECTED.as_bytes());
        let mut bytes = Vec::new();
        output.render_to_writer(&mut bytes).unwrap();
        assert_eq!(bytes, EXPECTED.as_bytes());
    }
    assert_eq!(models[0].title, "Wash <dishes> & \"cups\"");
}

#[test]
fn empty_list_and_model_changes() {
    assert_eq!(
        templates::todos_page(&[]).render_to_string().unwrap(),
        "<h1>Todos (0)</h1>\n<table>\n<tr><th>Id</th><th>Title</th><th>Due by</th><th>Status</th></tr>\n</table>\n"
    );
    let mut models = templates::sample_models();
    models[0].is_complete = false;
    models[0].due_by = Some("<tomorrow>".into());
    assert_eq!(
        templates::todo_row(&models[0]).render_to_string().unwrap(),
        "<tr>\n  <td>1</td>\n  <td>Wash &lt;dishes&gt; &amp; &quot;cups&quot;</td>\n  <td>&lt;tomorrow&gt;</td>\n  <td>Incomplete</td>\n</tr>\n"
    );
}

#[test]
fn owned_borrowed_nested_and_dynamic_children_escape_once() {
    let child = html! { "<li>" {"<&>\"'🦀"} {HtmlString::from("<br>")} "</li>" };
    let owned = partial(child);
    let parent = html! { "<ul>" {owned} "</ul>" };
    let boxed: Box<dyn Slice> = Box::new(parent);
    let borrowed: &dyn Slice = &boxed;
    let grandparent = html! { "<main>" {partial(borrowed)} "</main>" };
    assert_eq!(
        grandparent.render_to_string().unwrap(),
        "<main><ul><li>&lt;&amp;&gt;&quot;&#39;🦀<br></li></ul></main>"
    );
    let row = templates::todo_row(&templates::sample_models()[0])
        .render_to_string()
        .unwrap();
    assert!(row.contains("&lt;dishes&gt;"));
}

struct TextOnly;

impl Slice for TextOnly {
    fn render(&self, writer: &mut dyn fmt::Write) -> fmt::Result {
        writer.write_str("<p>")?;
        razor_slices::encode_html_to("<&> 世界", writer)?;
        writer.write_str("</p>")
    }

    fn render_to_string(&self) -> Result<String, razor_slices::RenderError> {
        panic!("partial must not use a buffered convenience method")
    }
}

#[test]
fn handwritten_text_child_renders_without_buffering_or_reencoding() {
    let page = html! { "<main>" {partial(TextOnly)} "</main>" };
    assert_eq!(
        page.render_to_string().unwrap(),
        "<main><p>&lt;&amp;&gt; 世界</p></main>"
    );
}

struct ByteOverride;

impl Slice for ByteOverride {
    fn render(&self, _: &mut dyn fmt::Write) -> fmt::Result {
        panic!("byte override must be forwarded by references and boxes")
    }

    fn render_to_writer(&self, writer: &mut dyn io::Write) -> io::Result<()> {
        writer.write_all(b"<b>byte override</b>")
    }
}

#[test]
fn byte_overrides_are_forwarded_through_references_boxes_and_partials() {
    fn render(slice: impl Slice) -> Vec<u8> {
        let mut bytes = Vec::new();
        slice.render_to_writer(&mut bytes).unwrap();
        bytes
    }
    let value = ByteOverride;
    let borrowed: &dyn Slice = &value;
    assert_eq!(render(borrowed), b"<b>byte override</b>");
    let boxed: Box<dyn Slice> = Box::new(ByteOverride);
    assert_eq!(render(&boxed), b"<b>byte override</b>");
    let parent = html! { {partial(&boxed)} };
    assert_eq!(parent.render_to_string().unwrap(), "<b>byte override</b>");
}

#[derive(Debug)]
struct ValueFailure;

impl fmt::Display for ValueFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("child value failed")
    }
}

impl std::error::Error for ValueFailure {}

impl WriteHtmlValue for ValueFailure {
    fn write_html_to(&self, _: &mut dyn io::Write) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            ValueFailure,
        ))
    }
}

#[test]
fn nested_value_failure_retains_original_error_and_stops_parent() {
    let calls = Cell::new(0);
    let counter = &calls;
    let child: Box<dyn Slice> = Box::new(html! { "<child>" {ValueFailure} "</child>" });
    let parent = html! { "<parent>" {partial(&child)} {{ counter.set(1); "after" }} };
    let outer = html! { "<outer>" {partial(&parent)} "</outer>" };
    let mut bytes = Vec::new();
    let error = outer.render_to_writer(&mut bytes).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(error.get_ref().unwrap().is::<ValueFailure>());
    assert_eq!(bytes, b"<outer><parent><child>");
    assert_eq!(calls.get(), 0);
    assert!(outer.render_to_string().is_err());
    assert_eq!(calls.get(), 0);
}

struct BrokenChild;

impl Slice for BrokenChild {
    fn render(&self, writer: &mut dyn fmt::Write) -> fmt::Result {
        writer.write_str("before")?;
        Err(fmt::Error)
    }
}

#[test]
fn handwritten_child_error_is_explicit() {
    let parent = html! { {partial(BrokenChild)} "after" };
    let mut bytes = Vec::new();
    let error = parent.render_to_writer(&mut bytes).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Other);
    assert!(error.get_ref().unwrap().is::<fmt::Error>());
    assert_eq!(bytes, b"before");
    assert!(parent.render_to_string().is_err());
}

struct ShortWriter {
    bytes: Vec<u8>,
    remaining: usize,
    interrupted: bool,
}

impl io::Write for ShortWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if !self.interrupted {
            self.interrupted = true;
            return Err(io::ErrorKind::Interrupted.into());
        }
        if self.remaining == 0 {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "destination failed",
            ));
        }
        let count = bytes.len().min(self.remaining).min(1);
        self.bytes.extend_from_slice(&bytes[..count]);
        self.remaining -= count;
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        panic!("partial rendering must not flush the destination")
    }
}

#[test]
fn destination_errors_and_short_interrupted_writes_survive_text_child_bridge() {
    let parent = html! { "<main>" {partial(TextOnly)} "</main>" };
    let expected = b"<main><p>&lt;&amp;&gt; \xe4\xb8\x96\xe7\x95\x8c</p></main>";
    for remaining in [1, 7, 11, 23] {
        let mut writer = ShortWriter {
            bytes: Vec::new(),
            remaining,
            interrupted: false,
        };
        let error = parent.render_to_writer(&mut writer).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
        assert_eq!(error.to_string(), "destination failed");
        assert_eq!(writer.bytes, expected[..remaining]);
    }
    let mut writer = ShortWriter {
        bytes: Vec::new(),
        remaining: 100,
        interrupted: false,
    };
    parent.render_to_writer(&mut writer).unwrap();
    assert_eq!(writer.bytes, expected);
}

#[test]
fn byte_bridge_retains_first_error_even_if_text_child_ignores_it() {
    struct IgnoresErrors;
    impl Slice for IgnoresErrors {
        fn render(&self, writer: &mut dyn fmt::Write) -> fmt::Result {
            let _ = writer.write_str("first");
            let _ = writer.write_str("second");
            Ok(())
        }
    }
    let mut writer = ShortWriter {
        bytes: Vec::new(),
        remaining: 2,
        interrupted: false,
    };
    let error = IgnoresErrors.render_to_writer(&mut writer).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    assert_eq!(writer.bytes, b"fi");
}

#[test]
fn text_destination_errors_stop_parent_after_partial() {
    struct BrokenText;
    impl fmt::Write for BrokenText {
        fn write_str(&mut self, _: &str) -> fmt::Result {
            Err(fmt::Error)
        }
    }
    let calls = Cell::new(0);
    let counter = &calls;
    let child = html! { "child" };
    let parent = html! { {partial(&child)} {{ counter.set(1); "after" }} };
    assert!(parent.render(&mut BrokenText).is_err());
    assert_eq!(calls.get(), 0);
}

#[test]
fn partials_preserve_split_utf8_and_reject_malformed_text_output() {
    let child = razor_slices::InlineSlice::new(|writer: &mut dyn io::Write| {
        for byte in "é中🦀".bytes() {
            writer.write_all(&[byte])?;
        }
        Ok(())
    });
    let parent = html! { "<p>" {partial(&child)} "</p>" };
    assert_eq!(parent.render_to_string().unwrap(), "<p>é中🦀</p>");
    for bytes in [vec![0xff], vec![0xe4, 0xb8]] {
        let child = razor_slices::InlineSlice::new(move |writer: &mut dyn io::Write| {
            writer.write_all(&bytes)
        });
        let parent = html! { {partial(&child)} };
        assert!(parent.render_to_string().is_err());
    }
}

#[tokio::test]
async fn composed_model_sample_works_with_buffered_async_bridge() {
    struct Writer(Vec<u8>);
    impl razor_slices::AsyncSliceWriter for Writer {
        async fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
            self.0.extend_from_slice(bytes);
            Ok(())
        }
        async fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let models = templates::sample_models();
    let page = templates::todos_page(&models);
    let output = html! { {partial(&page)} {HtmlString::from(FOOTER)} };
    let mut writer = Writer(Vec::new());
    razor_slices::render_async(&output, &mut writer)
        .await
        .unwrap();
    assert_eq!(writer.0, EXPECTED.as_bytes());
}
