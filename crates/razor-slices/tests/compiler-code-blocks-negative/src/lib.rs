//! Code blocks retain unit-valued tails without hiding preceding bindings.
//! ```
//! use razor_slices::{async_template, AsyncSlice};
//! fn touch() {}
//! async_template! {
//!     struct Page { model: () } => {
//!         @{ let label = "local"; touch() }
//!         {label}
//!     }
//! }
//! fn check(_: &impl AsyncSlice) {}
//! check(&Page::new(()));
//! ```
//!
//! A code block used as a statement cannot have a non-unit tail.
//! ```compile_fail
//! use razor_slices::async_template;
//! async_template! { struct Page { model: () } => { @{ 42 } } }
//! ```
//!
//! Fallible values are not silently discarded without a semicolon.
//! ```compile_fail
//! use razor_slices::async_template;
//! async_template! { struct Page { model: () } => {
//!     @{ Ok::<(), std::io::Error>(()) }
//! } }
//! ```
//!
//! A future tail must be awaited, not silently dropped.
//! ```compile_fail
//! use razor_slices::async_template;
//! async_template! { struct Page { model: () } => { @{ async {} } } }
//! ```
