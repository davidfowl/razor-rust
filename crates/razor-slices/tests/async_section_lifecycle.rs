use razor_slices::{
    declare_async_layout, render_incremental, render_incremental_with_context, AsyncHtmlWriter,
    AsyncLayout, AsyncRenderError, AsyncSlice, AsyncSliceWriter, CancellationToken,
    RenderCancelled, RenderContext,
};
use std::{
    fmt,
    future::{poll_fn, Future},
    io,
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    task::Poll,
};
use tokio::sync::Notify;

const DOCUMENT: &[u8] = b"OIPiFHhfFHhfo";
const FIXTURE: &str = include_str!("fixtures/async-section-lifecycle.txt");

#[derive(Clone, Default)]
struct Counts {
    locals: [usize; 5],
    live: [usize; 5],
    owners: [usize; 3],
    trace: String,
    headers: usize,
    entered_gate: bool,
    writer_drops: usize,
}

#[derive(Default)]
struct State {
    counts: Mutex<Counts>,
    held_at: AtomicUsize,
    application_at: AtomicUsize,
    release: Notify,
    failure: Arc<()>,
}

impl State {
    fn snapshot(&self) -> Counts {
        self.counts.lock().unwrap().clone()
    }
}

struct Local {
    state: Arc<State>,
    node: usize,
}

impl Local {
    fn new(state: &Arc<State>, node: usize) -> Self {
        let mut counts = state.counts.lock().unwrap();
        counts.live[node] += 1;
        counts.trace.push(char::from(b"PIOFH"[node]));
        Self {
            state: state.clone(),
            node,
        }
    }
}

impl Drop for Local {
    fn drop(&mut self) {
        let mut counts = self.state.counts.lock().unwrap();
        counts.live[self.node] -= 1;
        counts.locals[self.node] += 1;
        counts.trace.push(char::from(b"piofh"[self.node]));
    }
}

struct Owner {
    state: Arc<State>,
    node: usize,
}

impl Owner {
    fn new(state: &Arc<State>, node: usize) -> Self {
        Self {
            state: state.clone(),
            node,
        }
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        self.state.counts.lock().unwrap().owners[self.node] += 1;
    }
}

struct Page(Owner);

impl AsyncSlice for Page {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        let _local = Local::new(&self.0.state, 0);
        output.write_literal("P").await?;
        Ok(())
    }

    async fn render_section_into<W: AsyncSliceWriter + Send>(
        &self,
        name: &str,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        if name != "header" {
            return Ok(());
        }
        let state = &self.0.state;
        let _local = Local::new(state, 4);
        let invocation = {
            let mut counts = state.counts.lock().unwrap();
            counts.headers += 1;
            counts.headers
        };
        output.write_literal("H").await?;
        if invocation == state.held_at.load(Ordering::SeqCst) {
            state.counts.lock().unwrap().entered_gate = true;
            state.release.notified().await;
        }
        if invocation == state.application_at.load(Ordering::SeqCst) {
            return Err(io::Error::other(Original(state.failure.clone())).into());
        }
        output.write_literal("h").await?;
        Ok(())
    }
}

struct Inner(Owner);

impl AsyncLayout for Inner {
    async fn render_layout_into<B: AsyncSlice, W: AsyncSliceWriter + Send>(
        &self,
        body: &B,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        let _local = Local::new(&self.0.state, 1);
        output.write_literal("I").await?;
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
        if name != "footer" {
            return Ok(());
        }
        let _local = Local::new(&self.0.state, 3);
        output.write_literal("F").await?;
        output.write_section(body, "header").await?;
        output.write_literal("f").await?;
        Ok(())
    }
}

struct Outer(Owner);

impl AsyncLayout for Outer {
    async fn render_layout_into<B: AsyncSlice, W: AsyncSliceWriter + Send>(
        &self,
        body: &B,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        let _local = Local::new(&self.0.state, 2);
        output.write_literal("O").await?;
        body.render_body_into(output).await?;
        output.write_section(body, "footer").await?;
        output.write_section(body, "footer").await?;
        output.write_section(body, "unused").await?;
        output.write_literal("o").await?;
        Ok(())
    }
}

fn document(state: &Arc<State>) -> impl AsyncSlice {
    declare_async_layout(
        Page(Owner::new(state, 0)),
        declare_async_layout(Inner(Owner::new(state, 1)), Outer(Owner::new(state, 2))),
    )
}

struct Root<'a, S> {
    slice: &'a S,
    bare: bool,
}

impl<S: AsyncSlice> AsyncSlice for Root<'_, S> {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        if self.bare {
            self.slice.render_body_into(output).await
        } else {
            self.slice.render_into(output).await
        }
    }
}

#[derive(Debug)]
struct Original(Arc<()>);

impl fmt::Display for Original {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("original section lifecycle failure")
    }
}
impl std::error::Error for Original {}

fn assert_cancelled(error: AsyncRenderError) {
    let AsyncRenderError::Write(error) = error else {
        panic!("expected observable cancellation");
    };
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    assert!(error.get_ref().unwrap().is::<RenderCancelled>());
}

fn assert_original(error: AsyncRenderError, source: &Arc<()>, kind: io::ErrorKind) {
    let AsyncRenderError::Write(error) = error else {
        panic!("expected original I/O error");
    };
    assert_eq!(error.kind(), kind);
    let original = error.get_ref().unwrap().downcast_ref::<Original>().unwrap();
    assert!(Arc::ptr_eq(&original.0, source));
}

#[derive(Default)]
struct Writes {
    bytes: Vec<u8>,
    fail_at: Option<usize>,
    held_at: Option<usize>,
    entered: bool,
    flushes: usize,
}

struct Destination {
    state: Arc<State>,
    writes: Arc<Mutex<Writes>>,
    failure: Arc<()>,
}

impl Destination {
    fn new(state: &Arc<State>, fail_at: Option<usize>) -> Self {
        Self {
            state: state.clone(),
            writes: Arc::new(Mutex::new(Writes {
                fail_at,
                ..Writes::default()
            })),
            failure: Arc::new(()),
        }
    }
}

impl Drop for Destination {
    fn drop(&mut self) {
        self.state.counts.lock().unwrap().writer_drops += 1;
    }
}

impl AsyncSliceWriter for Destination {
    async fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        poll_fn(|_| {
            let mut writes = self.writes.lock().unwrap();
            if writes.fail_at == Some(writes.bytes.len()) {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    Original(self.failure.clone()),
                )));
            }
            if writes.held_at == Some(writes.bytes.len()) {
                writes.entered = true;
                return Poll::Pending;
            }
            writes.bytes.extend_from_slice(bytes);
            Poll::Ready(Ok(()))
        })
        .await
    }

    async fn flush(&mut self) -> io::Result<()> {
        self.writes.lock().unwrap().flushes += 1;
        Ok(())
    }
}

async fn poll_once<F: Future>(mut future: Pin<&mut F>) -> Poll<F::Output> {
    poll_fn(|context| Poll::Ready(future.as_mut().poll(context))).await
}

async fn render_scoped(
    slice: &impl AsyncSlice,
    destination: &mut Destination,
    context: &RenderContext<()>,
    scoped: bool,
) -> Result<(), AsyncRenderError> {
    if scoped {
        render_incremental_with_context(slice, destination, context).await
    } else {
        render_incremental(slice, destination).await
    }
}

struct Case {
    label: String,
    held_at: usize,
    fail_at: Option<usize>,
    application_at: usize,
    cancel: bool,
    repeats: usize,
    bare: bool,
}

impl Case {
    fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            held_at: 0,
            fail_at: None,
            application_at: 0,
            cancel: false,
            repeats: 1,
            bare: false,
        }
    }
}

fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for held_at in 0..=2 {
        for fail_at in std::iter::once(None).chain((0..DOCUMENT.len()).map(Some)) {
            let mode = if held_at == 0 {
                "ready".into()
            } else {
                format!("held-{held_at}")
            };
            let outcome =
                fail_at.map_or_else(|| "success".into(), |offset| format!("error-{offset}"));
            cases.push(Case {
                held_at,
                fail_at,
                ..Case::new(format!("{mode}/{outcome}"))
            });
        }
    }
    for invocation in 1..=2 {
        cases.push(Case {
            held_at: invocation,
            application_at: invocation,
            ..Case::new(format!("application-{invocation}"))
        });
    }
    for invocation in 1..=2 {
        cases.push(Case {
            held_at: invocation,
            cancel: true,
            ..Case::new(format!("cancel-{invocation}"))
        });
    }
    cases.push(Case {
        repeats: 2,
        ..Case::new("reuse")
    });
    cases.push(Case {
        bare: true,
        ..Case::new("bare")
    });
    cases
}

fn common_row(label: &str, status: &str, state: &State, writes: &Writes) -> String {
    let counts = state.snapshot();
    assert_eq!(counts.live, [0; 5]);
    format!(
        "common/{label}: status={status};output={};local={};trace={};gate={}",
        std::str::from_utf8(&writes.bytes).unwrap(),
        counts.locals.map(|n| n.to_string()).join(","),
        counts.trace,
        usize::from(counts.entered_gate),
    )
}

async fn run_case(case: &Case, scoped: bool) -> String {
    let state = Arc::new(State::default());
    state.held_at.store(case.held_at, Ordering::SeqCst);
    state
        .application_at
        .store(case.application_at, Ordering::SeqCst);
    let document = document(&state);
    let root = Root {
        slice: &document,
        bare: case.bare,
    };
    let wrapper = Box::new(&root);
    let mut destination = Destination::new(&state, case.fail_at);
    let context = RenderContext::new((), CancellationToken::new());
    assert_eq!(state.snapshot().locals, [0; 5]);
    assert!(state.snapshot().trace.is_empty());
    let mut status = "ok";
    for _ in 0..case.repeats {
        let result = {
            let mut render = Box::pin(render_scoped(&wrapper, &mut destination, &context, scoped));
            match poll_once(render.as_mut()).await {
                Poll::Ready(result) => result,
                Poll::Pending => {
                    let counts = state.snapshot();
                    assert!(counts.entered_gate);
                    assert_eq!(counts.live, [0, 0, 1, 1, 1]);
                    if case.cancel {
                        assert!(scoped);
                        context.cancellation_token().cancel();
                    } else {
                        state.release.notify_one();
                    }
                    match poll_once(render.as_mut()).await {
                        Poll::Ready(result) => result,
                        Poll::Pending => {
                            panic!("section release/cancellation must settle rendering")
                        }
                    }
                }
            }
        };
        match result {
            Ok(()) => {
                assert!(case.fail_at.is_none() && case.application_at == 0 && !case.cancel);
            }
            Err(error) if case.cancel => {
                assert_cancelled(error);
                status = "cancelled";
            }
            Err(error) if case.application_at != 0 => {
                assert_original(error, &state.failure, io::ErrorKind::Other);
                status = "same-application-error";
            }
            Err(error) => {
                assert!(case.fail_at.is_some());
                assert_original(error, &destination.failure, io::ErrorKind::BrokenPipe);
                status = "same-write-error";
            }
        }
    }
    let row = common_row(
        &case.label,
        status,
        &state,
        &destination.writes.lock().unwrap(),
    );
    let before = state.snapshot();
    assert_eq!(before.owners, [0; 3]);
    assert_eq!(
        destination.writes.lock().unwrap().flushes,
        if status == "ok" { case.repeats } else { 0 }
    );
    state.held_at.store(0, Ordering::SeqCst);
    state.application_at.store(0, Ordering::SeqCst);
    destination.writes.lock().unwrap().fail_at = None;
    for _ in 0..2 {
        destination.writes.lock().unwrap().bytes.clear();
        render_incremental(&document, &mut destination)
            .await
            .unwrap();
        assert_eq!(destination.writes.lock().unwrap().bytes, DOCUMENT);
        assert_eq!(state.snapshot().owners, [0; 3]);
    }
    assert_eq!(
        state.snapshot().locals,
        std::array::from_fn(|node| before.locals[node] + if node < 3 { 2 } else { 4 })
    );
    drop(wrapper);
    drop(document);
    assert_eq!(state.snapshot().owners, [1; 3]);
    assert_eq!(state.snapshot().writer_drops, 0);
    drop(destination);
    assert_eq!(state.snapshot().writer_drops, 1);
    row
}

#[tokio::test]
async fn pinned_inherited_section_cleanup_and_caller_owned_retries() {
    let cases = cases();
    assert_eq!(cases.len(), 48);
    let expected: Vec<_> = FIXTURE
        .lines()
        .filter(|line| line.starts_with("common/"))
        .collect();
    assert_eq!(expected.len(), 48);
    for scoped in [false, true] {
        for (case, expected) in cases.iter().zip(&expected) {
            if !scoped && case.cancel {
                continue;
            }
            assert_eq!(run_case(case, scoped).await, *expected, "scoped={scoped}");
        }
    }
}

#[tokio::test]
async fn held_inherited_section_and_write_drop_or_cancel_release_only_locals() {
    for invocation in 1..=2 {
        for held_write in [false, true] {
            for mode in ["legacy-drop", "scoped-drop", "cancel"] {
                let state = Arc::new(State::default());
                let prefix = if invocation == 1 { 6 } else { 10 };
                if !held_write {
                    state.held_at.store(invocation, Ordering::SeqCst);
                }
                let document = document(&state);
                let mut destination = Destination::new(&state, None);
                let writes = destination.writes.clone();
                if held_write {
                    writes.lock().unwrap().held_at = Some(prefix);
                }
                let context = RenderContext::new((), CancellationToken::new());
                let mut render = Box::pin(render_scoped(
                    &document,
                    &mut destination,
                    &context,
                    mode != "legacy-drop",
                ));
                assert!(poll_once(render.as_mut()).await.is_pending());
                assert_eq!(state.snapshot().live, [0, 0, 1, 1, 1]);
                assert_eq!(writes.lock().unwrap().bytes, DOCUMENT[..prefix]);
                assert_eq!(writes.lock().unwrap().entered, held_write);
                assert_eq!(state.snapshot().entered_gate, !held_write);
                if mode == "cancel" {
                    context.cancellation_token().cancel();
                    let Poll::Ready(Err(error)) = poll_once(render.as_mut()).await else {
                        panic!("cancellation must settle held section rendering");
                    };
                    assert_cancelled(error);
                }
                drop(render);
                assert_eq!(state.snapshot().live, [0; 5]);
                assert_eq!(state.snapshot().locals, [1, 1, 1, invocation, invocation]);
                assert_eq!(state.snapshot().owners, [0; 3]);
                assert_eq!(writes.lock().unwrap().flushes, 0);
                assert_eq!(
                    context.cancellation_token().is_cancelled(),
                    mode == "cancel"
                );
                state.release.notify_one();
                assert_eq!(writes.lock().unwrap().bytes, DOCUMENT[..prefix]);
                state.held_at.store(0, Ordering::SeqCst);
                {
                    let mut writes = writes.lock().unwrap();
                    writes.held_at = None;
                    writes.bytes.clear();
                }
                render_incremental(&document, &mut destination)
                    .await
                    .unwrap();
                assert_eq!(writes.lock().unwrap().bytes, DOCUMENT);
                assert_eq!(
                    state.snapshot().locals,
                    [2, 2, 2, invocation + 2, invocation + 2]
                );
                assert_eq!(state.snapshot().owners, [0; 3]);
                drop(document);
                assert_eq!(state.snapshot().owners, [1; 3]);
                assert_eq!(state.snapshot().writer_drops, 0);
                drop(destination);
                assert_eq!(state.snapshot().writer_drops, 1);
            }
        }
    }
}

#[tokio::test]
async fn dropping_an_unpolled_root_never_acquires_section_resources() {
    let state = Arc::new(State::default());
    let document = document(&state);
    let mut destination = Destination::new(&state, None);
    drop(render_incremental(&document, &mut destination));
    assert!(state.snapshot().trace.is_empty());
    assert_eq!(state.snapshot().locals, [0; 5]);
    assert_eq!(state.snapshot().owners, [0; 3]);
    assert_eq!(destination.writes.lock().unwrap().flushes, 0);
    drop(document);
    assert_eq!(state.snapshot().owners, [1; 3]);
}
