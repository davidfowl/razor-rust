use razor_slices::{
    declare_layout, html, partial, section, with_layout, with_section, InlineSlice, Layout, Slice,
};
use std::{cell::Cell, fmt, io};

struct Inner;

impl Layout for Inner {
    fn render_layout(&self, body: &dyn Slice, w: &mut dyn io::Write) -> io::Result<()> {
        w.write_all(b"<inner>")?;
        body.render_to_writer(w)?;
        w.write_all(b"</inner>")
    }

    fn render_layout_section(
        &self,
        _: &dyn Slice,
        name: &str,
        w: &mut dyn io::Write,
    ) -> io::Result<()> {
        if name.is_empty() {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        if name == "footer" {
            w.write_all(b"inner-footer")?;
        }
        Ok(())
    }
}

struct Outer;

impl Layout for Outer {
    fn render_layout(&self, body: &dyn Slice, w: &mut dyn io::Write) -> io::Result<()> {
        body.render_to_writer(w)?;
        html! { "<footer>" {section(body, "footer")} "</footer>" }.render_to_writer(w)
    }
}

#[test]
fn outer_layout_renders_inner_layouts_own_section_after_body() {
    let page = declare_layout(html! { "page" }, declare_layout(Inner, Outer));
    assert_eq!(
        page.render_to_string().unwrap(),
        "<inner>page</inner><footer>inner-footer</footer>"
    );
}

#[derive(Default)]
struct Counts {
    header: Cell<usize>,
    footer: Cell<usize>,
    banner: Cell<usize>,
}

struct ContextInner<'a>(&'a Counts);

impl Layout for ContextInner<'_> {
    fn render_layout(&self, body: &dyn Slice, w: &mut dyn io::Write) -> io::Result<()> {
        html! { "<inner>" {section(body, "header")} ":" {partial(body)} "</inner>" }
            .render_to_writer(w)
    }

    fn render_layout_section(
        &self,
        body: &dyn Slice,
        name: &str,
        w: &mut dyn io::Write,
    ) -> io::Result<()> {
        if name.is_empty() {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        if name == "footer" {
            self.0.footer.set(self.0.footer.get() + 1);
            html! { "inner-footer[" {section(body, "header")} "]" }.render_to_writer(w)?;
        }
        Ok(())
    }
}

struct ContextOuter<'a>(&'a Counts);

impl Layout for ContextOuter<'_> {
    fn render_layout(&self, body: &dyn Slice, w: &mut dyn io::Write) -> io::Result<()> {
        html! {
            "<outer>" {partial(body)} "|" {section(body, "footer")}
            "|" {section(body, "footer")} {section(body, "page-only")}
            {section(body, "Footer")} {section(body, " ")} "</outer>"
        }
        .render_to_writer(w)
    }

    fn render_layout_section(
        &self,
        body: &dyn Slice,
        name: &str,
        w: &mut dyn io::Write,
    ) -> io::Result<()> {
        if name.is_empty() {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        if name == "banner" {
            self.0.banner.set(self.0.banner.get() + 1);
            html! { "outer-banner{" {section(body, "footer")} "}" }.render_to_writer(w)?;
        }
        Ok(())
    }
}

struct Top;

impl Layout for Top {
    fn render_layout(&self, body: &dyn Slice, w: &mut dyn io::Write) -> io::Result<()> {
        html! { "<top>" {partial(body)} "|" {section(body, "banner")} "</top>" }.render_to_writer(w)
    }
}

fn context_page(counts: &Counts) -> impl Slice + '_ {
    let header = InlineSlice::new(move |w: &mut dyn io::Write| {
        counts.header.set(counts.header.get() + 1);
        w.write_all(b"page-header")
    });
    let body = with_section(html! { "page" }, "header", header).unwrap();
    let body = with_section(body, "page-only", html! { "must-not-inherit" }).unwrap();
    let inner: Box<dyn Layout + '_> = Box::new(ContextInner(counts));
    let outer: Box<dyn Layout + '_> = Box::new(ContextOuter(counts));
    declare_layout(body, declare_layout(inner, declare_layout(outer, Top)))
}

const GOLDEN: &str = include_str!("fixtures/nested-layout-sections.txt");

#[test]
fn pinned_csharp_golden_matches_immediate_context_repetition_and_partial_bypass() {
    let counts = Counts::default();
    let page: Box<dyn Slice + '_> = Box::new(context_page(&counts));
    let borrowed: &dyn Slice = &page;
    let document = borrowed.render_to_string().unwrap();
    let bare = html! { {partial(borrowed)} }.render_to_string().unwrap();
    let mut output = format!(
        "document: {document}\npartial: {bare}\nsection-renders: header={},footer={},banner={}\n",
        counts.header.get(),
        counts.footer.get(),
        counts.banner.get()
    );
    assert_eq!(
        Top.render_layout_section(borrowed, "", &mut Vec::new())
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
    output.push_str("empty-name: rejected\n");
    assert_eq!(output, GOLDEN);

    let expected = document.as_bytes();
    for _ in 0..2 {
        assert_eq!(borrowed.render_to_bytes().unwrap(), expected);
        let mut text = String::new();
        borrowed.render(&mut text).unwrap();
        assert_eq!(text, document);
        let mut bytes = Vec::new();
        borrowed.render_to_writer(&mut bytes).unwrap();
        assert_eq!(bytes, expected);
    }
    assert_eq!(counts.header.get(), 28);
    assert_eq!(counts.footer.get(), 21);
    assert_eq!(counts.banner.get(), 7);
}

#[test]
fn section_associations_preserve_layout_contract_shadowing_and_declared_content_bypass() {
    let shadowed = InlineSlice::new(|_: &mut dyn io::Write| -> io::Result<()> {
        panic!("shadowed section must remain lazy")
    });
    let inner = with_section(Inner, "footer", shadowed).unwrap();
    let footer = declare_layout(html! { "<b>" {"<&>"} "</b>" }, Top);
    let inner = with_section(inner, "footer", footer).unwrap();
    let layout: Box<dyn Layout> = Box::new(declare_layout(inner, Outer));
    let borrowed: &dyn Layout = &layout;
    let page = declare_layout(html! { "page" }, borrowed);
    assert_eq!(
        page.render_to_string().unwrap(),
        "<inner>page</inner><footer><b>&lt;&amp;&gt;</b></footer>"
    );
    // Explicit documents used as section content remain composed.
    let explicit = with_layout(html! { "explicit" }, Inner);
    let layout = declare_layout(with_section(Inner, "footer", explicit).unwrap(), Outer);
    let page = with_section(
        declare_layout(html! { "page" }, layout),
        "footer",
        html! { "consumer" },
    )
    .unwrap();
    assert_eq!(
        page.render_to_string().unwrap(),
        "<inner>page</inner><footer><inner>explicit</inner></footer>"
    );
    assert_eq!(
        html! { {section(&page, "footer")} {partial(&page)} }
            .render_to_string()
            .unwrap(),
        "consumerpage"
    );
}

#[test]
fn every_layout_section_dispatch_rejects_empty_names_and_keeps_unknown_names_optional() {
    fn check(layout: &dyn Layout) {
        let body = html! { "must-not-render" };
        let mut output = Vec::new();
        for name in ["missing", "Footer", " "] {
            layout
                .render_layout_section(&body, name, &mut output)
                .unwrap();
        }
        assert!(output.is_empty());
        assert_eq!(
            layout
                .render_layout_section(&body, "", &mut output)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(output.is_empty());
    }
    check(&Top);
    let associated = with_section(Top, "footer", html! { "footer" }).unwrap();
    check(&associated);
    let declared = declare_layout(associated, Outer);
    let boxed: Box<dyn Layout> = Box::new(declared);
    check(&&boxed);
}

#[derive(Debug)]
struct DestinationFailure;

impl fmt::Display for DestinationFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("original nested section destination failure")
    }
}

impl std::error::Error for DestinationFailure {}

#[test]
fn failures_at_every_document_byte_preserve_source_prefix_and_stop_without_flush() {
    struct ShortWriter {
        bytes: Vec<u8>,
        limit: usize,
        interrupted: bool,
    }
    impl io::Write for ShortWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if !self.interrupted {
                self.interrupted = true;
                return Err(io::ErrorKind::Interrupted.into());
            }
            if self.bytes.len() == self.limit {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    DestinationFailure,
                ));
            }
            self.bytes.push(bytes[0]);
            Ok(1)
        }
        fn flush(&mut self) -> io::Result<()> {
            panic!("synchronous layouts must not flush")
        }
    }
    let expected = GOLDEN
        .lines()
        .next()
        .unwrap()
        .strip_prefix("document: ")
        .unwrap();
    for limit in 0..=expected.len() {
        let counts = Counts::default();
        let page = context_page(&counts);
        let mut writer = ShortWriter {
            bytes: Vec::new(),
            limit,
            interrupted: false,
        };
        let result = page.render_to_writer(&mut writer);
        if limit == expected.len() {
            result.unwrap();
        } else {
            let error = result.unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
            assert!(error.get_ref().unwrap().is::<DestinationFailure>());
        }
        assert_eq!(writer.bytes, &expected.as_bytes()[..limit]);
        if limit < "<top><outer><inner>".len() {
            assert_eq!(counts.header.get(), 0);
            assert_eq!(counts.footer.get(), 0);
            assert_eq!(counts.banner.get(), 0);
        }
    }
}

#[test]
fn section_provider_error_after_body_keeps_source_and_skips_outer_suffix() {
    let footer = InlineSlice::new(|w: &mut dyn io::Write| {
        w.write_all(b"accepted-section-prefix")?;
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            DestinationFailure,
        ))
    });
    let layout = declare_layout(with_section(Inner, "footer", footer).unwrap(), Outer);
    let page = declare_layout(html! { "page" }, layout);
    let mut bytes = Vec::new();
    let error = page.render_to_writer(&mut bytes).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(error.get_ref().unwrap().is::<DestinationFailure>());
    assert_eq!(bytes, b"<inner>page</inner><footer>accepted-section-prefix");
}

#[test]
fn shared_layout_keeps_each_pages_original_context_across_alternating_renders() {
    let counts = Counts::default();
    let layout = declare_layout(ContextInner(&counts), Outer);
    let one = with_section(html! { "one" }, "header", html! { "A" }).unwrap();
    let two = with_section(html! { "two" }, "header", html! { "B" }).unwrap();
    let one = declare_layout(one, &layout);
    let two = declare_layout(two, &layout);
    for _ in 0..2 {
        assert_eq!(
            one.render_to_string().unwrap(),
            "<inner>A:one</inner><footer>inner-footer[A]</footer>"
        );
        assert_eq!(
            two.render_to_string().unwrap(),
            "<inner>B:two</inner><footer>inner-footer[B]</footer>"
        );
    }
}

#[test]
fn section_only_layout_does_not_eagerly_execute_body_or_lose_child_context() {
    struct OnlyFooter;
    impl Layout for OnlyFooter {
        fn render_layout(&self, body: &dyn Slice, w: &mut dyn io::Write) -> io::Result<()> {
            html! { {section(body, "footer")} }.render_to_writer(w)
        }
    }
    let counts = Counts::default();
    let body = InlineSlice::new(|_: &mut dyn io::Write| -> io::Result<()> {
        panic!("body must not execute")
    });
    let body = with_section(body, "header", html! { "page-header" }).unwrap();
    let page = declare_layout(body, declare_layout(ContextInner(&counts), OnlyFooter));
    assert_eq!(counts.footer.get(), 0);
    assert_eq!(
        page.render_to_string().unwrap(),
        "inner-footer[page-header]"
    );
    assert_eq!(counts.footer.get(), 1);

    struct BrokenText;
    impl fmt::Write for BrokenText {
        fn write_str(&mut self, _: &str) -> fmt::Result {
            Err(fmt::Error)
        }
    }
    let counts = Counts::default();
    let page = context_page(&counts);
    assert!(page.render(&mut BrokenText).is_err());
    assert_eq!(counts.header.get(), 0);
    assert_eq!(counts.footer.get(), 0);
    assert_eq!(counts.banner.get(), 0);
}
