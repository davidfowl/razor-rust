use std::io::{self, Write};

use razor_slices::{attribute, AttributeValue, HtmlString, WriteHtmlValue};

#[path = "../examples/attributes.rs"]
mod example;

fn rendered(value: impl WriteHtmlValue) -> String {
    let mut bytes = Vec::new();
    value.write_html_to(&mut bytes).unwrap();
    String::from_utf8(bytes).unwrap()
}

#[test]
fn upstream_boolean_cases_and_nullable_variants() {
    assert_eq!(
        rendered(attribute("checked", true).unwrap()),
        " checked=\"checked\""
    );
    assert_eq!(rendered(attribute("checked", false).unwrap()), "");
    assert_eq!(
        rendered(attribute("checked", Some(true)).unwrap()),
        " checked=\"checked\""
    );
    assert_eq!(rendered(attribute("checked", Some(false)).unwrap()), "");
    assert_eq!(rendered(attribute("checked", None::<bool>).unwrap()), "");
    assert_eq!(rendered(attribute("checked", &Some(false)).unwrap()), "");
    assert_eq!(
        rendered(attribute("checked", Some(Some(true))).unwrap()),
        " checked=\"checked\""
    );
    assert_eq!(
        rendered(attribute("checked", Some(None::<bool>)).unwrap()),
        ""
    );
}

#[test]
fn upstream_string_cases_retain_empty_values() {
    for (value, expected) in [
        (None, ""),
        (Some(""), " class=\"\""),
        (Some("active"), " class=\"active\""),
    ] {
        assert_eq!(rendered(attribute("class", value).unwrap()), expected);
    }
    assert_eq!(rendered(attribute("class", "").unwrap()), " class=\"\"");
    assert_eq!(
        rendered(attribute("class", String::new()).unwrap()),
        " class=\"\""
    );
    assert_eq!(
        rendered(attribute("class", HtmlString::empty()).unwrap()),
        " class=\"\""
    );
}

#[test]
fn values_escape_once_and_preserve_unicode() {
    let text = "\"<&>' 世界 😀 &lt;";
    let expected = " title=\"&quot;&lt;&amp;&gt;&#39; 世界 😀 &amp;lt;\"";
    assert_eq!(rendered(attribute("title", text).unwrap()), expected);
    assert_eq!(
        rendered(attribute("title", text.to_owned()).unwrap()),
        expected
    );
    assert_eq!(rendered(attribute("title", Some(text)).unwrap()), expected);
}

#[test]
fn trusted_values_preserve_upstream_raw_dispatch() {
    let trusted = HtmlString::from("&lt;already encoded&gt; &#34; 世界");
    assert_eq!(
        rendered(attribute("title", &trusted).unwrap()),
        " title=\"&lt;already encoded&gt; &#34; 世界\""
    );
    assert_eq!(
        rendered(attribute("title", Some(&trusted)).unwrap()),
        " title=\"&lt;already encoded&gt; &#34; 世界\""
    );
}

#[test]
fn valid_names_preserve_spelling_and_boolean_name() {
    for name in [
        "checked",
        "CHECKED",
        "data-id",
        "aria-label",
        "xml:lang",
        "_custom",
        ":custom",
        "a.b-9",
    ] {
        assert_eq!(
            rendered(attribute(name, true).unwrap()),
            format!(" {name}=\"{name}\"")
        );
    }
}

#[test]
fn invalid_names_error_even_when_omitted() {
    let empty = attribute("", None::<bool>).unwrap_err();
    assert_eq!(empty.byte_index(), None);
    assert_eq!(empty.to_string(), "HTML attribute name must not be empty");
    for (name, index) in [
        ("9id", 0),
        ("-id", 0),
        (".id", 0),
        ("a b", 1),
        ("a\tb", 1),
        ("a\nb", 1),
        ("a\rb", 1),
        ("a\0b", 1),
        ("a\u{7f}b", 1),
        ("a\"b", 1),
        ("a'b", 1),
        ("a=b", 1),
        ("a<b", 1),
        ("a>b", 1),
        ("a/b", 1),
        ("a`b", 1),
        ("a&b", 1),
        ("世界", 0),
        ("a世界", 1),
        ("checked=\"checked\" onclick", 7),
    ] {
        let error = attribute(name, false).unwrap_err();
        assert_eq!(error.byte_index(), Some(index), "{name:?}");
        assert_eq!(
            error.to_string(),
            format!("invalid HTML attribute name byte at offset {index}")
        );
        assert!(attribute(name, "value").is_err());
    }
}

#[test]
fn primitive_values_use_existing_writer_representations() {
    macro_rules! check {
        ($($value:expr),* $(,)?) => {
            $(
                let value = $value;
                let expected = format!(" value=\"{}\"", rendered(value));
                assert_eq!(rendered(attribute("value", value).unwrap()), expected);
            )*
        };
    }
    check!(
        i8::MIN,
        i16::MIN,
        i32::MIN,
        i64::MIN,
        i128::MIN,
        isize::MIN,
        u8::MAX,
        u16::MAX,
        u32::MAX,
        u64::MAX,
        u128::MAX,
        usize::MAX,
        0u8,
        1.25f32,
        -0.0f64,
        f64::MIN_POSITIVE,
        f64::INFINITY,
        f64::NAN
    );
}

#[test]
fn borrowed_model_and_name_support_repeated_renders() {
    let name = String::from("title");
    let model = String::from("<model> 世界");
    let value = attribute(&name, Some(model.as_str())).unwrap();
    for _ in 0..3 {
        assert_eq!(
            rendered(BorrowedAttribute(&value)),
            " title=\"&lt;model&gt; 世界\""
        );
    }
}

impl WriteHtmlValue for BorrowedAttribute<'_> {
    fn write_html_to(&self, writer: &mut dyn Write) -> io::Result<()> {
        self.0.write_html_to(writer)
    }
}

struct BorrowedAttribute<'a>(&'a dyn WriteHtmlValue);

#[test]
fn trait_object_value_delegation() {
    let text = String::from("<dynamic>");
    let value: &dyn AttributeValue = &text;
    assert_eq!(
        rendered(attribute("title", value).unwrap()),
        " title=\"&lt;dynamic&gt;\""
    );
    let boolean: &dyn AttributeValue = &false;
    assert_eq!(rendered(attribute("checked", boolean).unwrap()), "");
}

struct ShortWriter {
    bytes: Vec<u8>,
    interrupt: bool,
}

impl Write for ShortWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.interrupt {
            self.interrupt = false;
            return Err(io::ErrorKind::Interrupted.into());
        }
        self.interrupt = true;
        let count = bytes.len().min(1);
        self.bytes.extend_from_slice(&bytes[..count]);
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        panic!("attributes must not flush")
    }
}

#[test]
fn short_and_interrupted_writes_preserve_utf8_and_entities() {
    let mut writer = ShortWriter {
        bytes: Vec::new(),
        interrupt: true,
    };
    attribute("title", "<世界> & \"😀\"")
        .unwrap()
        .write_html_to(&mut writer)
        .unwrap();
    assert_eq!(
        writer.bytes,
        " title=\"&lt;世界&gt; &amp; &quot;😀&quot;\"".as_bytes()
    );
}

#[derive(Debug)]
struct SinkFailure;

impl std::fmt::Display for SinkFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("original sink failure")
    }
}

impl std::error::Error for SinkFailure {}

struct FailingWriter {
    bytes: Vec<u8>,
    remaining: usize,
}

impl Write for FailingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, SinkFailure));
        }
        let count = bytes.len().min(self.remaining);
        self.bytes.extend_from_slice(&bytes[..count]);
        self.remaining -= count;
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        panic!("attributes must not flush")
    }
}

#[test]
fn sink_errors_at_every_output_byte_preserve_kind_payload_and_prefix() {
    let value = attribute("title", "<世界> & \"😀\"").unwrap();
    let expected = rendered(BorrowedAttribute(&value)).into_bytes();
    for limit in 0..expected.len() {
        let mut writer = FailingWriter {
            bytes: Vec::new(),
            remaining: limit,
        };
        let error = value.write_html_to(&mut writer).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
        assert!(error.get_ref().unwrap().is::<SinkFailure>());
        assert_eq!(writer.bytes, expected[..limit]);
    }
}

#[test]
fn omitted_attributes_do_not_touch_a_failing_sink() {
    let mut writer = FailingWriter {
        bytes: Vec::new(),
        remaining: 0,
    };
    attribute("checked", false)
        .unwrap()
        .write_html_to(&mut writer)
        .unwrap();
    attribute("title", None::<&str>)
        .unwrap()
        .write_html_to(&mut writer)
        .unwrap();
    assert!(writer.bytes.is_empty());
}

struct FailedValue;

impl AttributeValue for FailedValue {
    fn write_attribute_value(&self, _: &str, writer: &mut dyn Write) -> io::Result<()> {
        writer.write_all(b"partial")?;
        Err(io::Error::new(io::ErrorKind::InvalidData, SinkFailure))
    }
}

#[test]
fn custom_value_errors_are_not_hidden_or_followed_by_suffix() {
    let mut output = Vec::new();
    let error = attribute("title", Some(FailedValue))
        .unwrap()
        .write_html_to(&mut output)
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.get_ref().unwrap().is::<SinkFailure>());
    assert_eq!(output, b" title=\"partial");
}

#[test]
fn write_zero_is_reported() {
    assert_eq!(
        attribute("title", "value")
            .unwrap()
            .write_html_to(&mut [].as_mut_slice())
            .unwrap_err()
            .kind(),
        io::ErrorKind::WriteZero
    );
}

#[test]
fn executable_example_matches_complete_fixture() {
    assert_eq!(
        example::render_example().unwrap(),
        include_bytes!("fixtures/attributes.html")
    );
}
