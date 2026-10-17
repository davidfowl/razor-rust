//! Body-owned incremental rendering with bounded frames and no producer task.

use std::{
    future::Future,
    io,
    pin::Pin,
    task::{Context, Poll},
};

use axum::{
    body::{Body, Bytes},
    http::{header::CONTENT_TYPE, StatusCode},
    response::{IntoResponse, Response},
};
use futures_core::Stream;
use razor_slices::{render_incremental, AsyncRenderError, AsyncSlice, AsyncSliceWriter};
use tokio::sync::mpsc;

const FRAME_SIZE: usize = 1_024;

/// An owned async slice rendered incrementally as Axum polls its body.
///
/// Defaults to status 200 and `text/html; charset=utf-8`. Conversion does not
/// execute the template. The body owns/polls the render future, with one queued
/// frame and at most one pending write frame, each at most 1,024 bytes. There is
/// no spawned producer or full-page buffer. Models must be owned or `'static`.
///
/// Errors are logged through `tracing` and emitted as body errors with their
/// original source. Headers/status are already selected: a failed stream is
/// not replaced with a clean 500 or successful EOF. Use [`crate::SliceResponse`]
/// when atomic buffered error responses are required.
///
/// Dropping the body stops polling its render/application futures. Accepted
/// bytes are not rolled back; independently spawned work is not cancelled.
/// Sink acceptance/explicit flush makes frames available, not a guarantee that
/// a client/socket received them. Panics are not caught.
///
/// ```
/// use axum::response::IntoResponse;
/// use razor_slices::{AsyncHtmlWriter, AsyncRenderError, AsyncSlice, AsyncSliceWriter};
/// use razor_slices_axum::StreamingSliceResponse;
/// struct Page { title: String }
/// impl AsyncSlice for Page {
///     async fn render_into<W: AsyncSliceWriter + Send>(
///         &self, out: &mut AsyncHtmlWriter<'_, W>,
///     ) -> Result<(), AsyncRenderError> {
///         out.write_literal("<h1>").await?;
///         out.write_value(&self.title).await?;
///         out.write_literal("</h1>").await?;
///         Ok(())
///     }
/// }
/// let response = StreamingSliceResponse::new(Page { title: "<World>".into() })
///     .into_response();
/// assert_eq!(response.status(), 200);
/// assert_eq!(response.headers()["content-type"], "text/html; charset=utf-8");
/// ```
///
/// Borrowed models cannot escape into an owned HTTP body:
///
/// ```compile_fail
/// use razor_slices::{AsyncHtmlWriter, AsyncRenderError, AsyncSlice, AsyncSliceWriter};
/// use razor_slices_axum::StreamingSliceResponse;
/// struct Page<'a>(&'a str);
/// impl AsyncSlice for Page<'_> {
///     async fn render_into<W: AsyncSliceWriter + Send>(
///         &self, out: &mut AsyncHtmlWriter<'_, W>,
///     ) -> Result<(), AsyncRenderError> {
///         out.write_value(self.0).await?;
///         Ok(())
///     }
/// }
/// let title = String::from("Title");
/// let _ = StreamingSliceResponse::new(Page(&title));
/// ```
#[must_use]
pub struct StreamingSliceResponse<S> {
    slice: S,
    status: StatusCode,
}

impl<S: AsyncSlice + Send + 'static> StreamingSliceResponse<S> {
    /// Wraps an owned incremental template with the default status 200.
    pub fn new(slice: S) -> Self {
        Self {
            slice,
            status: StatusCode::OK,
        }
    }

    /// Sets the status selected before streaming execution.
    ///
    /// Unlike the buffered adapter, render failures cannot change this status
    /// to 500. Standard HTTP body rules still apply; Axum strips GET bodies for
    /// HEAD, dropping the unpolled renderer.
    pub fn with_status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }
}

impl<S: AsyncSlice + Send + 'static> IntoResponse for StreamingSliceResponse<S> {
    fn into_response(self) -> Response {
        let Self { slice, status } = self;
        let (sender, receiver) = mpsc::channel(1);
        let mut writer = OutputWriter(sender);
        let rendering = Box::pin(async move { render_incremental(&slice, &mut writer).await });
        let stream = OutputStream {
            receiver,
            rendering: Some(rendering),
            error: None,
        };
        (
            status,
            [(CONTENT_TYPE, "text/html; charset=utf-8")],
            Body::from_stream(stream),
        )
            .into_response()
    }
}

struct OutputWriter(mpsc::Sender<Bytes>);

impl AsyncSliceWriter for OutputWriter {
    async fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        for chunk in bytes.chunks(FRAME_SIZE) {
            self.0
                .send(Bytes::copy_from_slice(chunk))
                .await
                .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "response body closed"))?;
        }
        Ok(())
    }

    async fn flush(&mut self) -> io::Result<()> {
        if self.0.is_closed() {
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "response body closed",
            ))
        } else {
            Ok(())
        }
    }
}

type Rendering = Pin<Box<dyn Future<Output = Result<(), AsyncRenderError>> + Send>>;

struct OutputStream {
    receiver: mpsc::Receiver<Bytes>,
    rendering: Option<Rendering>,
    error: Option<AsyncRenderError>,
}

impl Stream for OutputStream {
    type Item = Result<Bytes, AsyncRenderError>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if let Some(rendering) = &mut this.rendering {
            if let Poll::Ready(result) = rendering.as_mut().poll(context) {
                this.rendering = None;
                if let Err(error) = result {
                    let error_kind = match &error {
                        AsyncRenderError::Write(error) => Some(error.kind()),
                        AsyncRenderError::Render(_) => None,
                    };
                    tracing::error!(
                        target: "razor_slices_axum",
                        error = %error,
                        error_kind = ?error_kind,
                        "streaming slice response rendering failed"
                    );
                    this.error = Some(error);
                }
            }
        }
        match this.receiver.poll_recv(context) {
            Poll::Ready(Some(bytes)) => Poll::Ready(Some(Ok(bytes))),
            Poll::Ready(None) => Poll::Ready(this.error.take().map(Err)),
            Poll::Pending => Poll::Pending,
        }
    }
}
