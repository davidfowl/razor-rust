//! Native match type/scope contracts for streamed template arms.
//!
//! Positive proof: leading or patterns, guards, typed bindings and empty arms.
//! ```
//! use razor_slices::{html, Slice};
//! let page = html! {
//!     match Some(2) {
//!         | Some(value @ (1 | 2)) if value > 0 => { {value} }
//!         _ => {}
//!     }
//! };
//! assert_eq!(page.render_to_string().unwrap(), "2");
//! ```
//!
//! A match remains exhaustive.
//! ```compile_fail
//! use razor_slices::html;
//! let _ = html! { match Some(1) { None => {} } };
//! ```
//!
//! Guards do not establish exhaustiveness.
//! ```compile_fail
//! use razor_slices::html;
//! let _ = html! { match true { value if value => {} } };
//! ```
//!
//! Pattern types are checked against the scrutinee.
//! ```compile_fail
//! use razor_slices::html;
//! let _ = html! { match 1 { Some(value) => { {value} } _ => {} } };
//! ```
//!
//! Guards must return bool.
//! ```compile_fail
//! use razor_slices::html;
//! let _ = html! { match 1 { value if value => {} _ => {} } };
//! ```
//!
//! Bindings do not escape an arm.
//! ```compile_fail
//! use razor_slices::html;
//! let _ = html! { match Some(1) { Some(value) => {} None => {} } {value} };
//! ```
//!
//! An arm cannot consume an owned capture of the reusable renderer.
//! ```compile_fail
//! use razor_slices::html;
//! let owned = Some(String::from("owned"));
//! let _ = html! { match owned { Some(value) => { {value} } None => {} } };
//! ```
//!
//! Arms require braced template bodies, not ordinary value expressions.
//! ```compile_fail
//! use razor_slices::html;
//! let _ = html! { match 1 { _ => "unbraced" } };
//! ```
//!
//! Async-only directives remain illegal in synchronous arms.
//! ```compile_fail
//! use razor_slices::html;
//! let _ = html! { match () { () => { @flush; } } };
//! ```
//!
//! Async templates also preserve native exhaustiveness.
//! ```compile_fail
//! use razor_slices::async_template;
//! async_template! { struct Page { model: bool } => { match model { true => {} } } }
//! ```
