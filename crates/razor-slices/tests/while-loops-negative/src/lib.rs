//! While-loop type, scope and reusable capture contracts.
//!
//! Valid conditions and bindings compile in both macro forms.
//! ```
//! use razor_slices::{async_template, html, Slice};
//! use std::cell::Cell;
//! let count = Cell::new(0);
//! let count = &count;
//! let page = html! {
//!     while count.get() < 2 {
//!         {{ let value = count.get(); count.set(value + 1); value }}
//!     }
//! };
//! assert_eq!(page.render_to_string().unwrap(), "01");
//! async_template! {
//!     struct Page { model: usize } => {
//!         @{ let mut next = Some(0); }
//!         while let Some(value) = next {
//!             {value}
//!             @{ next = None; }
//!         }
//!     }
//! }
//! let _ = Page::new(0);
//! ```
//!
//! A missing condition is not literal output.
//! ```compile_fail
//! use razor_slices::html;
//! let _ = html! { while { "missing condition" } };
//! ```
//!
//! Synchronous conditions must be boolean.
//! ```compile_fail
//! use razor_slices::html;
//! let _ = html! { while 1 { "not boolean" } };
//! ```
//!
//! Pattern bindings do not escape their loop body.
//! ```compile_fail
//! use razor_slices::html;
//! let _ = html! { while let Some(value) = None::<u8> { {value} } {value} };
//! ```
//!
//! Unselected loop bodies are still type checked.
//! ```compile_fail
//! use razor_slices::html;
//! struct Unsupported;
//! let _ = html! { while false { {Unsupported} } };
//! ```
//!
//! A reusable synchronous template cannot move an owned capture into a pattern.
//! ```compile_fail
//! use razor_slices::html;
//! let owned = Some(String::from("owned"));
//! let _ = html! { while let Some(value) = owned { {value} } };
//! ```
//!
//! Asynchronous conditions also retain Rust type checking.
//! ```compile_fail
//! use razor_slices::async_template;
//! async_template! { struct Page { model: () } => { while 1 { "not boolean" } } }
//! ```
