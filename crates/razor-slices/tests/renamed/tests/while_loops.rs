use slices::{async_template, html, Slice};
use std::cell::Cell;

#[test]
fn while_binding_codegen_retains_hygiene_with_a_renamed_runtime() {
    let index = Cell::new(0);
    let index = &index;
    let next = || {
        let value = ["&", "<"].get(index.get()).copied();
        index.set(index.get() + 1);
        value
    };
    let page = html! {
        while let Some(__razor_writer) = next() { {__razor_writer} }
    };
    for _ in 0..2 {
        index.set(0);
        assert_eq!(page.render_to_string().unwrap(), "&amp;&lt;");
    }
}

async_template! {
    struct AsyncWhile { model: usize } => {
        @{ let mut next = Some(*model); }
        while let Some(__razor_writer) = next {
            {__razor_writer}
            @{ next = None; }
        }
    }
}

#[test]
fn typed_async_while_declaration_uses_the_renamed_runtime() {
    let _ = AsyncWhile::new(1);
}
