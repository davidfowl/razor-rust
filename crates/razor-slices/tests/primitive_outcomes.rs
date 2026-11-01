use razor_slices::WriteHtmlValue;

fn render(value: &dyn WriteHtmlValue) -> String {
    let mut out = Vec::new();
    value.write_html_to(&mut out).unwrap();
    String::from_utf8(out).unwrap()
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Outcome {
    /// Rust output equals the C# output (pipe and text agree).
    Match,
    /// Rust deliberately differs; see docs/parity.md "Primitive formatting decisions (#99)".
    Retained,
}

fn cases() -> Vec<(&'static str, String, Outcome)> {
    use Outcome::*;
    vec![
        ("bool-direct-true", render(&true), Retained),
        ("bool-direct-false", render(&false), Retained),
        ("bool-generic-true", render(&true), Retained),
        ("bool-generic-false", render(&false), Retained),
        ("double-nan", render(&f64::NAN), Match),
        ("double-positive-infinity", render(&f64::INFINITY), Retained),
        (
            "double-negative-infinity",
            render(&f64::NEG_INFINITY),
            Retained,
        ),
        ("double-negative-zero", render(&-0.0f64), Match),
        ("double-small-exponent", render(&1e-30f64), Retained),
        ("double-large-exponent", render(&1e30f64), Retained),
        ("single-nan", render(&f32::NAN), Match),
        ("single-positive-infinity", render(&f32::INFINITY), Retained),
        (
            "single-negative-infinity",
            render(&f32::NEG_INFINITY),
            Retained,
        ),
        ("single-negative-zero", render(&-0.0f32), Match),
        ("character-single-quote", render(&'\''), Retained),
        ("character-double-quote", render(&'"'), Match),
        ("character-unicode", render(&'雪'), Retained),
        ("rune-supplementary", render(&'🦀'), Retained),
    ]
}

fn csharp_fixture() -> Vec<(String, String)> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/parity/fixtures/primitive-outcomes.txt"
    );
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| {
            let (name, rest) = line.split_once(": pipe=[").unwrap();
            let (pipe, rest) = rest.split_once("],text=[").unwrap();
            let text = rest.strip_suffix(']').unwrap();
            // The shared PipeWriter output is empty only for the lost direct-bool path.
            let value = if pipe.is_empty() { text } else { pipe };
            assert!(pipe.is_empty() || pipe == text, "{name}: pipe/text differ");
            (name.to_owned(), value.to_owned())
        })
        .collect()
}

#[test]
fn every_csharp_case_has_an_explicit_outcome() {
    let reference = csharp_fixture();
    assert_eq!(reference.len(), 18);
    let rust = cases();
    let names: Vec<_> = reference.iter().map(|(n, _)| n.as_str()).collect();
    let rust_names: Vec<_> = rust.iter().map(|(n, _, _)| *n).collect();
    assert_eq!(names, rust_names);
}

#[test]
fn matched_cases_equal_csharp_and_retained_cases_differ() {
    for ((name, csharp), (_, actual, outcome)) in csharp_fixture().into_iter().zip(cases()) {
        match outcome {
            Outcome::Match => assert_eq!(actual, csharp, "{name}"),
            Outcome::Retained => assert_ne!(actual, csharp, "{name}"),
        }
    }
}

#[test]
fn retained_rust_representations_are_pinned() {
    let pinned = [
        ("bool-direct-true", "true"),
        ("bool-direct-false", "false"),
        ("bool-generic-true", "true"),
        ("bool-generic-false", "false"),
        ("double-positive-infinity", "inf"),
        ("double-negative-infinity", "-inf"),
        ("single-positive-infinity", "inf"),
        ("single-negative-infinity", "-inf"),
        ("character-single-quote", "&#39;"),
        ("character-unicode", "雪"),
        ("rune-supplementary", "🦀"),
        ("double-small-exponent", "0.000000000000000000000000000001"),
        ("double-large-exponent", "1000000000000000000000000000000"),
    ];
    let rust = cases();
    for (name, expected) in pinned {
        let actual = &rust.iter().find(|(n, _, _)| *n == name).unwrap().1;
        assert_eq!(actual, expected, "{name}");
    }
}
