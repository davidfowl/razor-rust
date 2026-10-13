use std::{
    fmt, io,
    sync::{Arc, Mutex},
};

use axum::{
    body::Body,
    http::{Request, StatusCode},
    response::IntoResponse,
    routing::get,
    Router,
};
use http_body_util::{BodyExt, Empty};
use razor_slices::{
    async_template, html, AsyncHtmlWriter, AsyncRenderError, AsyncSlice, AsyncSliceWriter,
    CancellationToken, HtmlString, RenderCancelled, RenderContext, Slice, WriteHtmlValue,
};
use razor_slices_axum::{SliceResponse, StreamingSliceResponse};
use tower::ServiceExt;
use tracing::instrument::WithSubscriber;

#[path = "../../razor-slices/examples/support/async_values.rs"]
mod generated_values;
#[path = "../examples/support/page.rs"]
mod page;
#[path = "../examples/support/streaming.rs"]
mod streaming_sample;

async fn body(response: axum::response::Response) -> Vec<u8> {
    response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec()
}

#[tokio::test]
async fn get_model_driven_html_is_utf8_and_encoded_exactly_once() {
    let app = Router::new().route(
        "/",
        get(|| async {
            SliceResponse::new(page::greeting_page(page::Greeting {
                name: "<世界> & \"friends\" '雪'",
                trusted_body: HtmlString::from("<strong>Trusted & ready</strong>"),
            }))
        }),
    );
    let response = app
        .oneshot(Request::get("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-type"],
        "text/html; charset=utf-8"
    );
    assert_eq!(
        body(response).await,
        include_bytes!("fixtures/greeting.html")
    );
}

#[tokio::test]
async fn custom_status_and_borrowed_trait_object_are_supported() {
    let name = String::from("&amp;");
    let borrowed_name = name.as_str();
    let slice = html! { "<p>" {borrowed_name} "</p>" };
    let borrowed: &dyn Slice = &slice;
    let response = SliceResponse::new(borrowed)
        .with_status(StatusCode::CREATED)
        .try_into_response()
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(body(response).await, b"<p>&amp;amp;</p>");
}

#[tokio::test]
async fn render_invoked_once_and_boxed_slice_supported() {
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let captured = count.clone();
    let slice: Box<dyn Slice> = Box::new(html! {
        {{captured.fetch_add(1, std::sync::atomic::Ordering::SeqCst); "once"}}
    });
    assert_eq!(
        body(SliceResponse::new(slice).into_response()).await,
        b"once"
    );
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn get_custom_status_and_head_follow_axum_semantics() {
    let app = Router::new().route(
        "/",
        get(|| async {
            SliceResponse::new(html! { "<p>Created</p>" }).with_status(StatusCode::CREATED)
        }),
    );
    let response = app
        .clone()
        .oneshot(Request::get("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(
        response.headers()["content-type"],
        "text/html; charset=utf-8"
    );
    assert_eq!(body(response).await, b"<p>Created</p>");
    let response = app
        .oneshot(
            Request::builder()
                .method("HEAD")
                .uri("/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(
        response.headers()["content-type"],
        "text/html; charset=utf-8"
    );
    assert!(body(response).await.is_empty());
}

#[derive(Clone)]
struct Logs(Arc<Mutex<Vec<u8>>>);

impl io::Write for Logs {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Logs {
    type Writer = Self;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[tokio::test]
async fn get_failure_logs_original_error_and_discards_partial_body_and_status() {
    let logs = Logs(Arc::new(Mutex::new(Vec::new())));
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_writer(logs.clone())
        .finish();
    let app = Router::new().route(
        "/error",
        get(|| async { SliceResponse::new(page::BrokenPage).with_status(StatusCode::CREATED) }),
    );
    let response = app
        .oneshot(Request::get("/error").body(Body::empty()).unwrap())
        .with_subscriber(subscriber)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        response.headers()["content-type"],
        "text/plain; charset=utf-8"
    );
    assert_eq!(body(response).await, b"Internal Server Error");
    let recorded = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(recorded.contains("ERROR"));
    assert!(recorded.contains("razor_slices_axum"));
    assert!(recorded.contains("slice response rendering failed"));
    assert!(recorded.contains("deliberate example rendering failure"));
    assert!(recorded.contains("error_kind=Other"));
}

#[derive(Debug)]
struct OriginalError;

impl fmt::Display for OriginalError {
    fn fmt(&self, writer: &mut fmt::Formatter<'_>) -> fmt::Result {
        writer.write_str("original value error")
    }
}
impl std::error::Error for OriginalError {}

struct BrokenValue;
impl WriteHtmlValue for BrokenValue {
    fn write_html_to(&self, writer: &mut dyn io::Write) -> io::Result<()> {
        writer.write_all(b"partial secret")?;
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            OriginalError,
        ))
    }
}

#[test]
fn fallible_conversion_preserves_original_value_error() {
    let error = SliceResponse::new(html! { "<p>" {BrokenValue} "</p>" })
        .try_into_response()
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(error.get_ref().unwrap().is::<OriginalError>());
}

struct TextFailure;
impl Slice for TextFailure {
    fn render(&self, writer: &mut dyn fmt::Write) -> fmt::Result {
        writer.write_str("partial secret")?;
        Err(fmt::Error)
    }
}

#[tokio::test]
async fn handwritten_text_failure_is_500_without_partial_output() {
    let response = SliceResponse::new(TextFailure).into_response();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body(response).await, b"Internal Server Error");
}

struct InvalidUtf8;
impl WriteHtmlValue for InvalidUtf8 {
    fn write_html_to(&self, writer: &mut dyn io::Write) -> io::Result<()> {
        writer.write_all(&[0xff])
    }
}

#[tokio::test]
async fn malformed_byte_renderer_cannot_claim_utf8_html() {
    let error = SliceResponse::new(html! { "prefix" {InvalidUtf8} })
        .try_into_response()
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    let response = SliceResponse::new(html! { "prefix" {InvalidUtf8} }).into_response();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body(response).await, b"Internal Server Error");
}

#[tokio::test]
async fn empty_slice_has_successful_html_response() {
    let response = SliceResponse::new(html! {}).into_response();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-type"],
        "text/html; charset=utf-8"
    );
    assert!(body(response).await.is_empty());
}

use std::sync::atomic::{AtomicUsize, Ordering};

fn rendering_error<'a>(mut error: &'a (dyn std::error::Error + 'static)) -> &'a AsyncRenderError {
    loop {
        if let Some(error) = error.downcast_ref::<AsyncRenderError>() {
            return error;
        }
        error = error
            .source()
            .expect("HTTP body must retain the rendering error in its source chain");
    }
}

struct CountedPage {
    started: Arc<AtomicUsize>,
    completed: Arc<AtomicUsize>,
    dropped: Arc<AtomicUsize>,
    release: Arc<tokio::sync::Notify>,
}

impl Drop for CountedPage {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::SeqCst);
    }
}

impl AsyncSlice for CountedPage {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        self.started.fetch_add(1, Ordering::SeqCst);
        output.write_literal("prefix").await?;
        output.flush().await?;
        self.release.notified().await;
        self.completed.fetch_add(1, Ordering::SeqCst);
        output.write_value("<tail>").await?;
        Ok(())
    }
}

fn counted_page() -> CountedPage {
    CountedPage {
        started: Arc::new(AtomicUsize::new(0)),
        completed: Arc::new(AtomicUsize::new(0)),
        dropped: Arc::new(AtomicUsize::new(0)),
        release: Arc::new(tokio::sync::Notify::new()),
    }
}

#[tokio::test]
async fn streaming_conversion_is_lazy_and_prefix_precedes_later_application_await() {
    let page = counted_page();
    let started = page.started.clone();
    let completed = page.completed.clone();
    let release = page.release.clone();
    let response = StreamingSliceResponse::new(page)
        .with_status(StatusCode::CREATED)
        .into_response();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(
        response.headers()["content-type"],
        "text/html; charset=utf-8"
    );
    assert!(!response.headers().contains_key("content-length"));
    assert_eq!(started.load(Ordering::SeqCst), 0);
    let mut output = response.into_body();
    let prefix = output.frame().await.unwrap().unwrap().into_data().unwrap();
    assert_eq!(prefix, b"prefix".as_slice());
    assert_eq!(started.load(Ordering::SeqCst), 1);
    assert_eq!(completed.load(Ordering::SeqCst), 0);
    release.notify_one();
    assert_eq!(
        output.collect().await.unwrap().to_bytes(),
        b"&lt;tail&gt;".as_slice()
    );
    assert_eq!(completed.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn dropping_stream_cancels_polling_of_renderer_and_application_await() {
    let page = counted_page();
    let completed = page.completed.clone();
    let dropped = page.dropped.clone();
    let release = page.release.clone();
    let mut output = StreamingSliceResponse::new(page)
        .into_response()
        .into_body();
    assert_eq!(
        output.frame().await.unwrap().unwrap().into_data().unwrap(),
        b"prefix".as_slice()
    );
    drop(output);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    release.notify_one();
    tokio::task::yield_now().await;
    assert_eq!(completed.load(Ordering::SeqCst), 0);
}

struct LargePage {
    text: String,
    completed: Arc<AtomicUsize>,
}

impl AsyncSlice for LargePage {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        output.write_literal(&self.text).await?;
        self.completed.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn streaming_frames_are_bounded_and_slow_consumers_hold_render_work() {
    let completed = Arc::new(AtomicUsize::new(0));
    let text = "雪<&>".repeat(2_000);
    let page = LargePage {
        text: text.clone(),
        completed: completed.clone(),
    };
    let mut output = StreamingSliceResponse::new(page)
        .into_response()
        .into_body();
    let first = output.frame().await.unwrap().unwrap().into_data().unwrap();
    assert_eq!(first.len(), 1_024);
    assert_eq!(completed.load(Ordering::SeqCst), 0);
    tokio::task::yield_now().await;
    assert_eq!(completed.load(Ordering::SeqCst), 0);
    let mut bytes = first.to_vec();
    while let Some(frame) = output.frame().await {
        let bytes_frame = frame.unwrap().into_data().unwrap();
        assert!(!bytes_frame.is_empty());
        assert!(bytes_frame.len() <= 1_024);
        bytes.extend_from_slice(&bytes_frame);
    }
    assert_eq!(bytes, text.as_bytes());
    assert_eq!(completed.load(Ordering::SeqCst), 1);
}

struct StreamingFailure;
impl AsyncSlice for StreamingFailure {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        output.write_literal("prefix").await?;
        Err(io::Error::new(io::ErrorKind::PermissionDenied, OriginalError).into())
    }
}

#[tokio::test]
async fn streaming_failure_logs_original_source_and_is_an_error_not_successful_eof() {
    let logs = Logs(Arc::new(Mutex::new(Vec::new())));
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_writer(logs.clone())
        .finish();
    async {
        let response = StreamingSliceResponse::new(StreamingFailure)
            .with_status(StatusCode::CREATED)
            .into_response();
        assert_eq!(response.status(), StatusCode::CREATED);
        let mut output = response.into_body();
        assert_eq!(
            output.frame().await.unwrap().unwrap().into_data().unwrap(),
            b"prefix".as_slice()
        );
        let error = output.frame().await.unwrap().unwrap_err();
        match rendering_error(&error) {
            AsyncRenderError::Write(error) => {
                assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
                assert!(error.get_ref().unwrap().is::<OriginalError>());
            }
            other => panic!("unexpected error: {other:?}"),
        }
        assert!(output.frame().await.is_none());
    }
    .with_subscriber(subscriber)
    .await;
    let recorded = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(recorded.contains("streaming slice response rendering failed"));
    assert!(recorded.contains("original value error"));
    assert!(recorded.contains("PermissionDenied"));
    assert_eq!(
        recorded
            .matches("streaming slice response rendering failed")
            .count(),
        1
    );
}

struct EarlyFailure;
impl AsyncSlice for EarlyFailure {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        _: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        Err(razor_slices::RenderError(fmt::Error).into())
    }
}

#[tokio::test]
async fn failure_before_first_write_is_still_a_body_error_and_not_a_buffered_500() {
    let response = StreamingSliceResponse::new(EarlyFailure).into_response();
    assert_eq!(response.status(), StatusCode::OK);
    let error = response.into_body().collect().await.unwrap_err();
    assert!(matches!(
        rendering_error(&error),
        AsyncRenderError::Render(_)
    ));
}

struct EmptyAsync;
impl AsyncSlice for EmptyAsync {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        _: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        Ok(())
    }
}

#[tokio::test]
async fn empty_async_template_has_html_status_and_clean_eof() {
    let response = StreamingSliceResponse::new(EmptyAsync).into_response();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-type"],
        "text/html; charset=utf-8"
    );
    assert!(body(response).await.is_empty());
}

#[tokio::test]
async fn runnable_streaming_error_route_fails_its_body_without_error_html() {
    let app = streaming_sample::app(Arc::new(tokio::sync::Notify::new()));
    let response = app
        .oneshot(Request::get("/error").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-type"],
        "text/html; charset=utf-8"
    );
    let error = response.into_body().collect().await.unwrap_err();
    match rendering_error(&error) {
        AsyncRenderError::Write(error) => {
            assert_eq!(error.to_string(), "deliberate streaming failure")
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn head_strips_stream_without_starting_the_owned_renderer() {
    let page = counted_page();
    let started = page.started.clone();
    let dropped = page.dropped.clone();
    let page = Arc::new(Mutex::new(Some(page)));
    let app = Router::new().route(
        "/",
        get(move || {
            let page = page.lock().unwrap().take().unwrap();
            async move { StreamingSliceResponse::new(page).with_status(StatusCode::CREATED) }
        }),
    );
    let response = app
        .oneshot(
            Request::builder()
                .method("HEAD")
                .uri("/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(
        response.headers()["content-type"],
        "text/html; charset=utf-8"
    );
    assert!(body(response).await.is_empty());
    assert_eq!(started.load(Ordering::SeqCst), 0);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
}

async fn http_client(
    address: std::net::SocketAddr,
) -> (
    hyper::client::conn::http1::SendRequest<Empty<axum::body::Bytes>>,
    tokio::task::JoinHandle<Result<(), hyper::Error>>,
) {
    let socket = tokio::net::TcpStream::connect(address).await.unwrap();
    let (sender, connection) =
        hyper::client::conn::http1::handshake(hyper_util::rt::TokioIo::new(socket))
            .await
            .unwrap();
    (sender, tokio::spawn(connection))
}

struct AbortOnDrop<T>(tokio::task::JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[derive(Default)]
struct DisconnectState {
    suspended: tokio::sync::Notify,
    released: tokio::sync::Notify,
    dropped: tokio::sync::Notify,
    drop_count: AtomicUsize,
    completed: AtomicUsize,
    write_pending: std::sync::atomic::AtomicBool,
    dropped_during_write: std::sync::atomic::AtomicBool,
}

struct DisconnectGuard(Arc<DisconnectState>);

impl Drop for DisconnectGuard {
    fn drop(&mut self) {
        self.0.dropped_during_write.store(
            self.0.write_pending.load(Ordering::SeqCst),
            Ordering::SeqCst,
        );
        self.0.drop_count.fetch_add(1, Ordering::SeqCst);
        self.0.dropped.notify_one();
    }
}

struct DisconnectPage {
    state: Arc<DisconnectState>,
    write_until_disconnected: bool,
}

impl AsyncSlice for DisconnectPage {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        let _guard = DisconnectGuard(self.state.clone());
        output.write_literal("prefix").await?;
        output.flush().await?;
        if self.write_until_disconnected {
            let text = "x".repeat(64 * 1_024);
            loop {
                let mut write = std::pin::pin!(output.write_literal(&text));
                std::future::poll_fn(|cx| {
                    let result = std::future::Future::poll(write.as_mut(), cx);
                    let pending = result.is_pending();
                    self.state.write_pending.store(pending, Ordering::SeqCst);
                    if pending {
                        self.state.suspended.notify_one();
                    }
                    result
                })
                .await?;
            }
        }
        self.state.suspended.notify_one();
        self.state.released.notified().await;
        self.state.completed.fetch_add(1, Ordering::SeqCst);
        output.write_literal("must not be rendered").await?;
        Ok(())
    }
}

async fn real_peer_disconnect(write_until_disconnected: bool) {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let state = Arc::new(DisconnectState::default());
        let token = CancellationToken::new();
        let captured_token = token.clone();
        let captured = state.clone();
        let app = Router::new().route(
            "/",
            get(move || {
                let state = captured.clone();
                let token = captured_token.clone();
                async move {
                    StreamingSliceResponse::new(DisconnectPage {
                        state,
                        write_until_disconnected,
                    })
                    .with_cancellation_token(token)
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let _server = AbortOnDrop(tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        }));
        let (mut client, connection) = http_client(address).await;
        let mut connection = AbortOnDrop(connection);
        let response = client
            .send_request(
                Request::builder()
                    .uri("/")
                    .header("host", address.to_string())
                    .body(Empty::new())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let mut output = response.into_body();
        assert_eq!(
            output.frame().await.unwrap().unwrap().into_data().unwrap(),
            b"prefix".as_slice()
        );
        state.suspended.notified().await;
        assert_eq!(state.drop_count.load(Ordering::SeqCst), 0);
        assert_eq!(state.completed.load(Ordering::SeqCst), 0);
        assert!(!token.is_cancelled());

        // Aborting the client driver closes its TCP socket, not the server body.
        connection.0.abort();
        assert!((&mut connection.0).await.unwrap_err().is_cancelled());
        drop(output);
        drop(client);
        state.dropped.notified().await;
        token.cancelled().await;
        assert!(token.is_cancelled());
        assert_eq!(state.drop_count.load(Ordering::SeqCst), 1);
        assert_eq!(
            state.dropped_during_write.load(Ordering::SeqCst),
            write_until_disconnected
        );
        state.released.notify_one();
        assert_eq!(state.completed.load(Ordering::SeqCst), 0);
    })
    .await
    .expect("real peer disconnect must drop the suspended server renderer");
}

#[tokio::test]
async fn real_peer_disconnect_cancels_held_application_await() {
    real_peer_disconnect(false).await;
}

#[tokio::test]
async fn real_peer_disconnect_cancels_pending_destination_write() {
    real_peer_disconnect(true).await;
}

struct HeldBeforeOutput {
    suspended: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
    completed: Arc<AtomicUsize>,
    fail: bool,
}

impl AsyncSlice for HeldBeforeOutput {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        self.suspended.notify_one();
        self.release.notified().await;
        self.completed.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, OriginalError).into());
        }
        output.write_value("<世界>").await?;
        Ok(())
    }
}

async fn real_http_empty_prefix_commitment(fail: bool) {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let suspended = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let completed = Arc::new(AtomicUsize::new(0));
        let captured_suspended = suspended.clone();
        let captured_release = release.clone();
        let captured_completed = completed.clone();
        let app = Router::new().route(
            "/",
            get(move || {
                let page = HeldBeforeOutput {
                    suspended: captured_suspended.clone(),
                    release: captured_release.clone(),
                    completed: captured_completed.clone(),
                    fail,
                };
                async move { StreamingSliceResponse::new(page).with_status(StatusCode::CREATED) }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let _server = AbortOnDrop(tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        }));
        let (mut client, connection) = http_client(address).await;
        let mut connection = AbortOnDrop(connection);
        let response = client
            .send_request(
                Request::builder()
                    .uri("/")
                    .header("host", address.to_string())
                    .body(Empty::new())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(
            response.headers()["content-type"],
            "text/html; charset=utf-8"
        );
        assert_eq!(response.headers()["transfer-encoding"], "chunked");
        suspended.notified().await;
        assert_eq!(completed.load(Ordering::SeqCst), 0);
        release.notify_one();
        let output = response.into_body().collect().await;
        if fail {
            assert!(
                output.is_err(),
                "post-header failure must fail the transfer, not return clean EOF or error HTML"
            );
        } else {
            assert_eq!(output.unwrap().to_bytes(), "&lt;世界&gt;".as_bytes());
        }
        assert_eq!(completed.load(Ordering::SeqCst), 1);
        connection.0.abort();
        match (&mut connection.0).await {
            Err(error) => assert!(error.is_cancelled()),
            Ok(result) => result.unwrap(),
        }
    })
    .await
    .expect("HTTP/1 headers must arrive before releasing empty-prefix rendering");
}

#[tokio::test]
async fn real_http_headers_arrive_before_first_output_and_later_success() {
    real_http_empty_prefix_commitment(false).await;
}

#[tokio::test]
async fn real_http_failure_after_empty_prefix_headers_is_not_a_replacement_500() {
    real_http_empty_prefix_commitment(true).await;
}

#[tokio::test]
async fn real_http_prefix_is_received_before_separate_release_request_and_exact_suffix() {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = streaming_sample::app(Arc::new(tokio::sync::Notify::new()));
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let (mut client, connection) = http_client(address).await;
        let response = client
            .send_request(
                Request::builder()
                    .uri("/stream")
                    .header("host", address.to_string())
                    .body(Empty::new())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()["content-type"],
            "text/html; charset=utf-8"
        );
        let mut output = response.into_body();
        let prefix = output.frame().await.unwrap().unwrap().into_data().unwrap();
        assert_eq!(prefix, b"<p>Loading...</p>\n".as_slice());

        let (mut release_client, release_connection) = http_client(address).await;
        let released = release_client
            .send_request(
                Request::builder()
                    .method("POST")
                    .uri("/release")
                    .header("host", address.to_string())
                    .body(Empty::new())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(released.status(), StatusCode::NO_CONTENT);
        assert!(released
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .is_empty());
        let mut bytes = prefix.to_vec();
        bytes.extend_from_slice(&output.collect().await.unwrap().to_bytes());
        assert_eq!(bytes, include_bytes!("fixtures/streaming.html"));
        connection.abort();
        release_connection.abort();
        server.abort();
    })
    .await
    .expect("prefix-before-release HTTP handshake must not deadlock");
}

#[tokio::test]
async fn generated_mixed_values_stream_exactly_over_http_before_later_work() {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let release = Arc::new(tokio::sync::Notify::new());
        let loads = Arc::new(AtomicUsize::new(0));
        let captured_release = release.clone();
        let captured_loads = loads.clone();
        let app = Router::new().route(
            "/values",
            get(move || {
                let page = generated_values::ValuePage::new(generated_values::ValueModel::demo(
                    captured_release.clone(),
                    captured_loads.clone(),
                ));
                async move { StreamingSliceResponse::new(page) }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let _server = AbortOnDrop(tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        }));
        let (mut client, connection) = http_client(address).await;
        let _connection = AbortOnDrop(connection);
        let response = client
            .send_request(
                Request::builder()
                    .uri("/values")
                    .header("host", address.to_string())
                    .body(Empty::new())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()["content-type"],
            "text/html; charset=utf-8"
        );
        let mut output = response.into_body();
        let mut prefix = Vec::new();
        while prefix.len() < generated_values::PREFIX.len() {
            let frame = output.frame().await.unwrap().unwrap().into_data().unwrap();
            prefix.extend_from_slice(&frame);
        }
        assert_eq!(prefix, generated_values::PREFIX.as_bytes());
        assert_eq!(loads.load(Ordering::SeqCst), 0);
        release.notify_one();
        prefix.extend_from_slice(&output.collect().await.unwrap().to_bytes());
        assert_eq!(prefix, generated_values::PAGE.as_bytes());
        assert_eq!(loads.load(Ordering::SeqCst), 1);
    })
    .await
    .expect("generated value HTTP prefix handshake must not deadlock");
}

#[tokio::test]
async fn explicit_token_cancellation_is_logged_original_body_error_not_eof() {
    let page = counted_page();
    let completed = page.completed.clone();
    let dropped = page.dropped.clone();
    let token = CancellationToken::new();
    let mut output = StreamingSliceResponse::new(page)
        .with_cancellation_token(token.clone())
        .into_response()
        .into_body();
    assert_eq!(
        output.frame().await.unwrap().unwrap().into_data().unwrap(),
        b"prefix".as_slice()
    );
    let logs = Logs(Arc::new(Mutex::new(Vec::new())));
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_writer(logs.clone())
        .finish();
    token.cancel();
    let error = output
        .collect()
        .with_subscriber(subscriber)
        .await
        .unwrap_err();
    match rendering_error(&error) {
        AsyncRenderError::Write(error) => {
            assert_eq!(error.kind(), io::ErrorKind::Interrupted);
            assert!(error.get_ref().unwrap().is::<RenderCancelled>());
        }
        other => panic!("cancellation source lost: {other:?}"),
    }
    assert_eq!(completed.load(Ordering::SeqCst), 0);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    let recorded = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(recorded.contains("request rendering cancelled"));
    assert!(
        recorded.contains("error_kind=Some(Interrupted)"),
        "{recorded}"
    );
}

#[tokio::test]
async fn token_cancellation_interrupts_bounded_pending_body_write() {
    let completed = Arc::new(AtomicUsize::new(0));
    let response = StreamingSliceResponse::new(LargePage {
        text: "x".repeat(64 * 1_024),
        completed: completed.clone(),
    });
    let token = response.cancellation_token().clone();
    let mut output = response.into_response().into_body();
    assert_eq!(
        output
            .frame()
            .await
            .unwrap()
            .unwrap()
            .into_data()
            .unwrap()
            .len(),
        1_024
    );
    assert_eq!(completed.load(Ordering::SeqCst), 0);
    token.cancel();
    loop {
        match output.frame().await {
            Some(Ok(frame)) => assert!(frame.into_data().unwrap().len() <= 1_024),
            Some(Err(error)) => {
                match rendering_error(&error) {
                    AsyncRenderError::Write(error) => {
                        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
                        assert!(error.get_ref().unwrap().is::<RenderCancelled>());
                    }
                    other => panic!("unexpected error: {other:?}"),
                }
                break;
            }
            None => panic!("cancelled pending write returned successful EOF"),
        }
    }
    assert_eq!(completed.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn unfinished_unpolled_drop_signals_token_but_successful_eof_does_not() {
    let response = StreamingSliceResponse::new(EmptyAsync);
    let token = response.cancellation_token().clone();
    let unpolled_body = response.into_response().into_body();
    assert!(!token.is_cancelled());
    drop(unpolled_body);
    assert!(token.is_cancelled());
    let response = StreamingSliceResponse::new(EmptyAsync);
    let token = response.cancellation_token().clone();
    assert!(body(response.into_response()).await.is_empty());
    assert!(!token.is_cancelled());
}

struct RequestData {
    name: String,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
    completed: AtomicUsize,
}

async_template! {
    struct RequestChild { model: Arc<RenderContext<RequestData>> } => {
        "<p>" {&model.request().name} "</p>"
    }
}

async_template! {
    struct RequestPage { model: Arc<RenderContext<RequestData>> } => {
        "<h1>" {&model.request().name} "</h1>"
        @flush;
        @{ model.request().entered.notify_one(); }
        @await {model.request().release.notified()};
        @render {RequestChild::new(model.clone())};
        @{ model.request().completed.fetch_add(1, Ordering::SeqCst); }
    }
}

#[tokio::test]
async fn real_http_request_contexts_are_isolated_and_explicit_cancellation_fails_transfer() {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let make = |name: &str| {
            Arc::new(RenderContext::new(
                RequestData {
                    name: name.to_owned(),
                    entered: tokio::sync::Notify::new(),
                    release: tokio::sync::Notify::new(),
                    completed: AtomicUsize::new(0),
                },
                CancellationToken::new(),
            ))
        };
        let a = make("<a>");
        let b = make("&b");
        let captured_a = a.clone();
        let captured_b = b.clone();
        let app = Router::new()
            .route(
                "/a",
                get(move || {
                    let context = captured_a.clone();
                    async move {
                        StreamingSliceResponse::new(RequestPage::new(context.clone()))
                            .with_status(StatusCode::CREATED)
                            .with_cancellation_token(context.cancellation_token().clone())
                    }
                }),
            )
            .route(
                "/b",
                get(move || {
                    let context = captured_b.clone();
                    async move {
                        StreamingSliceResponse::new(RequestPage::new(context.clone()))
                            .with_cancellation_token(context.cancellation_token().clone())
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let mut server = AbortOnDrop(tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        }));
        let (mut client_a, connection_a) = http_client(address).await;
        let (mut client_b, connection_b) = http_client(address).await;
        let mut connection_a = AbortOnDrop(connection_a);
        let mut connection_b = AbortOnDrop(connection_b);
        let request = |path| {
            Request::builder()
                .uri(path)
                .header("host", address.to_string())
                .body(Empty::new())
                .unwrap()
        };
        let response_a = client_a.send_request(request("/a")).await.unwrap();
        let response_b = client_b.send_request(request("/b")).await.unwrap();
        assert_eq!(response_a.status(), StatusCode::CREATED);
        assert_eq!(response_b.status(), StatusCode::OK);
        assert_eq!(
            response_a.headers()["content-type"],
            "text/html; charset=utf-8"
        );
        let mut output_a = response_a.into_body();
        let mut output_b = response_b.into_body();
        let read_prefix = |output: hyper::body::Incoming, expected: &'static [u8]| async move {
            let mut output = output;
            let mut prefix = Vec::new();
            while prefix.len() < expected.len() {
                prefix.extend_from_slice(
                    &output.frame().await.unwrap().unwrap().into_data().unwrap(),
                );
            }
            assert_eq!(prefix, expected);
            output
        };
        output_a = read_prefix(output_a, b"<h1>&lt;a&gt;</h1>").await;
        output_b = read_prefix(output_b, b"<h1>&amp;b</h1>").await;
        a.request().entered.notified().await;
        b.request().entered.notified().await;
        a.cancellation_token().cancel();
        assert!(
            output_a.collect().await.is_err(),
            "HTTP cancellation must fail transfer"
        );
        assert_eq!(a.request().completed.load(Ordering::SeqCst), 0);
        assert!(!b.cancellation_token().is_cancelled());
        b.request().release.notify_one();
        assert_eq!(
            output_b.collect().await.unwrap().to_bytes(),
            b"<p>&amp;b</p>".as_slice()
        );
        assert_eq!(b.request().completed.load(Ordering::SeqCst), 1);
        assert!(!b.cancellation_token().is_cancelled());
        drop(client_a);
        drop(client_b);
        connection_a.0.abort();
        connection_b.0.abort();
        let _ = (&mut connection_a.0).await;
        let _ = (&mut connection_b.0).await;
        server.0.abort();
        assert!((&mut server.0).await.unwrap_err().is_cancelled());
    })
    .await
    .expect("isolated request-context HTTP handshake must not deadlock");
}
