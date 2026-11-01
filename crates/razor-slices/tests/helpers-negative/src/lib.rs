//! Compile contracts for helper, method and delegate composition (#101/#104).
//!
//! Positive proof: a helper returns a slice and is embedded explicitly.
//! ```
//! use razor_slices::{html, partial, Slice};
//! fn badge(label: &str) -> impl Slice + '_ { html! { "<b>" {label} "</b>" } }
//! let page = html! { {partial(badge("<x>"))} };
//! assert_eq!(page.render_to_string().unwrap(), "<b>&lt;x&gt;</b>");
//! ```
//!
//! A helper's output is not interpolable without the explicit `partial`.
//! ```compile_fail
//! use razor_slices::{html, Slice};
//! fn badge() -> impl Slice { html! { "<b>x</b>" } }
//! let _ = html! { {badge()} };
//! ```
//!
//! Helpers are typed: the wrong argument type is rejected.
//! ```compile_fail
//! use razor_slices::{html, Slice};
//! fn badge(count: u32) -> impl Slice { html! { {count} } }
//! let _ = badge("many");
//! ```
//!
//! A helper returning plain text is not a slice (no implicit trusted HTML).
//! ```compile_fail
//! use razor_slices::partial;
//! fn badge() -> String { "<b>x</b>".to_owned() }
//! let _ = partial(badge());
//! ```
//!
//! A delegate bound to a single closure type cannot return different slice types.
//! ```compile_fail
//! use razor_slices::html;
//! let delegate = |flag: bool| if flag { html! { "a" } } else { html! { "b" } };
//! let _ = delegate(true);
//! ```
//!
//! A reusable parent cannot consume a one-shot delegate result on each render.
//! ```compile_fail
//! use razor_slices::{html, partial};
//! let delegate = |n: u32| html! { {n} };
//! let child = delegate(1);
//! let _ = html! { {partial(child)} };
//! ```
//!
//! A delegate result cannot outlive the value it borrows.
//! ```compile_fail
//! use razor_slices::{html, Slice};
//! let delegate = |text: &str| html! { {text} };
//! let slice;
//! {
//!     let owned = String::from("short");
//!     slice = delegate(&owned);
//! }
//! let _ = slice.render_to_string();
//! ```
//!
//! There is no helper-declaration directive in `html!`.
//! ```compile_fail
//! use razor_slices::html;
//! let _ = html! { @helper badge() { "x" } };
//! ```
//!
//! There is no inline delegate literal in template bodies.
//! ```compile_fail
//! use razor_slices::html;
//! let _ = html! { @{ let f = @<b>"x"</b>; } };
//! ```
