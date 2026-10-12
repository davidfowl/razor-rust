//! Manual throughput comparison between the current `memchr`-backed HTML
//! encoder scan and the byte-by-byte scalar scan it replaced (see
//! `docs/adr/0005-simd-html-encoding.md`).
//!
//! Deliberately uses only `std::time::Instant` (no benchmarking crate):
//! `criterion`'s dependency tree pulls in a `clap_builder` version that
//! requires Cargo's unstable `edition2024` feature, which breaks this
//! workspace's pinned 1.75 MSRV build (`cargo build --workspace
//! --all-targets` in the `msrv` CI job). Run with `--release` for
//! meaningful numbers:
//!
//! ```sh
//! cargo run -p razor-slices --example html_encode_bench --release
//! ```
use razor_slices::encode_html_to_writer;
use std::time::Instant;

/// The byte-by-byte scalar scan used before adopting `memchr` (ADR 0005),
/// kept here only as a comparison baseline.
fn scalar_encode_html_to_writer(value: &str, out: &mut Vec<u8>) {
    fn needs_escape(b: u8) -> bool {
        matches!(b, b'&' | b'<' | b'>' | b'"' | b'\'')
    }
    fn escape_entity(b: u8) -> &'static str {
        match b {
            b'&' => "&amp;",
            b'<' => "&lt;",
            b'>' => "&gt;",
            b'"' => "&quot;",
            b'\'' => "&#39;",
            _ => unreachable!(),
        }
    }

    let bytes = value.as_bytes();
    let mut start = 0;
    for (i, &b) in bytes.iter().enumerate() {
        if needs_escape(b) {
            if start < i {
                out.extend_from_slice(&bytes[start..i]);
            }
            out.extend_from_slice(escape_entity(b).as_bytes());
            start = i + 1;
        }
    }
    if start < bytes.len() {
        out.extend_from_slice(&bytes[start..]);
    }
}

/// Builds a payload of roughly `len` bytes of plain text with an escaped
/// character inserted every `escape_every` bytes (`0` = no escapes).
fn make_payload(len: usize, escape_every: usize) -> String {
    let mut s = String::with_capacity(len);
    let filler = "the quick brown fox jumps over the lazy dog ";
    let mut filler_iter = filler.chars().cycle();
    while s.len() < len {
        if escape_every > 0 && s.len() % escape_every == 0 {
            s.push('<');
        } else {
            s.push(filler_iter.next().unwrap());
        }
    }
    s
}

fn time_iters(iters: u32, mut f: impl FnMut()) -> std::time::Duration {
    let start = Instant::now();
    for _ in 0..iters {
        f();
    }
    start.elapsed()
}

fn main() {
    let cases: &[(&str, usize, usize)] = &[
        ("short_no_escapes_32b", 32, 0),
        ("short_with_escapes_32b", 32, 8),
        ("long_no_escapes_8kb", 8192, 0),
        ("long_sparse_escapes_8kb", 8192, 512),
        ("long_dense_escapes_8kb", 8192, 8),
    ];

    const ITERS: u32 = 20_000;

    println!(
        "{:<28} {:>14} {:>14} {:>10}",
        "case", "memchr (ms)", "scalar (ms)", "speedup"
    );
    for &(name, len, escape_every) in cases {
        let payload = make_payload(len, escape_every);

        let memchr_time = time_iters(ITERS, || {
            let mut out = Vec::with_capacity(payload.len());
            encode_html_to_writer(&payload, &mut out).unwrap();
            std::hint::black_box(&out);
        });
        let scalar_time = time_iters(ITERS, || {
            let mut out = Vec::with_capacity(payload.len());
            scalar_encode_html_to_writer(&payload, &mut out);
            std::hint::black_box(&out);
        });

        let speedup = scalar_time.as_secs_f64() / memchr_time.as_secs_f64();
        println!(
            "{:<28} {:>14.2} {:>14.2} {:>9.2}x",
            name,
            memchr_time.as_secs_f64() * 1000.0,
            scalar_time.as_secs_f64() * 1000.0,
            speedup
        );
    }
}
