use razor_slices::{
    render_incremental, AsyncHtmlWriter, AsyncRenderError, AsyncSlice, AsyncSliceWriter,
    TokioWriter,
};
use tokio::{io::AsyncReadExt, sync::Notify};

struct Page<'a> {
    later_work: &'a Notify,
    name: &'a str,
}

impl AsyncSlice for Page<'_> {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        output.write_literal("<p>Loading...</p>\n").await?;
        output.flush().await?;
        self.later_work.notified().await;
        output.write_literal("<p>").await?;
        output.write_value(self.name).await?;
        output.write_literal("</p>\n").await?;
        Ok(())
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let later_work = Notify::new();
    let page = Page {
        later_work: &later_work,
        name: "<雪 & ready>",
    };
    let (destination, mut reader) = tokio::io::duplex(16);
    let producer = async {
        let mut writer = TokioWriter::new(destination);
        render_incremental(&page, &mut writer).await
    };
    let consumer = async {
        let mut prefix = [0; 18];
        reader.read_exact(&mut prefix).await?;
        assert_eq!(&prefix, b"<p>Loading...</p>\n");
        println!(
            "Before releasing later await: {}",
            std::str::from_utf8(&prefix)?.trim_end()
        );
        later_work.notify_one();
        let mut suffix = String::new();
        reader.read_to_string(&mut suffix).await?;
        assert_eq!(suffix, "<p>&lt;雪 &amp; ready&gt;</p>\n");
        println!("After releasing later await: {}", suffix.trim_end());
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    let (rendered, observed) = tokio::join!(producer, consumer);
    rendered?;
    observed?;
    Ok(())
}
