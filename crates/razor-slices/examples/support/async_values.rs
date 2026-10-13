use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use razor_slices::{async_template, attribute, HtmlString};
use tokio::sync::Notify;

pub const PREFIX: &str =
    "<h1 data-count=\"2\" title=\"&lt;Values&gt;\">&lt;Values&gt;true<b>trusted</b></h1>\n";
pub const PAGE: &str = concat!(
    "<h1 data-count=\"2\" title=\"&lt;Values&gt;\">&lt;Values&gt;true<b>trusted</b></h1>\n",
    "<button checked=\"checked\" empty=\"\" title=\"&lt;&amp;&quot;&#39;\">2 -1.5 false</button>\n",
);

pub struct ValueModel {
    pub title: String,
    pub count: u32,
    pub ratio: f64,
    pub enabled: bool,
    pub note: Option<String>,
    pub trusted: HtmlString,
    pub release: Arc<Notify>,
    pub loads: Arc<AtomicUsize>,
}

impl ValueModel {
    pub fn demo(release: Arc<Notify>, loads: Arc<AtomicUsize>) -> Self {
        Self {
            title: String::from("<Values>"),
            count: 2,
            ratio: -1.5,
            enabled: true,
            note: Some(String::from("<&\"'")),
            trusted: HtmlString::from("<b>trusted</b>"),
            release,
            loads,
        }
    }
}

async_template! {
    pub struct ValuePage { model: ValueModel } => {
        "<h1"
        {attribute("data-count", model.count).unwrap()}
        {attribute("title", &model.title).unwrap()}
        ">" {model.title} {model.enabled} {model.trusted} "</h1>\n"
        @flush;
        @await { model.release.notified() };
        @{ model.loads.fetch_add(1, Ordering::SeqCst); }
        "<button"
        {attribute("checked", model.enabled).unwrap()}
        {attribute("disabled", !model.enabled).unwrap()}
        {attribute("none", None::<&str>).unwrap()}
        {attribute("empty", Some("")).unwrap()}
        {attribute("title", &model.note).unwrap()}
        ">" {model.count} " " {model.ratio} " " {false} "</button>\n"
    }
}
