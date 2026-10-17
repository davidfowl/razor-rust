use razor_slices::{
    async_template, declare_async_layout, render_incremental, AsyncHtmlWriter, AsyncLayout,
    AsyncRenderError, AsyncSlice, AsyncSliceWriter, InlineSlice, Slice,
};
use std::{
    fmt, io,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

struct Guard(Arc<AtomicUsize>);

impl Drop for Guard {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Default)]
struct FlakyText {
    fail: bool,
    text: String,
}

impl fmt::Write for FlakyText {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        if !text.is_empty() && std::mem::take(&mut self.fail) {
            return Err(fmt::Error);
        }
        self.text.push_str(text);
        Ok(())
    }
}

#[test]
fn failed_completed_utf8_does_not_poison_later_writes_or_renders() {
    for text in ["\u{1f980}", "\u{96ea}", "\u{e9}"] {
        for split in 1..text.len() {
            let page = InlineSlice::new(|writer: &mut dyn io::Write| {
                writer.write_all(&text.as_bytes()[..split])?;
                match writer.write_all(&text.as_bytes()[split..]) {
                    Ok(()) => writer.write_all(b"/end"),
                    Err(original) => {
                        assert_eq!(original.kind(), io::ErrorKind::Other);
                        assert!(original
                            .get_ref()
                            .unwrap()
                            .downcast_ref::<fmt::Error>()
                            .is_some());
                        writer.write_all(b"/cleanup").unwrap();
                        Err(original)
                    }
                }
            });
            let mut writer = FlakyText {
                fail: true,
                ..Default::default()
            };
            assert!(page.render(&mut writer).is_err());
            assert_eq!(writer.text, "/cleanup");
            for _ in 0..2 {
                assert_eq!(page.render_to_string().unwrap(), format!("{text}/end"));
            }
        }
    }
}

#[test]
fn invalid_pending_utf8_does_not_poison_explicit_cleanup_writes() {
    for prefix in [&b"\xf0\x9f\xa6"[..], &b"\xe9\x9b"[..], &b"\xc3"[..]] {
        let page = InlineSlice::new(|writer: &mut dyn io::Write| {
            writer.write_all(prefix)?;
            let original = writer.write_all(b"\xff").unwrap_err();
            assert_eq!(original.kind(), io::ErrorKind::InvalidData);
            assert!(original
                .get_ref()
                .unwrap()
                .downcast_ref::<std::str::Utf8Error>()
                .is_some());
            writer.write_all(b"/cleanup").unwrap();
            Err(original)
        });
        for _ in 0..2 {
            let mut text = String::new();
            assert!(page.render(&mut text).is_err());
            assert_eq!(text, "/cleanup");
        }
    }
}

#[derive(Debug)]
struct Original(Arc<()>);

impl fmt::Display for Original {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("lifecycle destination failure")
    }
}

impl std::error::Error for Original {}

struct Destination {
    bytes: Vec<u8>,
    writes: usize,
    fail_write: Option<usize>,
    flushes: usize,
    fail_flush: bool,
    source: Arc<()>,
    drops: Arc<AtomicUsize>,
}

impl Destination {
    fn failing_write(call: usize) -> Self {
        Self {
            bytes: Vec::new(),
            writes: 0,
            fail_write: Some(call),
            flushes: 0,
            fail_flush: false,
            source: Arc::new(()),
            drops: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn failure(&self) -> io::Error {
        io::Error::new(io::ErrorKind::BrokenPipe, Original(self.source.clone()))
    }

    fn assert_original(&self, error: &io::Error) {
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
        let original = error.get_ref().unwrap().downcast_ref::<Original>().unwrap();
        assert!(Arc::ptr_eq(&original.0, &self.source));
    }
}

impl Drop for Destination {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

impl io::Write for Destination {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.writes += 1;
        if self.fail_write == Some(self.writes) {
            return Err(self.failure());
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flushes += 1;
        if self.fail_flush {
            return Err(self.failure());
        }
        Ok(())
    }
}

impl AsyncSliceWriter for Destination {
    async fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        io::Write::write_all(self, bytes)
    }

    async fn flush(&mut self) -> io::Result<()> {
        io::Write::flush(self)
    }
}

#[test]
fn sync_render_locals_drop_on_errors_but_captures_and_destination_remain_owned() {
    let captured_drops = Arc::new(AtomicUsize::new(0));
    let local_drops = Arc::new(AtomicUsize::new(0));
    let captured = Guard(captured_drops.clone());
    let locals = local_drops.clone();
    let page = InlineSlice::new(move |writer: &mut dyn io::Write| {
        let _capture = &captured;
        let _local = Guard(locals.clone());
        writer.write_all(b"body")
    });
    assert_eq!(local_drops.load(Ordering::SeqCst), 0);
    assert!(page
        .render(&mut FlakyText {
            fail: true,
            ..Default::default()
        })
        .is_err());
    let mut writer = Destination::failing_write(1);
    let error = page.render_to_writer(&mut writer).unwrap_err();
    writer.assert_original(&error);
    assert_eq!(local_drops.load(Ordering::SeqCst), 2);
    assert_eq!(captured_drops.load(Ordering::SeqCst), 0);
    assert_eq!(writer.flushes, 0);
    assert_eq!(writer.drops.load(Ordering::SeqCst), 0);
    for _ in 0..2 {
        assert_eq!(page.render_to_string().unwrap(), "body");
    }
    assert_eq!(local_drops.load(Ordering::SeqCst), 4);
    drop(page);
    assert_eq!(captured_drops.load(Ordering::SeqCst), 1);
    let drops = writer.drops.clone();
    drop(writer);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[derive(Default)]
struct Effects {
    starts: AtomicUsize,
    finishes: AtomicUsize,
    locals: Arc<AtomicUsize>,
}

struct Model {
    _owned: Guard,
    effects: Arc<Effects>,
}

impl Model {
    fn local(&self) -> Guard {
        self.effects.starts.fetch_add(1, Ordering::SeqCst);
        Guard(self.effects.locals.clone())
    }

    fn finish(&self) {
        self.effects.finishes.fetch_add(1, Ordering::SeqCst);
    }
}

async_template! {
    struct Page { model: Model } => {
        @{ let _local = model.local(); }
        "body" {'&'}
        @{ model.finish() }
    }
}

struct Shell {
    _owned: Guard,
    locals: Arc<AtomicUsize>,
}

impl AsyncLayout for Shell {
    async fn render_layout_into<B: AsyncSlice, W: AsyncSliceWriter + Send>(
        &self,
        body: &B,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        let _local = Guard(self.locals.clone());
        output.write_literal("[").await?;
        body.render_body_into(output).await?;
        output.write_literal("]").await?;
        Ok(())
    }
}

#[tokio::test]
async fn nested_render_resources_drop_after_write_and_flush_errors_without_consuming_models() {
    let body_owned = Arc::new(AtomicUsize::new(0));
    let layout_owned = Arc::new(AtomicUsize::new(0));
    let layout_locals = Arc::new(AtomicUsize::new(0));
    let effects = Arc::new(Effects::default());
    let page = declare_async_layout(
        Page::new(Model {
            _owned: Guard(body_owned.clone()),
            effects: effects.clone(),
        }),
        Shell {
            _owned: Guard(layout_owned.clone()),
            locals: layout_locals.clone(),
        },
    );
    assert_eq!(effects.starts.load(Ordering::SeqCst), 0);
    let mut writer = Destination::failing_write(2);
    let AsyncRenderError::Write(error) = render_incremental(&page, &mut writer).await.unwrap_err()
    else {
        panic!("expected original destination error");
    };
    writer.assert_original(&error);
    assert_eq!(writer.bytes, b"[");
    assert_eq!(writer.writes, 2);
    assert_eq!(writer.flushes, 0);
    assert_eq!(effects.finishes.load(Ordering::SeqCst), 0);
    assert_eq!(effects.locals.load(Ordering::SeqCst), 1);
    assert_eq!(layout_locals.load(Ordering::SeqCst), 1);
    writer.fail_write = None;
    for count in 1..=2 {
        writer.bytes.clear();
        render_incremental(&page, &mut writer).await.unwrap();
        assert_eq!(writer.bytes, b"[body&amp;]");
        assert_eq!(writer.flushes, count);
    }
    writer.bytes.clear();
    writer.fail_flush = true;
    let AsyncRenderError::Write(error) = render_incremental(&page, &mut writer).await.unwrap_err()
    else {
        panic!("expected original destination error");
    };
    writer.assert_original(&error);
    assert_eq!(writer.bytes, b"[body&amp;]");
    assert_eq!(writer.flushes, 3);
    assert_eq!(effects.starts.load(Ordering::SeqCst), 4);
    assert_eq!(effects.finishes.load(Ordering::SeqCst), 3);
    assert_eq!(effects.locals.load(Ordering::SeqCst), 4);
    assert_eq!(layout_locals.load(Ordering::SeqCst), 4);
    assert_eq!(body_owned.load(Ordering::SeqCst), 0);
    assert_eq!(layout_owned.load(Ordering::SeqCst), 0);
    assert_eq!(writer.drops.load(Ordering::SeqCst), 0);
    drop(page);
    assert_eq!(body_owned.load(Ordering::SeqCst), 1);
    assert_eq!(layout_owned.load(Ordering::SeqCst), 1);
    let drops = writer.drops.clone();
    drop(writer);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}
