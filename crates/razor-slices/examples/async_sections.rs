use razor_slices::{
    render_incremental, with_async_section, AsyncHtmlWriter, AsyncRenderError, AsyncSlice,
    AsyncSliceWriter, TokioWriter,
};
use tokio::{io::AsyncReadExt, sync::Notify};

const PREFIX: &str = "<page><header>Loading...</header>\n";
const SUFFIX: &str = "<h1>&lt;雪 &amp; ready&gt;</h1>\n<main>Body</main>\n</page>\n";

struct Header<'a> {
    later_work: &'a Notify,
    title: &'a str,
}

impl AsyncSlice for Header<'_> {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        output
            .write_literal("<header>Loading...</header>\n")
            .await?;
        output.flush().await?;
        self.later_work.notified().await;
        output.write_literal("<h1>").await?;
        output.write_value(self.title).await?;
        output.write_literal("</h1>\n").await?;
        Ok(())
    }
}

struct Body;

impl AsyncSlice for Body {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        output.write_literal("<main>Body</main>\n").await?;
        Ok(())
    }
}

struct Page<S>(S);

impl<S: AsyncSlice> AsyncSlice for Page<S> {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        output.write_literal("<page>").await?;
        output.write_section(&self.0, "header").await?;
        self.0.render_into(output).await?;
        output.write_section(&self.0, "optional-scripts").await?;
        output.write_literal("</page>\n").await?;
        Ok(())
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let later_work = Notify::new();
    let page = Page(with_async_section(
        Body,
        "header",
        Header {
            later_work: &later_work,
            title: "<雪 & ready>",
        },
    )?);
    let (destination, mut reader) = tokio::io::duplex(1);
    let producer = async {
        let mut writer = TokioWriter::new(destination);
        render_incremental(&page, &mut writer).await
    };
    let consumer = async {
        let mut prefix = vec![0; PREFIX.len()];
        reader.read_exact(&mut prefix).await?;
        assert_eq!(prefix, PREFIX.as_bytes());
        println!("Before releasing section await: {}", PREFIX.trim_end());
        later_work.notify_one();
        let mut suffix = String::new();
        reader.read_to_string(&mut suffix).await?;
        assert_eq!(suffix, SUFFIX);
        println!("After releasing section await:\n{}", suffix.trim_end());
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    let (rendered, observed) = tokio::join!(producer, consumer);
    rendered?;
    observed?;
    Ok(())
}
