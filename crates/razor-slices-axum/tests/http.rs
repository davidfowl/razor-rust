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
use http_body_util::BodyExt;
use razor_slices::{html, HtmlString, Slice, WriteHtmlValue};
use razor_slices_axum::SliceResponse;
use tower::ServiceExt;
use tracing::instrument::WithSubscriber;

#[path = "../examples/support/page.rs"]
mod page;

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
