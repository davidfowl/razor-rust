#[cfg(test)]
mod tests {
    use slices::{html, partial, Slice};

    #[test]
    fn macro_resolves_renamed_runtime() {
        let value = String::from("<David>");
        let template = html! { "<p>" {value} "</p>" };
        assert_eq!(template.render_to_string().unwrap(), "<p>&lt;David&gt;</p>");
        let parent = html! { "<main>" {partial(&template)} "</main>" };
        assert_eq!(
            parent.render_to_string().unwrap(),
            "<main><p>&lt;David&gt;</p></main>"
        );
    }
}
