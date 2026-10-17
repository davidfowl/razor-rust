//! Buffered and incremental UTF-8 HTML responses for Axum 0.7.
//!
//! [`SliceResponse`] renders synchronously before publishing a response body.
//! It does not stream, execute async templates, or change the core runtime.
//! [`StreamingSliceResponse`] executes an owned async template as its response
//! body is polled, with bounded output and drop-based cancellation.
//!
//! ```
//! use axum::{http::StatusCode, response::IntoResponse};
//! use razor_slices::html;
//! use razor_slices_axum::SliceResponse;
//!
//! let response = SliceResponse::new(html! { "<p>" {"<World>"} "</p>" })
//!     .with_status(StatusCode::CREATED)
//!     .into_response();
//! assert_eq!(response.status(), StatusCode::CREATED);
//! assert_eq!(response.headers()["content-type"], "text/html; charset=utf-8");
//! ```

use std::io;

use axum::{
    http::{header::CONTENT_TYPE, StatusCode},
    response::{IntoResponse, Response},
};
use razor_slices::Slice;

mod streaming;
pub use streaming::StreamingSliceResponse;

/// A slice returned from an Axum handler as fully buffered UTF-8 HTML.
///
/// Defaults to status 200 and `text/html; charset=utf-8`. Rendering runs once
/// per conversion, on the calling thread, before any bytes become visible to
/// Axum. Borrowed slices/models need not be `'static` because the resulting
/// response owns its body.
///
/// [`IntoResponse`] logs a rendering/UTF-8 error through `tracing` and returns
/// status 500 with a generic plain-text body, discarding all rendered bytes.
/// Install a tracing subscriber to collect those events, or use
/// [`Self::try_into_response`] for application-owned error handling.
///
/// Buffers are unbounded; applications must bound input/output and avoid
/// expensive blocking work inside templates. This is not a streaming adapter.
#[must_use]
pub struct SliceResponse<S> {
    slice: S,
    status: StatusCode,
}

impl<S: Slice> SliceResponse<S> {
    /// Wraps a slice with the default status 200.
    pub fn new(slice: S) -> Self {
        Self {
            slice,
            status: StatusCode::OK,
        }
    }

    /// Sets the status used on successful rendering.
    ///
    /// Rendering failures still produce 500 through [`IntoResponse`].
    /// Standard HTTP rules still apply: for example, HEAD and status 204 do
    /// not transmit a body when served over HTTP.
    pub fn with_status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }

    /// Renders once and returns an owned HTML response only after success.
    ///
    /// Preserves the original [`io::Error`] from
    /// [`Slice::render_to_writer`]. Malformed UTF-8 is an
    /// [`io::ErrorKind::InvalidData`] error. No partial response escapes on
    /// either failure. This fallible path does not log: the caller owns error
    /// handling and observability.
    pub fn try_into_response(self) -> io::Result<Response> {
        let mut bytes = Vec::new();
        self.slice.render_to_writer(&mut bytes)?;
        let html = String::from_utf8(bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        Ok((
            self.status,
            [(CONTENT_TYPE, "text/html; charset=utf-8")],
            html,
        )
            .into_response())
    }
}

impl<S: Slice> IntoResponse for SliceResponse<S> {
    fn into_response(self) -> Response {
        match self.try_into_response() {
            Ok(response) => response,
            Err(error) => {
                tracing::error!(
                    target: "razor_slices_axum",
                    error = %error,
                    error_kind = ?error.kind(),
                    "slice response rendering failed"
                );
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error").into_response()
            }
        }
    }
}
