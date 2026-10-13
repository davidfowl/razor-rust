use razor_slices::{html, HtmlString, Slice, WriteHtmlValue};
use std::{cell::Cell, fmt, io};

#[test]
fn literals_values_and_repeated_rendering() {
    let name = String::from("<&>\"' 世界");
    let raw = HtmlString::from("<b>raw</b>");
    let value = html! { "\n<p>" {name} " " {raw} " " {42} " " {false} "</p>\n" };
    let expected = "\n<p>&lt;&amp;&gt;&quot;&#39; 世界 <b>raw</b> 42 false</p>\n";
    for _ in 0..2 {
        assert_eq!(value.render_to_string().unwrap(), expected);
        assert_eq!(value.render_to_bytes().unwrap(), expected.as_bytes());
        let mut bytes = Vec::new();
        value.write_to(&mut bytes).unwrap();
        assert_eq!(bytes, expected.as_bytes());
    }
    assert_eq!(html! {}.render_to_string().unwrap(), "");
    assert_eq!(
        html! { r#" {literal} " "# }.render_to_string().unwrap(),
        " {literal} \" "
    );
}

#[test]
fn borrowing_capture_timing_and_hygiene() {
    fn greeting(name: &str) -> impl Slice + '_ {
        html! { {name} }
    }
    let name = String::from("David");
    assert_eq!(greeting(&name).render_to_string().unwrap(), "David");
    assert_eq!(name, "David");
    let calls = Cell::new(0);
    let counter = &calls;
    let __razor_writer = "not the output writer";
    let value = html! { {__razor_writer} {{ counter.set(counter.get() + 1); counter.get() }} };
    assert_eq!(calls.get(), 0);
    assert_eq!(value.render_to_string().unwrap(), "not the output writer1");
    assert_eq!(value.render_to_string().unwrap(), "not the output writer2");
}

struct Chunks(Vec<Vec<u8>>);

impl WriteHtmlValue for Chunks {
    fn write_html_to(&self, writer: &mut dyn io::Write) -> io::Result<()> {
        for chunk in &self.0 {
            writer.write_all(chunk)?;
        }
        Ok(())
    }
}

#[test]
fn text_bridge_handles_every_utf8_split() {
    let text = "Aé中🦀Z";
    for split in 0..=text.len() {
        let value = Chunks(vec![
            text.as_bytes()[..split].to_vec(),
            Vec::new(),
            text.as_bytes()[split..].to_vec(),
        ]);
        assert_eq!(html! { {value} }.render_to_string().unwrap(), text);
    }
    let value = Chunks(text.bytes().map(|byte| vec![byte]).collect());
    assert_eq!(html! { {value} }.render_to_string().unwrap(), text);
}

#[test]
fn text_bridge_rejects_invalid_and_incomplete_utf8() {
    for chunks in [
        vec![vec![0xff]],
        vec![vec![0xc3]],
        vec![vec![0xc3], vec![b'x']],
        vec![vec![0xf0, 0x9f], vec![0xa6]],
    ] {
        let value = Chunks(chunks);
        assert!(html! { {value} }.render_to_string().is_err());
    }
}

struct Failure;

impl WriteHtmlValue for Failure {
    fn write_html_to(&self, _: &mut dyn io::Write) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "value failed",
        ))
    }
}

#[test]
fn value_error_keeps_source_and_stops_rendering() {
    let calls = Cell::new(0);
    let counter = &calls;
    let value = html! { "prefix" {Failure} {{ counter.set(1); "suffix" }} };
    let mut bytes = Vec::new();
    let error = value.write_to(&mut bytes).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(error.to_string(), "value failed");
    assert_eq!(bytes, b"prefix");
    assert_eq!(calls.get(), 0);
    assert!(value.render_to_string().is_err());
    assert_eq!(calls.get(), 0);
}

struct BrokenText;

impl fmt::Write for BrokenText {
    fn write_str(&mut self, _: &str) -> fmt::Result {
        Err(fmt::Error)
    }
}

struct ShortWriter {
    bytes: Vec<u8>,
    remaining: usize,
}

impl io::Write for ShortWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "sink failed"));
        }
        let count = bytes.len().min(self.remaining).min(1);
        self.bytes.extend_from_slice(&bytes[..count]);
        self.remaining -= count;
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        panic!("rendering must not flush the destination")
    }
}

#[test]
fn destination_errors_and_short_writes() {
    let value = html! { "<p>" {"&"} "</p>" };
    assert!(value.render(&mut BrokenText).is_err());
    for remaining in [1, 4, 8] {
        let mut sink = ShortWriter {
            bytes: Vec::new(),
            remaining,
        };
        let error = value.write_to(&mut sink).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
        assert_eq!(error.to_string(), "sink failed");
        assert_eq!(sink.bytes.len(), remaining);
    }
    let mut sink = ShortWriter {
        bytes: Vec::new(),
        remaining: 100,
    };
    value.write_to(&mut sink).unwrap();
    assert_eq!(sink.bytes, b"<p>&amp;</p>");
}

#[tokio::test]
async fn works_with_existing_async_bridge() {
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
    let mut writer = Writer(Vec::new());
    razor_slices::render_async(&html! { "<p>" {"<&>"} "</p>" }, &mut writer)
        .await
        .unwrap();
    assert_eq!(writer.0, b"<p>&lt;&amp;&gt;</p>");
}
