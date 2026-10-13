use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use razor_slices::{async_template, with_async_section, AsyncSlice, HtmlString};
use tokio::sync::Notify;

pub const PREFIX_BYTES: usize = 16_384;
pub const SUFFIX: &str = "<p>&lt;\u{96ea} &amp; ready&gt;</p>\n";

pub struct Model {
    pub prefix: HtmlString,
    pub release: Arc<Notify>,
    pub completed: Arc<AtomicUsize>,
}

impl Model {
    pub fn new(release: Arc<Notify>, completed: Arc<AtomicUsize>) -> Self {
        Self {
            prefix: HtmlString::from("x".repeat(PREFIX_BYTES)),
            release,
            completed,
        }
    }
}

async_template! {
    struct Prefix<'a> { model: &'a HtmlString } => { {model} }
}

async_template! {
    pub struct Page { model: Model } => {
        @render { Prefix::new(&model.prefix) };
        @await { model.release.notified() };
        @{ model.completed.fetch_add(1, Ordering::SeqCst); }
        "<p>" {"<\u{96ea} & ready>"} "</p>\n"
    }
}

pub fn section_page(model: Model) -> impl AsyncSlice {
    with_async_section(Empty::new(()), "content", Page::new(model)).unwrap()
}

async_template! {
    struct Empty { model: () } => { }
}

async_template! {
    pub struct SectionPage<S: AsyncSlice> { model: S } => {
        @section { model }, { "content" };
    }
}
