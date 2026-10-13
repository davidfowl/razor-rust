use slices::{html, Slice};

#[test]
fn nested_control_flow_uses_the_renamed_runtime() {
    let values = vec!["<&>", ""];
    let page = html! {
        for (index, value) in values.iter().enumerate() {
            if value.is_empty() { "empty" } else { {index} {value} }
        }
    };
    assert_eq!(page.render_to_string().unwrap(), "0&lt;&amp;&gt;empty");
    assert_eq!(page.render_to_string().unwrap(), "0&lt;&amp;&gt;empty");
}
