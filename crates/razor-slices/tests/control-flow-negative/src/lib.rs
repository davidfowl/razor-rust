//! Negative contract tests for template control flow.
//!
//! The same fixture must also compile and render valid nested syntax.
//! ```
//! use razor_slices::{html, Slice};
//! let values = vec!["<&>"];
//! let page = html! { for value in &values { if !value.is_empty() { {value} } } };
//! assert_eq!(page.render_to_string().unwrap(), "&lt;&amp;&gt;");
//! ```
//!
//! Missing `in` is not accepted as literal output.
//! ```compile_fail
//! use razor_slices::html;
//! let _ = html! { for value [1, 2] { {value} } };
//! ```
//!
//! Invalid nested bodies are rejected.
//! ```compile_fail
//! use razor_slices::html;
//! let _ = html! { if true { for value in [1] { 42 } } };
//! ```
//!
//! `else` requires a template block or `if`.
//! ```compile_fail
//! use razor_slices::html;
//! let _ = html! { if true {} else false {} };
//! ```
//!
//! Rust checks the condition's type.
//! ```compile_fail
//! use razor_slices::html;
//! let _ = html! { if 1 { "not boolean" } };
//! ```
//!
//! Rust checks the iterator's type.
//! ```compile_fail
//! use razor_slices::html;
//! let _ = html! { for value in 42 { {value} } };
//! ```
//!
//! Iteration cannot consume an owned capture in a reusable renderer.
//! ```compile_fail
//! use razor_slices::html;
//! let values = vec![String::from("owned")];
//! let _ = html! { for value in values { {value} } };
//! ```
//!
//! Borrowed loop models cannot escape their referent's scope.
//! ```compile_fail
//! use razor_slices::{html, Slice};
//! let page;
//! {
//!     let values = vec![String::from("borrowed")];
//!     let borrowed = &values;
//!     page = html! { for value in borrowed { {value} } };
//! }
//! let _ = page.render_to_string();
//! ```
//!
//! Unselected branch values are still type checked.
//! ```compile_fail
//! use razor_slices::html;
//! struct Unsupported;
//! let _ = html! { if false { {Unsupported} } };
//! ```
