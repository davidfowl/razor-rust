use razor_slices::{
    encode_html_to_writer, DefaultHtmlEncoder, Encoded, Escape, HtmlEncoderPolicy,
    UnicodeHtmlEncoder, WriteHtmlValue,
};

fn enc(policy: &dyn HtmlEncoderPolicy, s: &str) -> String {
    let mut out = Vec::new();
    policy.encode_to_writer(s, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn default_policy_is_byte_identical_to_free_function() {
    for s in [
        "",
        "plain",
        "<a href=\"x\">&'</a>",
        "雪 🦀 <>",
        "\u{0}\t\n\u{7f}",
    ] {
        let mut expected = Vec::new();
        encode_html_to_writer(s, &mut expected).unwrap();
        assert_eq!(enc(&DefaultHtmlEncoder, s).as_bytes(), expected.as_slice());
        let mut fmt = String::new();
        DefaultHtmlEncoder.encode_to(s, &mut fmt).unwrap();
        assert_eq!(fmt.as_bytes(), expected.as_slice());
    }
}

#[test]
fn default_policy_leaves_unicode_raw() {
    assert_eq!(enc(&DefaultHtmlEncoder, "雪🦀'"), "雪🦀&#39;");
}

#[test]
fn unicode_policy_escapes_non_ascii_and_controls() {
    assert_eq!(enc(&UnicodeHtmlEncoder, "雪"), "&#x96EA;");
    assert_eq!(enc(&UnicodeHtmlEncoder, "🦀"), "&#x1F980;");
    assert_eq!(
        enc(&UnicodeHtmlEncoder, "a<b>&\"'"),
        "a&lt;b&gt;&amp;&quot;&#39;"
    );
    assert_eq!(
        enc(&UnicodeHtmlEncoder, "\u{0}\u{7f}\t\n\r"),
        "&#x0;&#x7F;\t\n\r"
    );
    assert_eq!(enc(&UnicodeHtmlEncoder, "é-ok"), "&#xE9;-ok");
}

#[test]
fn unicode_policy_output_is_ascii_and_fmt_matches_io() {
    let s = "mixed é雪🦀 <tag> \u{1}";
    let out = enc(&UnicodeHtmlEncoder, s);
    assert!(out.is_ascii());
    let mut fmt = String::new();
    UnicodeHtmlEncoder.encode_to(s, &mut fmt).unwrap();
    assert_eq!(fmt, out);
}

#[test]
fn custom_policy_and_encoded_wrapper() {
    struct NoSlash;
    impl HtmlEncoderPolicy for NoSlash {
        fn escape(&self, c: char) -> Escape {
            match c {
                '/' => Escape::Entity("&#47;"),
                'é' => Escape::NumericReference,
                _ => Escape::Keep,
            }
        }
    }
    let mut out = Vec::new();
    Encoded::new("a/é<", NoSlash)
        .write_html_to(&mut out)
        .unwrap();
    assert_eq!(out, b"a&#47;&#xE9;<");
}

#[test]
fn write_errors_propagate() {
    struct Fail;
    impl std::io::Write for Fail {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("boom"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    assert!(UnicodeHtmlEncoder
        .encode_to_writer("雪", &mut Fail)
        .is_err());
}
