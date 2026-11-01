//! Native-Rust equivalents of upstream shareable helpers (#101) and templated
//! methods/delegates (#104). See docs/adr/0006-helpers-methods-delegates.md.

use razor_slices::{async_template, html, partial, render_incremental, AsyncSliceWriter, Slice};
use std::io;

mod shared_helpers {
    //! Stands in for a separate "helpers" crate: only public items are used.
    use razor_slices::{html, Slice};

    pub fn badge(label: &str) -> impl Slice + '_ {
        html! { "<span class=\"badge\">" {label} "</span>" }
    }

    pub fn list<'a, T: 'a, S: Slice + 'a>(
        items: &'a [T],
        item: impl Fn(&'a T) -> S + 'a,
    ) -> impl Slice + 'a {
        html! { "<ul>" for value in items { "<li>" {razor_slices::partial(item(value))} "</li>" } "</ul>" }
    }
}

#[test]
fn shared_helper_function_is_typed_escaped_and_reusable() {
    let page = html! { "<p>" {partial(shared_helpers::badge("<new>"))} "</p>" };
    for _ in 0..2 {
        assert_eq!(
            page.render_to_string().unwrap(),
            "<p><span class=\"badge\">&lt;new&gt;</span></p>"
        );
    }
}

/// Analogue of `@implements IFoo`: helpers declared once as trait methods.
trait Helpers {
    fn name(&self) -> &str;
    fn greeting(&self) -> impl Slice + '_ {
        html! { "<b>Hello, " {self.name()} "</b>" }
    }
}

struct User(String);
impl Helpers for User {
    fn name(&self) -> &str {
        &self.0
    }
}

#[test]
fn trait_default_methods_are_shared_helpers() {
    let user = User("<D&D>".into());
    let page = html! { {partial(user.greeting())} };
    assert_eq!(
        page.render_to_string().unwrap(),
        "<b>Hello, &lt;D&amp;D&gt;</b>"
    );
}

struct Todo {
    title: &'static str,
    done: bool,
}

impl Todo {
    fn status(&self) -> impl Slice + '_ {
        html! { if self.done { "<s>" {self.title} "</s>" } else { {self.title} } }
    }
}

#[test]
fn templated_method_with_control_flow() {
    let todos = [
        Todo {
            title: "a&b",
            done: true,
        },
        Todo {
            title: "c",
            done: false,
        },
    ];
    let page = html! { for todo in &todos { {partial(todo.status())} "|" } };
    assert_eq!(page.render_to_string().unwrap(), "<s>a&amp;b</s>|c|");
}

#[test]
fn templated_delegate_closure_passed_to_helper() {
    let todos = [
        Todo {
            title: "x<y",
            done: false,
        },
        Todo {
            title: "z",
            done: true,
        },
    ];
    let page = shared_helpers::list(&todos, |todo| {
        html! { "<em>" {todo.title} "</em>" {todo.done} }
    });
    assert_eq!(
        page.render_to_string().unwrap(),
        "<ul><li><em>x&lt;y</em>false</li><li><em>z</em>true</li></ul>"
    );
}

#[test]
fn closure_delegate_bound_locally_and_called_per_item() {
    let label = |text: &'static str| html! { "<i>" {text} "</i>" };
    let page = html! { {partial(label("a"))} {partial(label("<b>"))} };
    assert_eq!(page.render_to_string().unwrap(), "<i>a</i><i>&lt;b&gt;</i>");
}

#[test]
fn boxed_dyn_slice_delegate() {
    let delegate: Box<dyn Fn(u32) -> Box<dyn Slice>> = Box::new(|n| Box::new(html! { "#" {n} }));
    let page = html! { {partial(delegate(7))} };
    assert_eq!(page.render_to_string().unwrap(), "#7");
}

#[derive(Default)]
struct Sink(Vec<u8>);

impl AsyncSliceWriter for Sink {
    async fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.0.extend(bytes);
        Ok(())
    }

    async fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

async_template! {
    struct AsyncBadge<'a> { model: &'a str } => { "<b>" {model} "</b>" }
}

async_template! {
    struct AsyncPage<'a> { model: &'a [&'a str] } => {
        for label in model.iter().copied() {
            @render { AsyncBadge::new(label) };
        }
    }
}

#[tokio::test]
async fn async_template_declarations_are_shareable_helpers() {
    let page = AsyncPage::new(&["<a>", "b"]);
    for _ in 0..2 {
        let mut sink = Sink::default();
        render_incremental(&page, &mut sink).await.unwrap();
        assert_eq!(sink.0, b"<b>&lt;a&gt;</b><b>b</b>");
    }
}
