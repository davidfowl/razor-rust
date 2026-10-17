#[path = "../examples/support/layout.rs"]
mod page;
#[path = "../examples/support/todos.rs"]
mod todos;

use razor_slices::{html, partial, with_layout, HtmlString, InlineSlice, Layout, Slice};
use std::{cell::RefCell, fmt, io};

const EXPECTED: &str = include_str!("fixtures/layout.html");

struct Shell;

impl Layout for Shell {
    fn render_layout(&self, body: &dyn Slice, writer: &mut dyn io::Write) -> io::Result<()> {
        html! { "<shell>" {partial(body)} "</shell>" }.render_to_writer(writer)
    }
}

#[test]
fn complete_typed_sample_is_reusable_and_matches_fixture() {
    let todos = todos::sample_models();
    let model = page::LayoutModel {
        title: "Todos <today> & \"tomorrow\" '世界'".into(),
    };
    let document = page::todos_document(&model, &todos);
    for _ in 0..2 {
        assert_eq!(document.render_to_string().unwrap(), EXPECTED);
        assert_eq!(document.render_to_bytes().unwrap(), EXPECTED.as_bytes());
        let mut bytes = Vec::new();
        document.render_to_writer(&mut bytes).unwrap();
        assert_eq!(bytes, EXPECTED.as_bytes());
    }
    assert_eq!(model.title, "Todos <today> & \"tomorrow\" '世界'");
}

#[test]
fn empty_body_owned_bodies_borrowed_layouts_and_nested_documents() {
    assert_eq!(
        with_layout(html! {}, Shell).render_to_string().unwrap(),
        "<shell></shell>"
    );
    let body = {
        let model = String::from("<&>\"'🦀");
        html! { "<p>" {model} {HtmlString::from("<br>")} "</p>" }
    };
    let layout: Box<dyn Layout> = Box::new(Shell);
    let borrowed: &dyn Layout = &layout;
    let inner = with_layout(&body, borrowed);
    let outer = with_layout(inner, &layout);
    assert_eq!(
        outer.render_to_string().unwrap(),
        "<shell><shell><p>&lt;&amp;&gt;&quot;&#39;🦀<br></p></shell></shell>"
    );
}

#[test]
fn changed_layout_model_and_empty_model_list_are_rendered() {
    let model = page::LayoutModel {
        title: "<empty>".into(),
    };
    let result = page::todos_document(&model, &[])
        .render_to_string()
        .unwrap();
    assert!(result.contains("<title>&lt;empty&gt;</title>"));
    assert!(result.contains("<h1>Todos (0)</h1>"));
    assert!(!result.contains("&amp;lt;"));
}

struct OrderedLayout<'a>(&'a RefCell<Vec<&'static str>>);

impl Layout for OrderedLayout<'_> {
    fn render_layout(&self, body: &dyn Slice, writer: &mut dyn io::Write) -> io::Result<()> {
        self.0.borrow_mut().push("head");
        writer.write_all(b"head")?;
        body.render_to_writer(writer)?;
        self.0.borrow_mut().push("tail");
        writer.write_all(b"tail")
    }
}

#[test]
fn layout_starts_before_body_and_construction_does_not_render() {
    let events = RefCell::new(Vec::new());
    let recorder = &events;
    let body = InlineSlice::new(move |writer: &mut dyn io::Write| {
        recorder.borrow_mut().push("body");
        writer.write_all(b"body")
    });
    let document = with_layout(body, OrderedLayout(&events));
    assert!(events.borrow().is_empty());
    for _ in 0..2 {
        assert_eq!(document.render_to_string().unwrap(), "headbodytail");
    }
    assert_eq!(
        *events.borrow(),
        ["head", "body", "tail", "head", "body", "tail"]
    );
}

#[derive(Debug)]
struct Failure(&'static str);

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for Failure {}

fn failure(stage: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, Failure(stage))
}

struct FailingLayout {
    before_body: bool,
}

impl Layout for FailingLayout {
    fn render_layout(&self, body: &dyn Slice, writer: &mut dyn io::Write) -> io::Result<()> {
        writer.write_all(b"head")?;
        if self.before_body {
            return Err(failure("layout head"));
        }
        body.render_to_writer(writer)?;
        Err(failure("layout tail"))
    }
}

#[test]
fn layout_errors_keep_original_source_and_control_body_execution() {
    for before_body in [true, false] {
        let events = RefCell::new(Vec::new());
        let recorder = &events;
        let body = InlineSlice::new(move |writer: &mut dyn io::Write| {
            recorder.borrow_mut().push("body");
            writer.write_all(b"body")
        });
        let document = with_layout(body, FailingLayout { before_body });
        let mut bytes = Vec::new();
        let error = document.render_to_writer(&mut bytes).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(
            error
                .get_ref()
                .unwrap()
                .downcast_ref::<Failure>()
                .unwrap()
                .0,
            if before_body {
                "layout head"
            } else {
                "layout tail"
            }
        );
        if before_body {
            assert_eq!(bytes, b"head");
            assert!(events.borrow().is_empty());
        } else {
            assert_eq!(bytes, b"headbody");
            assert_eq!(*events.borrow(), ["body"]);
        }
        assert!(document.render_to_string().is_err());
    }
}

#[test]
fn nested_body_failure_retains_source_and_omits_layout_tails() {
    let events = RefCell::new(Vec::new());
    let body: Box<dyn Slice> = Box::new(InlineSlice::new(|writer: &mut dyn io::Write| {
        writer.write_all(b"before")?;
        Err(failure("body"))
    }));
    let inner = with_layout(&body, OrderedLayout(&events));
    let outer = with_layout(inner, Shell);
    let mut bytes = Vec::new();
    let error = outer.render_to_writer(&mut bytes).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(
        error
            .get_ref()
            .unwrap()
            .downcast_ref::<Failure>()
            .unwrap()
            .0,
        "body"
    );
    assert_eq!(bytes, b"<shell>headbefore");
    assert_eq!(*events.borrow(), ["head"]);
    assert!(outer.render_to_string().is_err());
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
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "sink failed"));
        }
        let count = bytes.len().min(self.remaining).min(1);
        self.bytes.extend_from_slice(&bytes[..count]);
        self.remaining -= count;
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        panic!("layouts must not flush the destination")
    }
}

#[test]
fn sink_failures_before_in_and_after_body_stop_in_render_order() {
    for remaining in [1, 5, 9, 100] {
        let events = RefCell::new(Vec::new());
        let recorder = &events;
        let body = InlineSlice::new(move |writer: &mut dyn io::Write| {
            recorder.borrow_mut().push("body");
            writer.write_all(b"body")
        });
        let document = with_layout(body, OrderedLayout(&events));
        let mut writer = ShortWriter {
            bytes: Vec::new(),
            remaining,
            interrupted: false,
        };
        let result = document.render_to_writer(&mut writer);
        if remaining == 100 {
            result.unwrap();
            assert_eq!(writer.bytes, b"headbodytail");
        } else {
            let error = result.unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
            assert_eq!(error.to_string(), "sink failed");
            assert_eq!(writer.bytes, b"headbodytail"[..remaining]);
        }
        assert_eq!(
            *events.borrow(),
            match remaining {
                1 => vec!["head"],
                5 => vec!["head", "body"],
                _ => vec!["head", "body", "tail"],
            }
        );
    }
}

#[test]
fn text_destination_failure_does_not_evaluate_body() {
    struct BrokenText;
    impl fmt::Write for BrokenText {
        fn write_str(&mut self, _: &str) -> fmt::Result {
            Err(fmt::Error)
        }
    }
    let events = RefCell::new(Vec::new());
    let recorder = &events;
    let body = InlineSlice::new(move |writer: &mut dyn io::Write| {
        recorder.borrow_mut().push("body");
        writer.write_all(b"body")
    });
    let document = with_layout(body, Shell);
    assert!(document.render(&mut BrokenText).is_err());
    assert!(events.borrow().is_empty());
}

#[test]
fn byte_override_on_dynamic_body_is_used_without_convenience_buffering() {
    struct ByteBody;
    impl Slice for ByteBody {
        fn render(&self, _: &mut dyn fmt::Write) -> fmt::Result {
            panic!("byte body override must be used")
        }
        fn render_to_writer(&self, writer: &mut dyn io::Write) -> io::Result<()> {
            writer.write_all("<p>世界</p>".as_bytes())
        }
        fn render_to_string(&self) -> Result<String, razor_slices::RenderError> {
            panic!("layout must not pre-render body")
        }
    }
    let body: Box<dyn Slice> = Box::new(ByteBody);
    let document = with_layout(&body, Shell);
    assert_eq!(
        document.render_to_string().unwrap(),
        "<shell><p>世界</p></shell>"
    );
}

#[tokio::test]
async fn full_layout_document_works_with_existing_buffered_async_bridge() {
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
    let todos = todos::sample_models();
    let model = page::LayoutModel {
        title: "Todos <today> & \"tomorrow\" '世界'".into(),
    };
    let document = page::todos_document(&model, &todos);
    let mut writer = Writer(Vec::new());
    razor_slices::render_async(&document, &mut writer)
        .await
        .unwrap();
    assert_eq!(writer.0, EXPECTED.as_bytes());
}
