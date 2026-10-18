//! Render-local synchronous block type, scope and capture contracts.
//!
//! A unit tail retains preceding bindings and mutations for later parts.
//! ```
//! use razor_slices::{html, Slice};
//! fn touch() {}
//! let page = html! {
//!     @{ let mut local = String::from("<local>"); touch() }
//!     @{ local.push('&'); }
//!     {local}
//! };
//! for _ in 0..2 {
//!     assert_eq!(page.render_to_string().unwrap(), "&lt;local&gt;&amp;");
//! }
//! ```
//!
//! Integer tails are not silently discarded.
//! ```compile_fail
//! use razor_slices::html;
//! let _ = html! { @{ 42 } };
//! ```
//!
//! Result tails are not silently discarded.
//! ```compile_fail
//! use razor_slices::html;
//! let _ = html! { @{ Ok::<(), std::io::Error>(()) } };
//! ```
//!
//! Future tails are not silently discarded.
//! ```compile_fail
//! use razor_slices::html;
//! let _ = html! { @{ async {} } };
//! ```
//!
//! Synchronous blocks do not make Rust awaits legal.
//! ```compile_fail
//! use razor_slices::html;
//! let _ = html! { @{ async {}.await; } };
//! ```
//!
//! Async-only template directives remain rejected.
//! ```compile_fail
//! use razor_slices::html;
//! let _ = html! { @{} @flush; };
//! ```
//!
//! Owned captures cannot be consumed by a reusable Fn renderer.
//! ```compile_fail
//! use razor_slices::html;
//! let owned = String::from("owned");
//! let _ = html! { @{ drop(owned); } };
//! ```
//!
//! A branch-local binding does not escape its branch.
//! ```compile_fail
//! use razor_slices::html;
//! let _ = html! { if true { @{ let label = "branch"; } } {label} };
//! ```
//!
//! A borrowed capture still cannot escape its source lifetime.
//! ```compile_fail
//! use razor_slices::{html, Slice};
//! let page;
//! {
//!     let owned = String::from("borrowed");
//!     let borrowed = &owned;
//!     page = html! { @{ let label = borrowed.as_str(); } {label} };
//! }
//! let _ = page.render_to_string();
//! ```
