use slices::{html, Slice};

#[test]
fn synchronous_blocks_retain_scope_hygiene_and_renamed_runtime_paths() {
    let owned = vec!["&", "<"];
    let page = html! {
        @{ let __razor_writer = "user"; let mut count = 0; }
        for value in &owned {
            @{ count += 1; }
            {value}
        }
        {count} {__razor_writer}
    };
    for _ in 0..2 {
        assert_eq!(page.render_to_string().unwrap(), "&amp;&lt;2user");
    }
}
