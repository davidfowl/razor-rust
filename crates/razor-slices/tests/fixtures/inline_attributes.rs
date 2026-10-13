//! Integration probe for this adapter plus the unmerged #33/#31 consumer.
//! Copy to tests/inline_attributes.rs only in an isolated combined checkout.

use std::{fmt, io};

use razor_slices::{attribute, html, partial, AttributeValue, HtmlString, Slice};

struct Model<'a> {
    label: &'a str,
    completed: bool,
    class: Option<&'a str>,
}

fn input<'a>(model: &'a Model<'a>) -> impl Slice + 'a {
    html! {
        "<input"
        {attribute("type", "checkbox").expect("constant valid name")}
        {attribute("checked", model.completed).expect("constant valid name")}
        {attribute("class", model.class).expect("constant valid name")}
        {attribute("aria-label", model.label).expect("constant valid name")}
        ">\n"
    }
}

#[test]
fn inline_model_and_partial_render_exact_fixture_repeatedly() {
    let first = Model {
        label: "Wash <dishes> & \"cups\"",
        completed: true,
        class: Some(""),
    };
    let second = Model {
        label: "Call 'Grandma' 世界",
        completed: false,
        class: None,
    };
    let first = input(&first);
    let second = input(&second);
    let page = html! { {partial(&first)} {partial(&second)} };
    for _ in 0..3 {
        assert_eq!(
            page.render_to_string().unwrap().as_bytes(),
            include_bytes!("fixtures/attributes.html")
        );
        let mut bytes = Vec::new();
        page.render_to_writer(&mut bytes).unwrap();
        assert_eq!(bytes, include_bytes!("fixtures/attributes.html"));
    }
}

#[test]
fn inline_trusted_value_is_not_encoded_again() {
    let value = HtmlString::from("&lt;trusted&gt; 世界");
    let page = html! {
        "<p" {attribute("title", &value).expect("constant valid name")} "></p>"
    };
    assert_eq!(
        page.render_to_string().unwrap(),
        "<p title=\"&lt;trusted&gt; 世界\"></p>"
    );
}

struct FailedValue;

impl AttributeValue for FailedValue {
    fn write_attribute_value(&self, _: &str, _: &mut dyn io::Write) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "original attribute error",
        ))
    }
}

#[test]
fn nested_partial_preserves_attribute_error_and_no_suffix() {
    let child = html! { "<p" {attribute("title", FailedValue).expect("constant valid name")} ">" };
    let page = html! { "<main>" {partial(&child)} "</main>" };
    let mut bytes = Vec::new();
    let error = page.render_to_writer(&mut bytes).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(error.to_string(), "original attribute error");
    assert_eq!(bytes, b"<main><p title=\"");
    assert!(page.render_to_string().is_err());
}

struct FailedTextWriter;

impl fmt::Write for FailedTextWriter {
    fn write_str(&mut self, _: &str) -> fmt::Result {
        Err(fmt::Error)
    }
}

#[test]
fn inline_text_writer_failure_is_not_hidden() {
    let page = html! { {attribute("checked", true).expect("constant valid name")} };
    assert!(page.render(&mut FailedTextWriter).is_err());
}
