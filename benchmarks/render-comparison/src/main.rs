use askama::Template;
use razor_slices::Slice;
use serde::{Deserialize, Serialize};
use std::alloc::{GlobalAlloc, Layout, System};
use std::error::Error;
use std::hint::black_box;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::time::Instant;

struct CountingAllocator;
static COUNTING: AtomicBool = AtomicBool::new(false);
static EVENTS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);

fn record(pointer: *mut u8, bytes: usize) {
    if !pointer.is_null() && COUNTING.load(Relaxed) {
        EVENTS.fetch_add(1, Relaxed);
        BYTES.fetch_add(bytes as u64, Relaxed);
    }
}

// Count successful allocation requests, not live memory or native allocator metadata.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = System.alloc(layout);
        record(pointer, layout.size());
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = System.alloc_zeroed(layout);
        record(pointer, layout.size());
        pointer
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let pointer = System.realloc(pointer, layout, size);
        record(pointer, size);
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        System.dealloc(pointer, layout);
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn calibrate_allocator() -> Result<(), Box<dyn Error>> {
    let initial = Layout::from_size_align(64, 8)?;
    let grown = Layout::from_size_align(128, 8)?;
    let zeroed = Layout::from_size_align(32, 8)?;
    EVENTS.store(0, Relaxed);
    BYTES.store(0, Relaxed);
    COUNTING.store(true, Relaxed);
    // Exercise all counted entry points with known sizes; keep pointers observable.
    unsafe {
        let pointer = black_box(std::alloc::alloc(initial));
        if pointer.is_null() {
            std::alloc::handle_alloc_error(initial);
        }
        let resized = black_box(std::alloc::realloc(pointer, initial, grown.size()));
        if resized.is_null() {
            std::alloc::handle_alloc_error(grown);
        }
        let cleared = black_box(std::alloc::alloc_zeroed(zeroed));
        if cleared.is_null() {
            std::alloc::handle_alloc_error(zeroed);
        }
        std::alloc::dealloc(resized, grown);
        std::alloc::dealloc(cleared, zeroed);
    }
    COUNTING.store(false, Relaxed);
    if EVENTS.load(Relaxed) != 3 || BYTES.load(Relaxed) != 224 {
        return Err(
            "allocator calibration failed: expected 3 events and 224 requested bytes".into(),
        );
    }
    Ok(())
}

#[derive(Clone, Deserialize, Serialize)]
struct Item {
    name: String,
    detail: String,
}

#[derive(Deserialize)]
struct Fixture {
    title: String,
    repeat: usize,
    items: Vec<Item>,
}

#[derive(Template)]
#[template(path = "askama-list.html", escape = "html")]
struct AskamaList<'a> {
    title: &'a str,
    items: &'a [Item],
}

type Render<'a> = Box<dyn Fn() -> Result<String, Box<dyn Error>> + 'a>;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 7 || !matches!(args[1].as_str(), "verify" | "measure") {
        return Err(
            "usage: render-comparison verify|measure FIXTURE EXPECTED WARMUP SAMPLES ITERATIONS"
                .into(),
        );
    }
    if cfg!(debug_assertions) {
        return Err("use cargo build --release; debug measurements are forbidden".into());
    }
    calibrate_allocator()?;
    let fixture: Fixture = serde_json::from_slice(&std::fs::read(&args[2])?)?;
    if fixture.repeat == 0 || fixture.repeat > 1024 || fixture.items.is_empty() {
        return Err("fixture must have items and repeat in 1..=1024".into());
    }
    let items: Vec<Item> = (0..fixture.repeat)
        .flat_map(|_| fixture.items.iter().cloned())
        .collect();
    let title = fixture.title.as_str();
    for text in std::iter::once(title).chain(
        items
            .iter()
            .flat_map(|item| [item.name.as_str(), item.detail.as_str()]),
    ) {
        if !text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b" &<>".contains(&byte))
        {
            return Err(
                "fixture exceeds common encoder subset: ASCII letters, digits, spaces and &<>"
                    .into(),
            );
        }
    }
    let expected = std::fs::read(&args[3])?;
    let warmup: usize = args[4].parse()?;
    let samples: usize = args[5].parse()?;
    let iterations: usize = args[6].parse()?;
    if warmup == 0 || samples == 0 || iterations == 0 {
        return Err("protocol counts must be positive".into());
    }

    let borrowed_items = items.as_slice();
    let razor = razor_slices::html! {
        "<h1>" {title} "</h1><ul>"
        for item in borrowed_items {
            "<li><span>" {item.name.as_str()} "</span><p>" {item.detail.as_str()} "</p></li>"
        }
        "</ul>\n"
    };
    let askama = AskamaList {
        title,
        items: &items,
    };
    let mut tera = tera::Tera::default();
    tera.add_raw_template("list.html", include_str!("../list.html"))?;
    let mut context = tera::Context::new();
    context.insert("title", title);
    context.insert("items", &items);
    let renderers: Vec<(&str, Render<'_>)> = vec![
        ("razor-rust", Box::new(|| Ok(razor.render_to_string()?))),
        ("askama", Box::new(|| Ok(askama.render()?))),
        (
            "maud",
            Box::new(|| {
                Ok(maud::html! {
                    h1 { (title) }
                    ul {
                        @for item in &items {
                            li { span { (item.name) } p { (item.detail) } }
                        }
                    }
                    "\n"
                }
                .into_string())
            }),
        ),
        ("tera", Box::new(|| Ok(tera.render("list.html", &context)?))),
    ];

    let mut mismatch = false;
    for (engine, render) in &renderers {
        let mut equal = true;
        for attempt in 1..=2 {
            let actual = render()?;
            if actual.as_bytes() != expected {
                eprintln!("output mismatch: {engine}, attempt {attempt}, expected {} bytes, actual {} bytes", expected.len(), actual.len());
                mismatch = true;
                equal = false;
            }
        }
        println!(
            "{}",
            serde_json::json!({
                "kind": "gate", "engine": engine, "equal": equal,
                "output_bytes": expected.len()
            })
        );
    }
    if mismatch {
        return Err("output equality gate failed; no measurements performed".into());
    }
    if args[1] == "verify" {
        return Ok(());
    }

    for (engine, render) in &renderers {
        for _ in 0..warmup {
            black_box(render()?);
        }
        for sample in 0..samples {
            let start = Instant::now();
            for _ in 0..iterations {
                black_box(render()?);
            }
            let elapsed = start.elapsed();
            EVENTS.store(0, Relaxed);
            BYTES.store(0, Relaxed);
            COUNTING.store(true, Relaxed);
            let allocations = (|| -> Result<(), Box<dyn Error>> {
                for _ in 0..iterations {
                    black_box(render()?);
                }
                Ok(())
            })();
            COUNTING.store(false, Relaxed);
            allocations?;
            let events = EVENTS.load(Relaxed);
            let bytes = BYTES.load(Relaxed);
            println!(
                "{}",
                serde_json::json!({
                    "kind": "sample", "engine": engine, "sample": sample,
                    "iterations": iterations, "elapsed_ns": elapsed.as_nanos() as u64,
                    "renders_per_second": iterations as f64 / elapsed.as_secs_f64(),
                    "output_bytes": expected.len(),
                    "allocation_events": events, "allocated_bytes": bytes,
                    "allocation_events_per_render": events as f64 / iterations as f64,
                    "allocated_bytes_per_render": bytes as f64 / iterations as f64
                })
            );
        }
    }
    Ok(())
}
