#[path = "../examples/support/control_flow.rs"]
mod sample;

use razor_slices::{html, partial, HtmlString, Slice, WriteHtmlValue};
use std::{cell::Cell, fmt, io};

#[test]
fn nested_model_lists_match_fixture_and_repeat_on_all_sinks() {
    let groups = sample::sample_groups();
    let page = sample::grouped_tasks(&groups);
    let expected = include_str!("fixtures/control_flow.html");
    for _ in 0..2 {
        assert_eq!(page.render_to_string().unwrap(), expected);
        assert_eq!(page.render_to_bytes().unwrap(), expected.as_bytes());
        let mut bytes = Vec::new();
        page.render_to_writer(&mut bytes).unwrap();
        assert_eq!(bytes, expected.as_bytes());
    }
    assert_eq!(
        sample::grouped_tasks(&[]).render_to_string().unwrap(),
        "<main>\n<footer>Reusable control flow.</footer>\n</main>\n"
    );
}

#[test]
fn conditions_evaluate_at_render_time_and_skip_unselected_bodies() {
    let state = Cell::new(0);
    let state = &state;
    let unreachable = || -> &str { panic!("unselected branch evaluated") };
    let page = html! {
        if state.get() == 0 { "zero" }
        else if state.get() == 1 { "one" }
        else { "many" }
        if false { {unreachable()} }
        for _ in std::iter::empty::<()>() { {unreachable()} }
    };
    for (value, expected) in [(0, "zero"), (1, "one"), (2, "many"), (0, "zero")] {
        state.set(value);
        assert_eq!(page.render_to_string().unwrap(), expected);
    }
}

#[test]
fn patterns_hygiene_owned_capture_raw_and_partials() {
    let pairs = vec![("<&>", true), ("ignored", false)];
    let __razor_writer = "user";
    let child = html! { "<b>" {"'"} "</b>" };
    let page = html! {
        for (__razor_writer, selected) in &pairs {
            if *selected {
                {__razor_writer} {HtmlString::from("<hr>")} {partial(&child)}
                for (index, value) in ["&", ">"].iter().enumerate() {
                    {index} {value}
                }
            }
        }
        {__razor_writer}
    };
    for _ in 0..2 {
        assert_eq!(
            page.render_to_string().unwrap(),
            "&lt;&amp;&gt;<hr><b>&#39;</b>0&amp;1&gt;user"
        );
    }
}

#[derive(Debug)]
struct Marker;
impl fmt::Display for Marker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("original failure")
    }
}
impl std::error::Error for Marker {}

struct Failure;
impl WriteHtmlValue for Failure {
    fn write_html_to(&self, _: &mut dyn io::Write) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::PermissionDenied, Marker))
    }
}

#[test]
fn nested_value_error_stops_later_iterations_and_preserves_source() {
    let visits = Cell::new(0);
    let visits = &visits;
    let page = html! {
        for index in 0..3 {
            {{ visits.set(visits.get() + 1); "" }}
            if index == 1 { {Failure} } else { {index} }
        }
        {{ visits.set(99); "suffix" }}
    };
    let mut bytes = Vec::new();
    let error = page.render_to_writer(&mut bytes).unwrap_err();
    assert_eq!(bytes, b"0");
    assert_eq!(visits.get(), 2);
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(error.get_ref().unwrap().downcast_ref::<Marker>().is_some());
    assert!(page.render_to_string().is_err());
    assert_eq!(visits.get(), 4);
}

struct BrokenSink {
    bytes: Vec<u8>,
    remaining: usize,
}
impl io::Write for BrokenSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, Marker));
        }
        let count = self.remaining.min(bytes.len()).min(1);
        self.bytes.extend_from_slice(&bytes[..count]);
        self.remaining -= count;
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        panic!("must not flush");
    }
}
struct BrokenText;
impl fmt::Write for BrokenText {
    fn write_str(&mut self, _: &str) -> fmt::Result {
        Err(fmt::Error)
    }
}

#[test]
fn nested_sink_error_stops_output_and_keeps_first_error() {
    let calls = Cell::new(0);
    let calls = &calls;
    let page = html! {
        for _ in 0..3 {
            if true { "<p>" {"&"} "</p>" }
            {{ calls.set(calls.get() + 1); "" }}
        }
    };
    for remaining in [0, 1, 4, 8] {
        let mut sink = BrokenSink {
            bytes: Vec::new(),
            remaining,
        };
        let error = page.render_to_writer(&mut sink).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
        assert!(error.get_ref().unwrap().downcast_ref::<Marker>().is_some());
        assert_eq!(sink.bytes, b"<p>&amp;</p>"[..remaining]);
        assert_eq!(calls.get(), 0);
    }
    assert!(page.render(&mut BrokenText).is_err());
    assert_eq!(calls.get(), 0);
    let mut sink = BrokenSink {
        bytes: Vec::new(),
        remaining: 100,
    };
    page.render_to_writer(&mut sink).unwrap();
    assert_eq!(sink.bytes, b"<p>&amp;</p><p>&amp;</p><p>&amp;</p>");
    assert_eq!(calls.get(), 3);
}
