use razor_slices::{html, partial, section, Slice};
use std::{fmt, io};

#[path = "../examples/support/integration.rs"]
mod sample;
#[path = "../examples/support/todos.rs"]
#[allow(dead_code)]
mod todos;

fn expected() -> (&'static str, &'static str) {
    include_str!("fixtures/integration.html")
        .split_once("<!-- partial -->\n")
        .unwrap()
}

#[test]
fn combined_features_repeat_on_text_bytes_writer_and_nested_wrappers() {
    let model = sample::sample_model();
    let page: Box<dyn Slice> = Box::new(sample::page(&model).unwrap());
    let wrapped = Box::new(&page);
    let wrapped = &wrapped;
    for _ in 0..2 {
        assert_eq!(wrapped.render_to_string().unwrap(), expected().0);
        assert_eq!(wrapped.render_to_bytes().unwrap(), expected().0.as_bytes());
        let mut bytes = Vec::new();
        wrapped.render_to_writer(&mut bytes).unwrap();
        assert_eq!(bytes, expected().0.as_bytes());
        assert_eq!(
            html! { {partial(wrapped)} }.render_to_string().unwrap(),
            expected().1
        );
    }
}

#[test]
fn declaration_forwards_named_sections_without_applying_layout() {
    let model = sample::sample_model();
    let page = sample::page(&model).unwrap();
    let page: &dyn Slice = &page;
    assert_eq!(
        html! { {section(page, "footer")} {section(page, "optional")} }
            .render_to_string()
            .unwrap(),
        "<footer>&lt;done&gt; &amp; reusable</footer>\n"
    );
}

#[test]
fn empty_model_preserves_layout_and_section_but_partial_is_bare() {
    let model = sample::Model {
        title: "<empty>".into(),
        footer: "&".into(),
        todos: Vec::new(),
    };
    let page = sample::page(&model).unwrap();
    assert_eq!(
        page.render_to_string().unwrap(),
        "<!doctype html>\n<html><head><title>&lt;empty&gt;</title></head><body>\n\
         <ul>\n</ul>\n<footer>&amp;</footer>\n</body></html>\n"
    );
    assert_eq!(
        html! { {partial(&page)} }.render_to_string().unwrap(),
        "<ul>\n</ul>\n"
    );
}

#[derive(Debug)]
struct DestinationFailure;

impl fmt::Display for DestinationFailure {
    fn fmt(&self, writer: &mut fmt::Formatter<'_>) -> fmt::Result {
        writer.write_str("integration destination failure")
    }
}

impl std::error::Error for DestinationFailure {}

struct FailingWriter {
    bytes: Vec<u8>,
    remaining: usize,
}

impl io::Write for FailingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                DestinationFailure,
            ));
        }
        let len = bytes.len().min(self.remaining);
        self.bytes.extend_from_slice(&bytes[..len]);
        self.remaining -= len;
        Ok(len)
    }

    fn flush(&mut self) -> io::Result<()> {
        panic!("synchronous composition must not flush")
    }
}

#[test]
fn combined_layout_attribute_and_section_failures_preserve_destination_source() {
    let model = sample::sample_model();
    let page = sample::page(&model).unwrap();
    let full = expected().0;
    for limit in [
        0,
        full.find("checked").unwrap() + 3,
        full.find("<footer>").unwrap() + 9,
    ] {
        let mut writer = FailingWriter {
            bytes: Vec::new(),
            remaining: limit,
        };
        let error = page.render_to_writer(&mut writer).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
        assert!(error.get_ref().unwrap().is::<DestinationFailure>());
        assert_eq!(writer.bytes, full.as_bytes()[..limit]);
    }
}
