#[cfg(test)]
mod tests {
    use slices::{html, Slice};

    #[test]
    fn macro_resolves_renamed_runtime() {
        let value = String::from("<David>");
        let template = html! { "<p>" {value} "</p>" };
        assert_eq!(template.render_to_string().unwrap(), "<p>&lt;David&gt;</p>");
    }
}
