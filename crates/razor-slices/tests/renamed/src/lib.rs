#[cfg(test)]
mod tests {
    use slices::{html, partial, with_layout, Layout, Slice};
    use std::io;

    struct Shell;

    impl Layout for Shell {
        fn render_layout(&self, body: &dyn Slice, writer: &mut dyn io::Write) -> io::Result<()> {
            html! { "<shell>" {partial(body)} "</shell>" }.render_to_writer(writer)
        }
    }

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
        let document = with_layout(&parent, Shell);
        assert_eq!(
            document.render_to_string().unwrap(),
            "<shell><main><p>&lt;David&gt;</p></main></shell>"
        );
    }
}
