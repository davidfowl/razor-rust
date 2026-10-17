use razor_slices::{async_template, AsyncHtmlWriter, AsyncSlice, AsyncSliceWriter};
use std::io;

struct __RazorWriter;

impl __RazorWriter {
    const TEXT: &'static str = "<first>";
}

struct __RazorWriter_;

impl __RazorWriter_ {
    const TEXT: &'static str = "&second";
}

struct r#__RazorWriter__;

impl r#__RazorWriter__ {
    const TEXT: &'static str = ">third";
}

async_template! {
    struct AssociatedNames { model: () } => {
        if true {
            {__RazorWriter::TEXT}
            for value in [__RazorWriter_::TEXT] {
                {value} {r#__RazorWriter__::TEXT}
            }
        }
    }
}

#[derive(Default)]
struct Writer(Vec<u8>);

impl AsyncSliceWriter for Writer {
    async fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.0.extend_from_slice(bytes);
        Ok(())
    }

    async fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn generated_writer_type_does_not_shadow_template_type_paths() {
    let page = AssociatedNames::new(());
    for _ in 0..2 {
        let mut writer = Writer::default();
        page.render_into(&mut AsyncHtmlWriter::new(&mut writer))
            .await
            .unwrap();
        assert_eq!(writer.0, b"&lt;first&gt;&amp;second&gt;third");
    }
}
