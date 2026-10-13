#[path = "../examples/support/sections.rs"]
mod page;
#[path = "../examples/support/todos.rs"]
mod todos;

use razor_slices::{
    html, partial, section, with_layout, with_section, HtmlString, InlineSlice, Layout, Slice,
    WriteHtmlValue,
};
use std::{cell::RefCell, fmt, io};

const EXPECTED: &str = include_str!("fixtures/sections.html");

struct Shell;

impl Layout for Shell {
    fn render_layout(&self, body: &dyn Slice, writer: &mut dyn io::Write) -> io::Result<()> {
        html! {
            "[" {section(body, "header")} {partial(body)}
            {section(body, "footer")} {section(body, "missing")} "]"
        }
        .render_to_writer(writer)
    }
}

#[test]
fn typed_full_page_is_reusable_and_matches_exact_output() {
    let model = page::sample_model();
    let todos = todos::sample_models();
    let document = page::todos_document(&model, &todos).unwrap();
    for _ in 0..2 {
        assert_eq!(document.render_to_string().unwrap(), EXPECTED);
        assert_eq!(document.render_to_bytes().unwrap(), EXPECTED.as_bytes());
        let mut bytes = Vec::new();
        document.render_to_writer(&mut bytes).unwrap();
        assert_eq!(bytes, EXPECTED.as_bytes());
    }
}

#[test]
fn body_and_sections_encode_their_values_once_and_keep_trusted_markup() {
    let text = String::from("<&>\"'世界");
    let text = &text;
    let body = with_section(
        html! { "<p>" {&text} "</p>" },
        "header",
        html! { "<h1>" {&text} {HtmlString::from("<br>")} "</h1>" },
    )
    .unwrap();
    let expected = "<h1>&lt;&amp;&gt;&quot;&#39;世界<br></h1>";
    for _ in 0..2 {
        let mut bytes = Vec::new();
        section(&body, "header").write_html_to(&mut bytes).unwrap();
        assert_eq!(String::from_utf8(bytes).unwrap(), expected);
        assert_eq!(
            body.render_to_string().unwrap(),
            "<p>&lt;&amp;&gt;&quot;&#39;世界</p>"
        );
    }
}

#[test]
fn named_sections_are_lazy_and_execute_at_each_insertion() {
    let events = RefCell::new(Vec::new());
    let recorder = &events;
    let body = InlineSlice::new(move |writer: &mut dyn io::Write| {
        recorder.borrow_mut().push("body");
        writer.write_all(b"B")
    });
    let header = InlineSlice::new(move |writer: &mut dyn io::Write| {
        recorder.borrow_mut().push("header");
        writer.write_all(b"H")
    });
    let footer = InlineSlice::new(move |writer: &mut dyn io::Write| {
        recorder.borrow_mut().push("footer");
        writer.write_all(b"F")
    });
    let body = with_section(
        with_section(body, "header", header).unwrap(),
        "footer",
        footer,
    )
    .unwrap();
    let document = with_layout(&body, Shell);
    assert!(events.borrow().is_empty());
    for _ in 0..2 {
        assert_eq!(document.render_to_string().unwrap(), "[HBF]");
    }
    let borrowed = &body;
    let repeated = html! { {section(borrowed, "header")} {section(borrowed, "header")} };
    assert_eq!(repeated.render_to_string().unwrap(), "HH");
    assert_eq!(
        *events.borrow(),
        ["header", "body", "footer", "header", "body", "footer", "header", "header"]
    );
}

#[test]
fn ordinary_body_rendering_does_not_execute_sections() {
    let body = with_section(
        html! { "body" },
        "unused",
        InlineSlice::new(|_: &mut dyn io::Write| -> io::Result<()> {
            panic!("unrequested section must not execute")
        }),
    )
    .unwrap();
    assert_eq!(body.render_to_string().unwrap(), "body");
    assert_eq!(
        html! { {partial(&body)} }.render_to_string().unwrap(),
        "body"
    );
}

#[test]
fn association_owns_the_name_but_can_borrow_models_and_section_slices() {
    let title = String::from("<Title>");
    let borrowed = &title;
    let content = html! { "<h1>" {borrowed} "</h1>" };
    let body;
    {
        let name = String::from("header");
        body = with_section(html! { "B" }, &name, &content).unwrap();
    }
    assert_eq!(
        with_layout(body, Shell).render_to_string().unwrap(),
        "[<h1>&lt;Title&gt;</h1>B]"
    );
    assert_eq!(title, "<Title>");
}

#[test]
fn unknown_sections_are_optional_case_sensitive_and_do_not_write() {
    let body = with_section(html! {}, "header", html! { "H" }).unwrap();
    let named = with_section(body, "世界", html! { "U" }).unwrap();
    let named = with_section(named, " ", html! { "S" }).unwrap();
    for name in ["missing", "Header", "世界 ", " header"] {
        let mut bytes = b"prefix".to_vec();
        section(&named, name).write_html_to(&mut bytes).unwrap();
        assert_eq!(bytes, b"prefix");
    }
    let page =
        html! { {section(&named, "header")} {section(&named, "世界")} {section(&named, " ")} };
    assert_eq!(page.render_to_string().unwrap(), "HUS");
    let mut bytes = Vec::new();
    html! { "unused" }
        .render_section_to_writer("unknown", &mut bytes)
        .unwrap();
    assert!(bytes.is_empty());
}

#[test]
fn empty_names_fail_at_construction_and_at_every_public_dispatch_boundary() {
    assert_eq!(
        with_section(html! {}, "", html! {}).err().unwrap().kind(),
        io::ErrorKind::InvalidInput
    );
    let body = with_section(html! {}, "header", html! { "H" }).unwrap();
    let mut bytes = Vec::new();
    for result in [
        body.render_section_to_writer("", &mut bytes),
        html! {}.render_section_to_writer("", &mut bytes),
        section(&body, "").write_html_to(&mut bytes),
    ] {
        let error = result.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(error.to_string(), "section name must not be empty");
    }
    assert!(bytes.is_empty());
    assert!(html! { {section(&body, "")} }.render_to_string().is_err());
}

#[test]
fn outer_association_overrides_a_name_without_evaluating_shadowed_content() {
    let body = with_section(
        html! { "B" },
        "header",
        InlineSlice::new(|_: &mut dyn io::Write| -> io::Result<()> {
            panic!("shadowed content must not execute")
        }),
    )
    .unwrap();
    let body = with_section(body, "header", html! { "new" }).unwrap();
    assert_eq!(
        with_layout(body, Shell).render_to_string().unwrap(),
        "[newB]"
    );
}

#[test]
fn references_and_boxes_forward_custom_section_dispatch_without_rendering_body() {
    struct Provider;
    impl Slice for Provider {
        fn render(&self, _: &mut dyn fmt::Write) -> fmt::Result {
            panic!("section lookup must not render body")
        }
        fn render_section_to_writer(
            &self,
            name: &str,
            writer: &mut dyn io::Write,
        ) -> io::Result<()> {
            assert_eq!(name, "custom");
            writer.write_all("世界".as_bytes())
        }
    }
    let body: Box<dyn Slice> = Box::new(Provider);
    let reference: &dyn Slice = &body;
    let value = html! { {section(reference, "custom")} };
    assert_eq!(value.render_to_string().unwrap(), "世界");
    let mut bytes = Vec::new();
    section(&body, "custom").write_html_to(&mut bytes).unwrap();
    assert_eq!(bytes, "世界".as_bytes());
}

#[test]
fn association_preserves_body_convenience_overrides_and_byte_native_sections() {
    struct Body;
    impl Slice for Body {
        fn render(&self, _: &mut dyn fmt::Write) -> fmt::Result {
            panic!("body override should be forwarded")
        }
        fn render_to_writer(&self, writer: &mut dyn io::Write) -> io::Result<()> {
            writer.write_all(b"W")
        }
        fn render_to_string(&self) -> Result<String, razor_slices::RenderError> {
            Ok("S".into())
        }
        fn render_to_bytes(&self) -> Result<Vec<u8>, razor_slices::RenderError> {
            Ok(b"B".to_vec())
        }
    }
    struct ByteSection;
    impl Slice for ByteSection {
        fn render(&self, _: &mut dyn fmt::Write) -> fmt::Result {
            panic!("byte section override must be used")
        }
        fn render_to_writer(&self, writer: &mut dyn io::Write) -> io::Result<()> {
            writer.write_all(b"H")
        }
    }
    let body = with_section(Body, "header", ByteSection).unwrap();
    assert_eq!(body.render_to_string().unwrap(), "S");
    assert_eq!(body.render_to_bytes().unwrap(), b"B");
    let mut bytes = Vec::new();
    body.render_to_writer(&mut bytes).unwrap();
    assert_eq!(bytes, b"W");
    assert_eq!(with_layout(body, Shell).render_to_string().unwrap(), "[HW]");
}

#[derive(Debug)]
struct SectionFailure;

impl fmt::Display for SectionFailure {
    fn fmt(&self, writer: &mut fmt::Formatter<'_>) -> fmt::Result {
        writer.write_str("section failed")
    }
}

impl std::error::Error for SectionFailure {}

#[test]
fn section_failure_preserves_source_and_skips_body_and_layout_tail() {
    let body = InlineSlice::new(|_: &mut dyn io::Write| -> io::Result<()> {
        panic!("body must not execute after failed header")
    });
    let header = InlineSlice::new(|writer: &mut dyn io::Write| {
        writer.write_all(b"prefix")?;
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            SectionFailure,
        ))
    });
    let body = with_section(body, "header", header).unwrap();
    let document = with_layout(body, Shell);
    let mut bytes = Vec::new();
    let error = document.render_to_writer(&mut bytes).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(error.get_ref().unwrap().is::<SectionFailure>());
    assert_eq!(bytes, b"[prefix");
    assert!(document.render_to_string().is_err());
}

#[test]
fn failing_section_value_keeps_original_error_and_stops_its_tail() {
    struct Value;
    impl WriteHtmlValue for Value {
        fn write_html_to(&self, _: &mut dyn io::Write) -> io::Result<()> {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                SectionFailure,
            ))
        }
    }
    let body = with_section(html! { "B" }, "header", html! { "H" {Value} "tail" }).unwrap();
    let mut bytes = Vec::new();
    let error = with_layout(body, Shell)
        .render_to_writer(&mut bytes)
        .unwrap_err();
    assert!(error.get_ref().unwrap().is::<SectionFailure>());
    assert_eq!(bytes, b"[H");
}

#[test]
fn body_failure_skips_footer_and_footer_failure_skips_layout_tail() {
    for fail_body in [true, false] {
        let body = InlineSlice::new(move |writer: &mut dyn io::Write| {
            writer.write_all(b"B")?;
            if fail_body {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    SectionFailure,
                ))
            } else {
                Ok(())
            }
        });
        let footer = InlineSlice::new(move |writer: &mut dyn io::Write| {
            assert!(!fail_body, "footer must not execute after body failure");
            writer.write_all(b"F")?;
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                SectionFailure,
            ))
        });
        let body = with_section(body, "header", html! { "H" }).unwrap();
        let document = with_layout(with_section(body, "footer", footer).unwrap(), Shell);
        let mut bytes = Vec::new();
        let error = document.render_to_writer(&mut bytes).unwrap_err();
        assert!(error.get_ref().unwrap().is::<SectionFailure>());
        assert_eq!(
            bytes,
            if fail_body {
                b"[HB".as_slice()
            } else {
                b"[HBF"
            }
        );
    }
}

#[test]
fn optional_absence_does_not_touch_sink_and_invalid_insertion_skips_provider() {
    struct Sink;
    impl io::Write for Sink {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            panic!("optional absence must not write")
        }
        fn flush(&mut self) -> io::Result<()> {
            panic!("optional absence must not flush")
        }
    }
    let body = with_section(html! {}, "header", html! { "H" }).unwrap();
    section(&body, "missing").write_html_to(&mut Sink).unwrap();

    struct Provider;
    impl Slice for Provider {
        fn render(&self, _: &mut dyn fmt::Write) -> fmt::Result {
            panic!("not reached")
        }
        fn render_section_to_writer(&self, _: &str, _: &mut dyn io::Write) -> io::Result<()> {
            panic!("invalid insertion must fail before calling custom provider")
        }
    }
    assert_eq!(
        section(Provider, "")
            .write_html_to(&mut Sink)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
}

#[test]
fn nested_layout_sections_are_explicit_not_implicitly_inherited() {
    let inner_body = with_section(html! { "B" }, "header", html! { "inner" }).unwrap();
    let inner_document = with_layout(inner_body, Shell);
    let outer_body = with_section(inner_document, "header", html! { "outer" }).unwrap();
    let document = with_layout(outer_body, Shell);
    assert_eq!(document.render_to_string().unwrap(), "[outer[innerB]]");
}

#[test]
fn interrupted_short_sink_writes_and_failures_stop_in_render_order_without_flush() {
    struct Sink {
        bytes: Vec<u8>,
        limit: usize,
        interrupted: bool,
    }
    impl io::Write for Sink {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if !self.interrupted {
                self.interrupted = true;
                return Err(io::ErrorKind::Interrupted.into());
            }
            if self.bytes.len() == self.limit {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, SectionFailure));
            }
            let count = bytes.len().min(1);
            self.bytes.extend_from_slice(&bytes[..count]);
            Ok(count)
        }
        fn flush(&mut self) -> io::Result<()> {
            panic!("section rendering must not flush")
        }
    }
    for limit in [0, 1, 2, 3, 4, 5] {
        let events = RefCell::new(Vec::new());
        let recorder = &events;
        let make = |name: &'static str, byte: &'static [u8]| {
            InlineSlice::new(move |writer: &mut dyn io::Write| {
                recorder.borrow_mut().push(name);
                writer.write_all(byte)
            })
        };
        let body = with_section(make("body", b"B"), "header", make("header", b"H")).unwrap();
        let body = with_section(body, "footer", make("footer", b"F")).unwrap();
        let document = with_layout(body, Shell);
        let mut sink = Sink {
            bytes: Vec::new(),
            limit,
            interrupted: false,
        };
        let result = document.render_to_writer(&mut sink);
        if limit == 5 {
            result.unwrap();
        } else {
            let error = result.unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
            assert!(error.get_ref().unwrap().is::<SectionFailure>());
        }
        assert_eq!(sink.bytes, b"[HBF]"[..limit]);
        assert_eq!(
            *events.borrow(),
            match limit {
                0 => vec![],
                1 => vec!["header"],
                2 => vec!["header", "body"],
                _ => vec!["header", "body", "footer"],
            }
        );
    }
}

#[test]
fn text_sink_failure_before_insertion_does_not_execute_section() {
    struct Sink;
    impl fmt::Write for Sink {
        fn write_str(&mut self, _: &str) -> fmt::Result {
            Err(fmt::Error)
        }
    }
    let body = with_section(
        html! {},
        "header",
        InlineSlice::new(|_: &mut dyn io::Write| -> io::Result<()> { panic!("not reached") }),
    )
    .unwrap();
    assert!(with_layout(body, Shell).render(&mut Sink).is_err());
}

#[tokio::test]
async fn full_document_is_compatible_with_the_existing_buffered_async_bridge() {
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
    let model = page::sample_model();
    let todos = todos::sample_models();
    let document = page::todos_document(&model, &todos).unwrap();
    let mut writer = Writer(Vec::new());
    razor_slices::render_async(&document, &mut writer)
        .await
        .unwrap();
    assert_eq!(writer.0, EXPECTED.as_bytes());
}
