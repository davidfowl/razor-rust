use razor_slices::{
    declare_async_layout, render_incremental_with_context, AsyncHtmlWriter, AsyncLayout,
    AsyncRenderError, AsyncSlice, AsyncSliceWriter, CancellationToken, RenderCancelled,
    RenderContext,
};
use std::{
    future::{pending, Future},
    io,
    sync::{Arc, Mutex},
    task::{Context, Poll, Wake, Waker},
};

const DOCUMENT: &str = "OIHhPbi|FHhfooto";
const SECTION_PREFIX: &str = "OIHhPbi|FHh";
const FLUSH_PREFIX: &str = "OIHhPbi|FHhfoot";
const WRITE_PREFIX: &str = "OIHhPbi|FHhf";
const SEAMS: &[&str] = &[
    "outer-body",
    "inner-body",
    "page-header",
    "header-child",
    "page-body",
    "bare-child",
    "inner-footer",
    "page-header",
    "header-child",
    "footer-child",
    "inner-missing",
];

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Ready,
    Section,
    Flush,
    WriteError,
}

struct State {
    label: String,
    mode: Mode,
    bytes: Vec<u8>,
    visits: Vec<(&'static str, CancellationToken)>,
    inputs: Vec<Vec<u8>>,
    flushes: usize,
    operation_drops: usize,
    owners: [usize; 3],
    traps: usize,
    stall_at: Option<usize>,
    original: Arc<()>,
}

impl State {
    fn new(label: &str, mode: Mode) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self {
            label: label.to_owned(),
            mode,
            bytes: Vec::new(),
            visits: Vec::new(),
            inputs: Vec::new(),
            flushes: 0,
            operation_drops: 0,
            owners: [0; 3],
            traps: 0,
            stall_at: None,
            original: Arc::new(()),
        }))
    }
}

struct Request<'a> {
    label: &'a str,
    state: Arc<Mutex<State>>,
}
type Scope<'a, 'r> = &'a RenderContext<Request<'r>>;

fn observe<W: AsyncSliceWriter + Send>(
    scope: Scope<'_, '_>,
    output: &AsyncHtmlWriter<'_, W>,
    seam: &'static str,
) {
    let actual = output.cancellation_token().expect("missing scoped token");
    assert!(std::ptr::eq(actual, scope.cancellation_token()), "{seam}");
    let mut state = scope.request().state.lock().unwrap();
    assert_eq!(scope.request().label, state.label, "{seam}");
    state.visits.push((seam, actual.clone()));
}

struct Owner(Arc<Mutex<State>>, usize);
impl Drop for Owner {
    fn drop(&mut self) {
        self.0.lock().unwrap().owners[self.1] += 1;
    }
}

struct Operation(Arc<Mutex<State>>);
impl Drop for Operation {
    fn drop(&mut self) {
        self.0.lock().unwrap().operation_drops += 1;
    }
}

struct Page<'a, 'r> {
    scope: Scope<'a, 'r>,
    _owner: Owner,
}
impl AsyncSlice for Page<'_, '_> {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        self.render_body_into(output).await
    }

    async fn render_body_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        observe(self.scope, output, "page-body");
        output.write_literal("P").await?;
        let child = declare_async_layout(Leaf(self.scope, "bare-child", "b"), Trap(self.scope));
        output.write_slice(&Box::new(&child)).await
    }

    async fn render_section_into<W: AsyncSliceWriter + Send>(
        &self,
        name: &str,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        match name {
            "header" => {
                observe(self.scope, output, "page-header");
                output.write_literal("H").await?;
                output
                    .write_slice(&Leaf(self.scope, "header-child", "h"))
                    .await?;
            }
            "page-only" => {
                observe(self.scope, output, "wrong-fallback");
                output.write_literal("BAD").await?;
            }
            _ => {}
        }
        Ok(())
    }
}

struct Leaf<'a, 'r>(Scope<'a, 'r>, &'static str, &'static str);
impl AsyncSlice for Leaf<'_, '_> {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        observe(self.0, output, self.1);
        output.write_literal(self.2).await?;
        Ok(())
    }
}

struct Trap<'a, 'r>(Scope<'a, 'r>);
impl AsyncLayout for Trap<'_, '_> {
    async fn render_layout_into<B: AsyncSlice, W: AsyncSliceWriter + Send>(
        &self,
        _body: &B,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        self.0.request().state.lock().unwrap().traps += 1;
        output.write_literal("TRAP").await?;
        Ok(())
    }
}

struct Inner<'a, 'r> {
    scope: Scope<'a, 'r>,
    _owner: Owner,
}
impl AsyncLayout for Inner<'_, '_> {
    async fn render_layout_into<B: AsyncSlice, W: AsyncSliceWriter + Send>(
        &self,
        body: &B,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        observe(self.scope, output, "inner-body");
        output.write_literal("I").await?;
        output.write_section(body, "header").await?;
        body.render_body_into(output).await?;
        output.write_literal("i").await?;
        Ok(())
    }

    async fn render_layout_section_into<B: AsyncSlice, W: AsyncSliceWriter + Send>(
        &self,
        body: &B,
        name: &str,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        if name == "footer" {
            observe(self.scope, output, "inner-footer");
            output.write_literal("F").await?;
            output.write_section(body, "header").await?;
            let held = self.scope.request().state.lock().unwrap().mode == Mode::Section;
            if held {
                let _operation = Operation(self.scope.request().state.clone());
                pending::<()>().await;
            }
            output
                .write_slice(&Leaf(self.scope, "footer-child", "foot"))
                .await?;
            output.flush().await?;
        } else {
            observe(self.scope, output, "inner-missing");
        }
        Ok(())
    }
}

struct Outer<'a, 'r> {
    scope: Scope<'a, 'r>,
    _owner: Owner,
}
impl AsyncLayout for Outer<'_, '_> {
    async fn render_layout_into<B: AsyncSlice, W: AsyncSliceWriter + Send>(
        &self,
        body: &B,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        observe(self.scope, output, "outer-body");
        output.write_literal("O").await?;
        body.render_body_into(output).await?;
        output.write_literal("|").await?;
        output.write_section(body, "footer").await?;
        output.write_section(body, "page-only").await?;
        output.write_literal("o").await?;
        Ok(())
    }
}

fn document<'a, 'r: 'a>(scope: Scope<'a, 'r>) -> impl AsyncSlice + 'a {
    let owner = |index| Owner(scope.request().state.clone(), index);
    declare_async_layout(
        Page {
            scope,
            _owner: owner(0),
        },
        declare_async_layout(
            Inner {
                scope,
                _owner: owner(1),
            },
            Outer {
                scope,
                _owner: owner(2),
            },
        ),
    )
}

struct Bare<S>(S);
impl<S: AsyncSlice> AsyncSlice for Bare<S> {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        output.write_slice(&self.0).await
    }
}

#[derive(Debug)]
struct Original(Arc<()>);
impl std::fmt::Display for Original {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("original combined request destination failure")
    }
}
impl std::error::Error for Original {}

struct Writer(Arc<Mutex<State>>);
impl AsyncSliceWriter for Writer {
    async fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        let held = {
            let mut state = self.0.lock().unwrap();
            let held = state.stall_at == Some(state.inputs.len());
            let failed = state.mode == Mode::WriteError && bytes == b"foot";
            state.inputs.push(bytes.to_vec());
            let accepted = if held || failed {
                bytes.len().min(1)
            } else {
                bytes.len()
            };
            state.bytes.extend_from_slice(&bytes[..accepted]);
            if failed {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    Original(state.original.clone()),
                ));
            }
            held
        };
        if held {
            let _operation = Operation(self.0.clone());
            pending::<()>().await;
        }
        Ok(())
    }

    async fn flush(&mut self) -> io::Result<()> {
        let held = {
            let mut state = self.0.lock().unwrap();
            state.flushes += 1;
            state.mode == Mode::Flush
        };
        if held {
            let _operation = Operation(self.0.clone());
            pending::<()>().await;
        }
        Ok(())
    }
}

struct Noop;
impl Wake for Noop {
    fn wake(self: Arc<Self>) {}
}
fn poll<F: Future>(future: std::pin::Pin<&mut F>) -> Poll<F::Output> {
    future.poll(&mut Context::from_waker(&Waker::from(Arc::new(Noop))))
}
fn assert_cancelled(result: Result<(), AsyncRenderError>) {
    let Err(AsyncRenderError::Write(error)) = result else {
        panic!("expected cancellation");
    };
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    assert!(error.get_ref().unwrap().is::<RenderCancelled>());
}
fn assert_original(result: Result<(), AsyncRenderError>, state: &State) {
    let Err(AsyncRenderError::Write(error)) = result else {
        panic!("expected original destination failure");
    };
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    let original = error.get_ref().unwrap().downcast_ref::<Original>().unwrap();
    assert!(Arc::ptr_eq(&original.0, &state.original));
}
fn ready<S: AsyncSlice, W: AsyncSliceWriter + Send>(
    page: &S,
    writer: &mut W,
    scope: Scope<'_, '_>,
) {
    let mut render = Box::pin(render_incremental_with_context(page, writer, scope));
    let Poll::Ready(result) = poll(render.as_mut()) else {
        panic!("render unexpectedly pending");
    };
    result.unwrap();
}

#[test]
fn pinned_csharp_matches_combined_tokens_context_and_cooperative_cancellation() {
    let mut actual = String::new();
    for (name, mode, bare, expected) in [
        ("document", Mode::Ready, false, DOCUMENT),
        ("bare", Mode::Ready, true, "Pb"),
        ("section-cancel", Mode::Section, false, SECTION_PREFIX),
        ("flush-cancel", Mode::Flush, false, FLUSH_PREFIX),
        ("write-error", Mode::WriteError, false, WRITE_PREFIX),
    ] {
        let label = String::from("borrowed-request");
        let state = State::new(&label, mode);
        let context = RenderContext::new(
            Request {
                label: &label,
                state: state.clone(),
            },
            CancellationToken::new(),
        );
        let page = document(&context);
        let wrapped = Box::new(&page);
        let mut writer = Writer(state.clone());
        let partial = Bare(&wrapped);
        let result = {
            let mut render = Box::pin(async {
                if bare {
                    render_incremental_with_context(&partial, &mut writer, &context).await
                } else {
                    render_incremental_with_context(&&wrapped, &mut writer, &context).await
                }
            });
            fn is_send(_: &impl Send) {}
            is_send(&render);
            let mut result = poll(render.as_mut());
            if matches!(mode, Mode::Section | Mode::Flush) {
                assert!(result.is_pending());
                assert_eq!(state.lock().unwrap().bytes, expected.as_bytes());
                context.cancellation_token().cancel();
                result = poll(render.as_mut());
            }
            let Poll::Ready(result) = result else {
                panic!("combined render must complete or cancel");
            };
            result
        };
        let status = match mode {
            Mode::Ready => {
                result.unwrap();
                "ok"
            }
            Mode::Section | Mode::Flush => {
                assert_cancelled(result);
                "cancelled"
            }
            Mode::WriteError => {
                assert_original(result, &state.lock().unwrap());
                "original-error"
            }
        };
        {
            let state = state.lock().unwrap();
            assert_eq!(state.bytes, expected.as_bytes());
            assert_eq!(state.owners, [0; 3]);
            assert_eq!(state.traps, 0);
            let seams = state
                .visits
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>();
            let expected_seams = if bare {
                &["page-body", "bare-child"][..]
            } else if mode == Mode::Section {
                &SEAMS[..9]
            } else if mode == Mode::Ready {
                SEAMS
            } else {
                &SEAMS[..10]
            };
            assert_eq!(seams, expected_seams);
            assert_eq!(
                state.flushes,
                match mode {
                    Mode::Ready if bare => 1,
                    Mode::Ready => 2,
                    Mode::Flush => 1,
                    _ => 0,
                }
            );
            actual.push_str(&format!(
                "{name}: status={status};output={expected};seams={};tokens=same;trap=0\n",
                seams.join(","),
            ));
        }
        context.cancellation_token().cancel();
        assert!(state
            .lock()
            .unwrap()
            .visits
            .iter()
            .all(|(_, token)| token.is_cancelled()));
        drop(wrapped);
        drop(page);
        assert_eq!(state.lock().unwrap().owners, [1; 3]);
        assert_eq!(context.request().label, label);
    }
    assert_eq!(
        actual,
        include_str!("fixtures/combined-request-cancellation.txt")
    );
}

#[test]
fn every_combined_write_cancels_or_drops_after_its_exact_accepted_prefix() {
    let label = String::from("request-a");
    let baseline = State::new(&label, Mode::Ready);
    let context = RenderContext::new(
        Request {
            label: &label,
            state: baseline.clone(),
        },
        CancellationToken::new(),
    );
    let page = document(&context);
    ready(&page, &mut Writer(baseline.clone()), &context);
    let inputs = baseline.lock().unwrap().inputs.clone();
    assert_eq!(inputs.concat(), DOCUMENT.as_bytes());
    assert_eq!(inputs.len(), 13);
    assert!(inputs.iter().any(|bytes| bytes == b"foot"));
    for call in 0..inputs.len() {
        for cancel in [false, true] {
            let state = State::new(&label, Mode::Ready);
            state.lock().unwrap().stall_at = Some(call);
            let context = RenderContext::new(
                Request {
                    label: &label,
                    state: state.clone(),
                },
                CancellationToken::new(),
            );
            let page = document(&context);
            let wrapped = Box::new(&page);
            let mut writer = Writer(state.clone());
            let mut expected = inputs[..call].concat();
            expected.extend_from_slice(&inputs[call][..1]);
            {
                let borrowed = &wrapped;
                let mut render = Box::pin(render_incremental_with_context(
                    &borrowed,
                    &mut writer,
                    &context,
                ));
                assert!(poll(render.as_mut()).is_pending());
                assert!(poll(render.as_mut()).is_pending());
                assert_eq!(state.lock().unwrap().bytes, expected);
                if cancel {
                    context.cancellation_token().cancel();
                    let Poll::Ready(result) = poll(render.as_mut()) else {
                        panic!("combined write {call} did not cancel");
                    };
                    assert_cancelled(result);
                }
            }

            {
                let state = state.lock().unwrap();
                assert_eq!(state.bytes, expected);
                assert_eq!(state.inputs.len(), call + 1);
                assert_eq!(state.operation_drops, 1);
                assert_eq!(state.flushes, usize::from(call == inputs.len() - 1));
                assert_eq!(state.owners, [0; 3]);
                assert_eq!(state.traps, 0);
                assert_eq!(context.cancellation_token().is_cancelled(), cancel);
                assert!(state
                    .visits
                    .iter()
                    .all(|(_, token)| token.is_cancelled() == cancel));
            }
            state.lock().unwrap().stall_at = None;
            let mut caller_write = Box::pin(writer.write_all(b"caller"));
            assert!(matches!(poll(caller_write.as_mut()), Poll::Ready(Ok(()))));
            expected.extend_from_slice(b"caller");
            assert_eq!(state.lock().unwrap().bytes, expected);
        }
    }
}

#[test]
fn borrowed_models_repeat_without_cross_request_cancellation_or_owner_destruction() {
    let label_a = String::from("request-a");
    let label_b = String::from("request-b");
    let state_a = State::new(&label_a, Mode::Section);
    let state_b = State::new(&label_b, Mode::Ready);
    let context_a = RenderContext::new(
        Request {
            label: &label_a,
            state: state_a.clone(),
        },
        CancellationToken::new(),
    );
    let context_b = RenderContext::new(
        Request {
            label: &label_b,
            state: state_b.clone(),
        },
        CancellationToken::new(),
    );
    let page_a = document(&context_a);
    let page_b = document(&context_b);
    let wrapped_b = Box::new(&page_b);
    let mut writer_a = Writer(state_a.clone());
    let mut writer_b = Writer(state_b.clone());
    {
        let mut held = Box::pin(render_incremental_with_context(
            &page_a,
            &mut writer_a,
            &context_a,
        ));
        assert!(poll(held.as_mut()).is_pending());
        for round in 1..=2 {
            ready(&wrapped_b, &mut writer_b, &context_b);
            assert_eq!(
                state_b.lock().unwrap().bytes,
                DOCUMENT.repeat(round).as_bytes()
            );
        }
        context_a.cancellation_token().cancel();
        let Poll::Ready(result) = poll(held.as_mut()) else {
            panic!("request A did not cancel");
        };
        assert_cancelled(result);
    }
    ready(&wrapped_b, &mut writer_b, &context_b);
    {
        let a = state_a.lock().unwrap();
        let b = state_b.lock().unwrap();
        assert_eq!(a.bytes, SECTION_PREFIX.as_bytes());
        assert_eq!(a.operation_drops, 1);
        assert_eq!(a.flushes, 0);
        assert_eq!(b.bytes, DOCUMENT.repeat(3).as_bytes());
        assert_eq!(b.flushes, 6);
        assert_eq!(
            b.visits.iter().map(|(seam, _)| *seam).collect::<Vec<_>>(),
            SEAMS.repeat(3)
        );
        assert!(a.visits.iter().all(|(_, token)| token.is_cancelled()));
        assert!(b.visits.iter().all(|(_, token)| !token.is_cancelled()));
        assert_eq!(a.owners, [0; 3]);
        assert_eq!(b.owners, [0; 3]);
    }
    let before = {
        let state = state_a.lock().unwrap();
        (
            state.bytes.clone(),
            state.visits.len(),
            state.inputs.len(),
            state.flushes,
        )
    };
    let mut repeated = Box::pin(render_incremental_with_context(
        &page_a,
        &mut writer_a,
        &context_a,
    ));
    let Poll::Ready(result) = poll(repeated.as_mut()) else {
        panic!("cancelled request must not restart");
    };
    assert_cancelled(result);
    drop(repeated);
    {
        let state = state_a.lock().unwrap();
        assert_eq!(
            (
                state.bytes.clone(),
                state.visits.len(),
                state.inputs.len(),
                state.flushes
            ),
            before,
        );
    }
    drop(wrapped_b);
    drop(page_a);
    drop(page_b);
    assert_eq!(state_a.lock().unwrap().owners, [1; 3]);
    assert_eq!(state_b.lock().unwrap().owners, [1; 3]);
    assert_eq!(context_a.request().label, label_a);
    assert_eq!(context_b.request().label, label_b);
}

#[cfg(feature = "tokio")]
#[tokio::test]
async fn actual_one_byte_duplex_cancels_in_the_combined_footer_partial_write() {
    use razor_slices::TokioWriter;
    use tokio::io::{AsyncReadExt, DuplexStream};

    struct Destination(TokioWriter<DuplexStream>, Arc<Mutex<State>>);
    impl AsyncSliceWriter for Destination {
        async fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
            self.0.write_all(bytes).await
        }
        async fn flush(&mut self) -> io::Result<()> {
            self.1.lock().unwrap().flushes += 1;
            self.0.flush().await
        }
    }

    let label = String::from("request-real-io");
    let state = State::new(&label, Mode::Ready);
    let context = RenderContext::new(
        Request {
            label: &label,
            state: state.clone(),
        },
        CancellationToken::new(),
    );
    let page = document(&context);
    let wrapped = Box::new(&page);
    let (sink, mut reader) = tokio::io::duplex(1);
    let mut writer = Destination(TokioWriter::new(sink), state.clone());
    let mut prefix = vec![0; WRITE_PREFIX.len()];
    let rendered = {
        let render = render_incremental_with_context(&wrapped, &mut writer, &context);
        let receive = async {
            reader.read_exact(&mut prefix).await.unwrap();
            assert_eq!(prefix, WRITE_PREFIX.as_bytes());
            assert_eq!(
                state
                    .lock()
                    .unwrap()
                    .visits
                    .iter()
                    .map(|(seam, _)| *seam)
                    .collect::<Vec<_>>(),
                SEAMS[..10],
            );
            context.cancellation_token().cancel();
        };
        let (rendered, ()) = tokio::join!(render, receive);
        rendered
    };
    assert_cancelled(rendered);
    assert_eq!(state.lock().unwrap().flushes, 0);
    assert!(state
        .lock()
        .unwrap()
        .visits
        .iter()
        .all(|(_, token)| token.is_cancelled()));
    writer.write_all(b"C").await.unwrap();
    let mut caller = [0];
    reader.read_exact(&mut caller).await.unwrap();
    assert_eq!(caller, *b"C");
    drop(writer);
    let mut suffix = Vec::new();
    reader.read_to_end(&mut suffix).await.unwrap();
    assert!(suffix.is_empty());
    assert_eq!(state.lock().unwrap().owners, [0; 3]);
    drop(wrapped);
    drop(page);
    assert_eq!(state.lock().unwrap().owners, [1; 3]);
}
