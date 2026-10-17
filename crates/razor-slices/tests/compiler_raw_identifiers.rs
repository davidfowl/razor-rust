use razor_slices::{async_template, AsyncSlice};

async_template! {
    struct Owned { r#model: String } => {
        {model} {r#model}
    }
}

async_template! {
    struct Borrowed<'a> { r#model: &'a str } => {
        for value in [model, r#model] {
            {value}
        }
    }
}

fn assert_async_slice(_: &impl AsyncSlice) {}

#[test]
fn raw_model_fields_preserve_typed_constructors_and_render_bindings() {
    let owned = Owned::new(String::from("owned"));
    assert_async_slice(&owned);
    assert_eq!(owned.model, "owned");
    assert_eq!(owned.r#model, "owned");

    let borrowed = Borrowed::new(&owned.model);
    assert_async_slice(&borrowed);
    assert_eq!(borrowed.model, "owned");
    assert_eq!(borrowed.r#model, "owned");
}
