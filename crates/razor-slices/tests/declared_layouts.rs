#[path = "../examples/support/declared_layout.rs"]
mod page;

use razor_slices::{
    declare_layout, html, partial, section, with_layout, with_section, InlineSlice, Layout,
    RenderError, Slice, WriteHtmlValue,
};
use std::{cell::RefCell, fmt, io};

struct Shell(&'static str);

impl Layout for Shell {
    fn render_layout(&self, body: &dyn Slice, writer: &mut dyn io::Write) -> io::Result<()> {
        html! { "<" {self.0} ">" }.render_to_writer(writer)?;
        // Deliberately use direct rendering, not the partial helper.
        body.render_to_writer(writer)?;
        html! { "</" {self.0} ">" }.render_to_writer(writer)
    }
}

#[test]
fn full_and_bare_sample_repeats_through_every_render_surface() {
    let model = page::sample_model();
    let page = page::greeting(&model).unwrap();
    let expected = include_str!("fixtures/declared-layout.html");
    let (full, bare) = expected.split_once("\nBare partial:\n").unwrap();
    let borrowed = &page;
    for _ in 0..2 {
        assert_eq!(page.render_to_string().unwrap(), full);
        assert_eq!(page.render_to_bytes().unwrap(), full.as_bytes());
        let mut text = String::new();
        page.render(&mut text).unwrap();
        assert_eq!(text, full);
        let mut bytes = Vec::new();
        page.render_to_writer(&mut bytes).unwrap();
        assert_eq!(bytes, full.as_bytes());
        let mut bytes = Vec::new();
        page.render_body_to_writer(&mut bytes).unwrap();
        assert_eq!(bytes, bare.trim_end().as_bytes());
        let output = html! { {partial(borrowed)} "\n" };
        assert_eq!(output.render_to_string().unwrap(), bare);
    }
}

#[test]
fn nested_declared_layouts_recurse_but_nested_body_declarations_are_bypassed() {
    let layout = declare_layout(Shell("inner"), Shell("outer"));
    let page = declare_layout(html! { "body" }, layout);
    assert_eq!(
        page.render_to_string().unwrap(),
        "<outer><inner>body</inner></outer>"
    );
    let borrowed = &page;
    assert_eq!(
        html! { {partial(borrowed)} }.render_to_string().unwrap(),
        "body"
    );
    let wrapped = declare_layout(&page, Shell("replacement"));
    assert_eq!(
        wrapped.render_to_string().unwrap(),
        "<replacement>body</replacement>"
    );
}

#[test]
fn explicit_composition_is_retained_inside_partials_and_declared_bodies() {
    let declared = declare_layout(html! { "body" }, Shell("declared"));
    let explicit = with_layout(&declared, Shell("explicit"));
    let borrowed = &explicit;
    assert_eq!(
        html! { {partial(borrowed)} }.render_to_string().unwrap(),
        "<explicit>body</explicit>"
    );
    let declared_explicit = declare_layout(explicit, Shell("document"));
    assert_eq!(
        declared_explicit.render_to_string().unwrap(),
        "<document><explicit>body</explicit></document>"
    );
    assert_eq!(
        html! { {partial(&declared_explicit)} }
            .render_to_string()
            .unwrap(),
        "<explicit>body</explicit>"
    );
}

#[test]
fn owned_borrowed_and_boxed_models_layouts_and_slice_objects() {
    let name = String::from("<&>\"'🦀");
    let layout: Box<dyn Layout> = Box::new(Shell("page"));
    let borrowed: &dyn Layout = &layout;
    let text = &name;
    let body: Box<dyn Slice + '_> = Box::new(html! { {text} });
    let page: Box<dyn Slice + '_> = Box::new(declare_layout(&body, borrowed));
    let reference: &dyn Slice = &page;
    assert_eq!(
        reference.render_to_string().unwrap(),
        "<page>&lt;&amp;&gt;&quot;&#39;🦀</page>"
    );
    assert_eq!(
        html! { {partial(reference)} }.render_to_string().unwrap(),
        "&lt;&amp;&gt;&quot;&#39;🦀"
    );
    let model = String::from("owned");
    let owned = declare_layout(html! { {model} }, Box::new(Shell("page")));
    assert_eq!(owned.render_to_string().unwrap(), "<page>owned</page>");
}

struct Ordered<'a>(&'a RefCell<Vec<&'static str>>);

impl Layout for Ordered<'_> {
    fn render_layout(&self, body: &dyn Slice, writer: &mut dyn io::Write) -> io::Result<()> {
        self.0.borrow_mut().push("head");
        writer.write_all(b"head")?;
        body.render_to_writer(writer)?;
        self.0.borrow_mut().push("tail");
        writer.write_all(b"tail")
    }
}

#[test]
fn lazy_construction_repeated_rendering_and_partial_order() {
    let events = RefCell::new(Vec::new());
    let recorder = &events;
    let body = InlineSlice::new(move |w: &mut dyn io::Write| {
        recorder.borrow_mut().push("body");
        w.write_all(b"body")
    });
    let page = declare_layout(body, Ordered(&events));
    let borrowed = &page;
    assert!(events.borrow().is_empty());
    for _ in 0..2 {
        assert_eq!(page.render_to_string().unwrap(), "headbodytail");
        assert_eq!(
            html! { {partial(borrowed)} }.render_to_string().unwrap(),
            "body"
        );
    }
    assert_eq!(
        *events.borrow(),
        ["head", "body", "tail", "body", "head", "body", "tail", "body"]
    );
}

#[derive(Debug)]
struct Failure;

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("original failure")
    }
}
impl std::error::Error for Failure {}

fn failure() -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, Failure)
}

fn assert_failure(error: io::Error) {
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(error.get_ref().unwrap().is::<Failure>());
}

#[test]
fn nested_body_failure_preserves_kind_source_and_skips_tails() {
    let body = InlineSlice::new(|w: &mut dyn io::Write| {
        w.write_all(b"prefix")?;
        Err(failure())
    });
    let page = declare_layout(body, declare_layout(Shell("inner"), Shell("outer")));
    let mut bytes = Vec::new();
    assert_failure(page.render_to_writer(&mut bytes).unwrap_err());
    assert_eq!(bytes, b"<outer><inner>prefix");
    bytes.clear();
    assert_failure(partial(&page).write_html_to(&mut bytes).unwrap_err());
    assert_eq!(bytes, b"prefix");
    assert!(page.render_to_string().is_err());
}

#[test]
fn early_layout_and_text_sink_errors_skip_body_but_partial_skips_layout_failure() {
    struct Fails;
    impl Layout for Fails {
        fn render_layout(&self, _: &dyn Slice, _: &mut dyn io::Write) -> io::Result<()> {
            Err(failure())
        }
    }
    struct BrokenText;
    impl fmt::Write for BrokenText {
        fn write_str(&mut self, _: &str) -> fmt::Result {
            Err(fmt::Error)
        }
    }
    let events = RefCell::new(Vec::new());
    let recorder = &events;
    let body = InlineSlice::new(move |w: &mut dyn io::Write| {
        recorder.borrow_mut().push("body");
        w.write_all(b"body")
    });
    let page = declare_layout(body, Fails);
    assert_failure(page.render_to_writer(&mut Vec::new()).unwrap_err());
    assert!(events.borrow().is_empty());
    let nested = declare_layout(&page, Shell("outer"));
    assert!(nested.render(&mut BrokenText).is_err());
    assert!(events.borrow().is_empty());
    assert_eq!(
        html! { {partial(&page)} }.render_to_string().unwrap(),
        "body"
    );
}

#[test]
fn short_interrupted_and_failing_sinks_stop_in_order_without_flush() {
    struct Writer {
        bytes: Vec<u8>,
        limit: usize,
        interrupted: bool,
    }
    impl io::Write for Writer {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if !self.interrupted {
                self.interrupted = true;
                return Err(io::ErrorKind::Interrupted.into());
            }
            if self.bytes.len() == self.limit {
                return Err(failure());
            }
            self.bytes.push(bytes[0]);
            Ok(1)
        }
        fn flush(&mut self) -> io::Result<()> {
            panic!("no implicit flush")
        }
    }
    for limit in [1, 5, 9, 12] {
        let events = RefCell::new(Vec::new());
        let recorder = &events;
        let body = InlineSlice::new(move |w: &mut dyn io::Write| {
            recorder.borrow_mut().push("body");
            w.write_all(b"body")
        });
        let page = declare_layout(body, Ordered(&events));
        let mut writer = Writer {
            bytes: Vec::new(),
            limit,
            interrupted: false,
        };
        let result = page.render_to_writer(&mut writer);
        if limit == 12 {
            result.unwrap();
        } else {
            assert_failure(result.unwrap_err());
        }
        assert_eq!(writer.bytes, b"headbodytail"[..limit]);
        assert_eq!(
            *events.borrow(),
            match limit {
                1 => vec!["head"],
                5 => vec!["head", "body"],
                _ => vec!["head", "body", "tail"],
            }
        );
    }
}

#[test]
fn section_associations_on_either_side_preserve_bypass_and_lazy_sections() {
    let events = RefCell::new(Vec::new());
    let recorder = &events;
    let footer = declare_layout(
        InlineSlice::new(move |w: &mut dyn io::Write| {
            recorder.borrow_mut().push("footer");
            w.write_all(b"footer")
        }),
        Shell("must-not-render"),
    );
    let page = declare_layout(
        with_section(html! { "body" }, "footer", footer).unwrap(),
        Shell("page"),
    );
    let page = with_section(page, "header", html! { "header" }).unwrap();
    assert!(events.borrow().is_empty());
    assert_eq!(page.render_to_string().unwrap(), "<page>body</page>");
    assert!(events.borrow().is_empty());
    let borrowed = &page;
    assert_eq!(
        html! { {partial(borrowed)} {section(borrowed, "footer")} {section(borrowed, "header")}
        {section(borrowed, "missing")} {section(borrowed, "footer")} }
        .render_to_string()
        .unwrap(),
        "bodyfooterheaderfooter"
    );
    assert_eq!(*events.borrow(), ["footer", "footer"]);
    assert_eq!(
        page.render_section_to_writer("", &mut Vec::new())
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
}

#[test]
fn layout_receives_bare_body_with_sections_for_direct_text_and_byte_rendering() {
    struct Inspect;
    impl Layout for Inspect {
        fn render_layout(&self, body: &dyn Slice, w: &mut dyn io::Write) -> io::Result<()> {
            assert_eq!(body.render_to_string().unwrap(), "body");
            assert_eq!(body.render_to_bytes().unwrap(), b"body");
            html! { {section(body, "footer")} {partial(body)} }.render_to_writer(w)
        }
    }
    let body = declare_layout(
        with_section(html! { "body" }, "footer", html! { "footer" }).unwrap(),
        Shell("ignored"),
    );
    let page = declare_layout(body, Inspect);
    assert_eq!(page.render_to_string().unwrap(), "footerbody");
}

#[test]
fn all_wrappers_forward_custom_body_section_and_convenience_overrides() {
    struct Overrides;
    impl Slice for Overrides {
        fn render(&self, _: &mut dyn fmt::Write) -> fmt::Result {
            panic!("must retain overrides")
        }
        fn render_to_writer(&self, w: &mut dyn io::Write) -> io::Result<()> {
            w.write_all(b"full-byte")
        }
        fn render_body_to_writer(&self, w: &mut dyn io::Write) -> io::Result<()> {
            w.write_all(b"bare-byte")
        }
        fn render_to_string(&self) -> Result<String, RenderError> {
            Ok("string-override".into())
        }
        fn render_to_bytes(&self) -> Result<Vec<u8>, RenderError> {
            Ok(b"bytes-override".to_vec())
        }
        fn render_section_to_writer(&self, name: &str, w: &mut dyn io::Write) -> io::Result<()> {
            assert_eq!(name, "custom");
            w.write_all(b"section-override")
        }
    }
    let boxed: Box<dyn Slice> = Box::new(Overrides);
    let referenced: &dyn Slice = &boxed;
    let sectioned = with_section(referenced, "other", html! {}).unwrap();
    assert_eq!(sectioned.render_to_string().unwrap(), "string-override");
    assert_eq!(sectioned.render_to_bytes().unwrap(), b"bytes-override");
    let mut bytes = Vec::new();
    sectioned.render_to_writer(&mut bytes).unwrap();
    assert_eq!(bytes, b"full-byte");
    let declared: Box<dyn Slice> = Box::new(declare_layout(sectioned, Shell("page")));
    assert_eq!(
        declared.render_to_string().unwrap(),
        "<page>bare-byte</page>"
    );
    assert_eq!(
        html! { {partial(&declared)} {section(&declared, "custom")} }
            .render_to_string()
            .unwrap(),
        "bare-bytesection-override"
    );
}

#[tokio::test]
async fn existing_buffered_async_bridge_renders_full_declared_document() {
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
    let page = declare_layout(html! { "body" }, Shell("page"));
    let mut writer = Writer(Vec::new());
    razor_slices::render_async(&page, &mut writer)
        .await
        .unwrap();
    assert_eq!(writer.0, b"<page>body</page>");
}

#[test]
fn empty_body_and_layout_controlled_repeated_insertion() {
    struct Twice;
    impl Layout for Twice {
        fn render_layout(&self, body: &dyn Slice, w: &mut dyn io::Write) -> io::Result<()> {
            body.render_to_writer(w)?;
            body.render_to_writer(w)
        }
    }
    assert_eq!(
        declare_layout(html! {}, Shell("empty"))
            .render_to_string()
            .unwrap(),
        "<empty></empty>"
    );
    let body = declare_layout(html! { "body" }, Shell("ignored"));
    assert_eq!(
        declare_layout(body, Twice).render_to_string().unwrap(),
        "bodybody"
    );
}

#[test]
fn section_and_layout_tail_errors_keep_sources_and_stop_subsequent_output() {
    struct TailFailure;
    impl Layout for TailFailure {
        fn render_layout(&self, body: &dyn Slice, w: &mut dyn io::Write) -> io::Result<()> {
            body.render_to_writer(w)?;
            Err(failure())
        }
    }
    let page = declare_layout(html! { "body" }, TailFailure);
    let mut bytes = Vec::new();
    assert_failure(page.render_to_writer(&mut bytes).unwrap_err());
    assert_eq!(bytes, b"body");
    bytes.clear();
    let content = declare_layout(
        InlineSlice::new(|w: &mut dyn io::Write| {
            w.write_all(b"section-prefix")?;
            Err(failure())
        }),
        Shell("ignored"),
    );
    let sectioned = with_section(page, "footer", content).unwrap();
    let output = html! { {section(&sectioned, "footer")} "must-not-write" };
    assert_failure(output.render_to_writer(&mut bytes).unwrap_err());
    assert_eq!(bytes, b"section-prefix");
}
