//! Typed request state and executor-independent cooperative cancellation.

use std::{
    future::{poll_fn, Future},
    io,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};

/// The typed source of an interrupted request-scoped render.
///
/// Cancellation returns an [`io::Error`] of kind [`io::ErrorKind::Interrupted`]
/// containing this source, including through [`super::AsyncRenderError::Write`].
#[derive(Debug)]
pub struct RenderCancelled;

impl std::fmt::Display for RenderCancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("request rendering cancelled")
    }
}

impl std::error::Error for RenderCancelled {}

fn cancellation_error() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, RenderCancelled)
}

#[derive(Default)]
struct State {
    cancelled: bool,
    waiters: Vec<Option<Waker>>,
}

/// A cloneable, one-shot, runtime-agnostic cooperative cancellation signal.
///
/// Clones share one scope. Dropping a token does not cancel it; call
/// [`Self::cancel`] explicitly. Tokens cannot be reset. Use a fresh token for a
/// new request. Cancellation does not stop threads or independently spawned
/// work; only code observing this token participates.
#[derive(Clone, Default)]
pub struct CancellationToken {
    state: Arc<Mutex<State>>,
}

impl CancellationToken {
    /// Creates an uncancelled scope without requiring an executor.
    pub fn new() -> Self {
        Self::default()
    }

    /// Signals cancellation once and wakes all current observers.
    pub fn cancel(&self) {
        let waiters = {
            let mut state = self.state.lock().expect("cancellation state poisoned");
            state.cancelled = true;
            std::mem::take(&mut state.waiters)
        };
        for waker in waiters.into_iter().flatten() {
            waker.wake();
        }
    }

    /// Reports whether this scope has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.state
            .lock()
            .expect("cancellation state poisoned")
            .cancelled
    }

    /// Waits for cancellation, including cancellation before the first poll.
    ///
    /// Dropping the returned future unregisters its waker. Storage is bounded
    /// by simultaneous observers, not the number of previous awaits.
    pub fn cancelled(&self) -> impl Future<Output = ()> + Send + '_ {
        Cancelled {
            token: self,
            slot: None,
        }
    }

    /// Races fallible work against this scope, dropping it on cancellation.
    ///
    /// Cancellation wins when already observed at the start of a poll; work
    /// ready in that poll otherwise retains its original result/error. Work
    /// must return from each poll: blocking code cannot be interrupted.
    /// Accepted writes/side effects are not rolled back. The destination or
    /// work must support future-drop cancellation; do not retry partial output.
    pub async fn run<T, E: From<io::Error>>(
        &self,
        future: impl Future<Output = Result<T, E>>,
    ) -> Result<T, E> {
        let mut cancelled = std::pin::pin!(self.cancelled());
        let mut future = std::pin::pin!(future);
        poll_fn(|cx| {
            if cancelled.as_mut().poll(cx).is_ready() {
                Poll::Ready(Err(cancellation_error().into()))
            } else {
                future.as_mut().poll(cx)
            }
        })
        .await
    }
}

struct Cancelled<'a> {
    token: &'a CancellationToken,
    slot: Option<usize>,
}

impl Future for Cancelled<'_> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.get_mut();
        let candidate = cx.waker().clone();
        let mut state = this
            .token
            .state
            .lock()
            .expect("cancellation state poisoned");
        if state.cancelled {
            return Poll::Ready(());
        }
        if let Some(slot) = this.slot {
            let waker = state.waiters[slot].as_mut().expect("registered waiter");
            if !waker.will_wake(cx.waker()) {
                let previous = std::mem::replace(waker, candidate);
                drop(state);
                drop(previous);
            }
        } else {
            let slot = state
                .waiters
                .iter()
                .position(Option::is_none)
                .unwrap_or(state.waiters.len());
            if slot == state.waiters.len() {
                state.waiters.push(Some(candidate));
            } else {
                state.waiters[slot] = Some(candidate);
            }
            this.slot = Some(slot);
        }
        Poll::Pending
    }
}

impl Drop for Cancelled<'_> {
    fn drop(&mut self) {
        if let Some(slot) = self.slot {
            let mut state = self
                .token
                .state
                .lock()
                .expect("cancellation state poisoned");
            let previous = state.waiters.get_mut(slot).and_then(Option::take);
            while state.waiters.last().is_some_and(Option::is_none) {
                state.waiters.pop();
            }
            drop(state);
            drop(previous);
        }
    }
}

/// Immutable, typed data and a cancellation token for one rendering request.
///
/// `T` can own request state or borrow it, without type erasure or `'static`
/// requirements. Bind `&RenderContext<T>` through existing template models
/// and pass the same context to [`super::render_incremental_with_context`].
/// Child/section models explicitly borrow the same typed data; the writer
/// automatically carries the token even to templates without context models.
/// Repeated successful renders reuse the data, not execution state or output.
///
/// Borrowed request data cannot escape:
///
/// ```compile_fail
/// use razor_slices::{CancellationToken, RenderContext};
/// let context;
/// {
///     let name = String::from("request");
///     context = RenderContext::new(name.as_str(), CancellationToken::new());
/// }
/// println!("{}", context.request());
/// ```
///
/// Non-Sync request data cannot become a shared Send render model:
///
/// ```compile_fail
/// use razor_slices::{async_template, CancellationToken, RenderContext};
/// use std::cell::Cell;
/// async_template! {
///     struct Page<'a> { model: &'a RenderContext<Cell<u32>> } => {
///         {model.request().get()}
///     }
/// }
/// ```
pub struct RenderContext<T> {
    request: T,
    cancellation: CancellationToken,
}

impl<T> RenderContext<T> {
    /// Associates typed request data with its dedicated token.
    pub fn new(request: T, cancellation: CancellationToken) -> Self {
        Self {
            request,
            cancellation,
        }
    }

    /// Borrows this request's data, without copying or changing its lifetime.
    pub fn request(&self) -> &T {
        &self.request
    }

    /// Borrows the token shared by this scope's templates and destination.
    pub fn cancellation_token(&self) -> &CancellationToken {
        &self.cancellation
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::task::Wake;

    struct Noop;
    impl Wake for Noop {
        fn wake(self: Arc<Self>) {}
    }

    struct CancelOnDrop(CancellationToken);
    impl Wake for CancelOnDrop {
        fn wake(self: Arc<Self>) {}
    }
    impl Drop for CancelOnDrop {
        fn drop(&mut self) {
            self.0.cancel();
        }
    }

    #[test]
    fn unregister_and_replace_drop_application_wakers_outside_state_lock() {
        for replace in [false, true] {
            let token = CancellationToken::new();
            let waker = Waker::from(Arc::new(CancelOnDrop(token.clone())));
            let mut observer = Box::pin(token.cancelled());
            assert!(observer
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending());
            drop(waker);
            if replace {
                let other = Waker::from(Arc::new(Noop));
                assert!(observer
                    .as_mut()
                    .poll(&mut Context::from_waker(&other))
                    .is_pending());
                assert!(observer
                    .as_mut()
                    .poll(&mut Context::from_waker(&other))
                    .is_ready());
            } else {
                drop(observer);
            }
            assert!(token.is_cancelled());
        }
    }

    #[test]
    fn observers_unregister_reuse_slots_and_replace_wakers() {
        let token = CancellationToken::new();
        let waker = Waker::from(Arc::new(Noop));
        let other = Waker::from(Arc::new(Noop));
        let mut first = std::pin::pin!(token.cancelled());
        assert!(first
            .as_mut()
            .poll(&mut Context::from_waker(&waker))
            .is_pending());
        assert!(first
            .as_mut()
            .poll(&mut Context::from_waker(&other))
            .is_pending());
        assert!(token.state.lock().unwrap().waiters[0]
            .as_ref()
            .unwrap()
            .will_wake(&other));
        for _ in 0..1_000 {
            let mut observer = Box::pin(token.cancelled());
            assert!(observer
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending());
            assert_eq!(token.state.lock().unwrap().waiters.len(), 2);
            drop(observer);
            assert_eq!(token.state.lock().unwrap().waiters.len(), 1);
        }
        token.cancel();
        token.cancel();
        assert!(first
            .as_mut()
            .poll(&mut Context::from_waker(&other))
            .is_ready());
        assert!(token.state.lock().unwrap().waiters.is_empty());
    }
}
