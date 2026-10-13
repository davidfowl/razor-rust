//! Positive control: borrowed models, bindings, await and nested syntax compile.
//! ```
//! use razor_slices::{async_template, AsyncSlice};
//! async_template! {
//!     pub struct Page<'a> { model: &'a [&'a str] } => {
//!         @{ let enabled = true; }
//!         if enabled {
//!             for value in *model {
//!                 @await { async {} };
//!                 {value}
//!             }
//!         }
//!         @flush;
//!     }
//! }
//! fn check(_: &impl AsyncSlice) {}
//! check(&Page::new(&["<&>"]));
//! ```
//!
//! Declaration requires exactly one named model field.
//! ```compile_fail
//! use razor_slices::async_template;
//! async_template! { struct Page { other: String } => {} }
//! ```
//!
//! Additional fields are not implicit captures.
//! ```compile_fail
//! use razor_slices::async_template;
//! async_template! { struct Page { model: String, extra: String } => {} }
//! ```
//!
//! Arrow is mandatory.
//! ```compile_fail
//! use razor_slices::async_template;
//! async_template! { struct Page { model: String } {} }
//! ```
//!
//! Await futures are braced.
//! ```compile_fail
//! use razor_slices::async_template;
//! async_template! { struct Page { model: String } => { @await async {}; } }
//! ```
//!
//! Flush requires a semicolon.
//! ```compile_fail
//! use razor_slices::async_template;
//! async_template! { struct Page { model: String } => { @flush } }
//! ```
//!
//! Section provider and name are separate braced expressions.
//! ```compile_fail
//! use razor_slices::async_template;
//! async_template! { struct Page { model: String } => { @section { model }; } }
//! ```
//!
//! Nested grammar remains checked.
//! ```compile_fail
//! use razor_slices::async_template;
//! async_template! { struct Page { model: String } => { if true { 42 } } }
//! ```
//!
//! The constructor's model type is not inferred from arbitrary input.
//! ```compile_fail
//! use razor_slices::async_template;
//! async_template! { struct Page { model: String } => { {model} } }
//! let _ = Page::new(42);
//! ```
//!
//! Model fields are statically checked even in unselected branches.
//! ```compile_fail
//! use razor_slices::async_template;
//! async_template! { struct Page { model: String } => { if false { {model.missing} } } }
//! ```
//!
//! Shared models must be Sync.
//! ```compile_fail
//! use razor_slices::async_template;
//! use std::rc::Rc;
//! async_template! { struct Page { model: Rc<String> } => { {model.as_str()} } }
//! ```
//!
//! Render-local values held across suspension must make the future Send.
//! ```compile_fail
//! use razor_slices::async_template;
//! async_template! { struct Page { model: String } => {
//!     @{ let local = std::rc::Rc::new(String::from("local")); }
//!     @await { async {} };
//!     {local.as_str()}
//! } }
//! ```
//!
//! Rendering cannot consume an owned model.
//! ```compile_fail
//! use razor_slices::async_template;
//! async_template! { struct Page { model: String } => { @{ drop(*model); } } }
//! ```
//!
//! A borrowed model cannot escape its owner's lifetime.
//! ```compile_fail
//! use razor_slices::async_template;
//! async_template! { struct Page<'a> { model: &'a str } => { {model} } }
//! let page;
//! {
//!     let name = String::from("borrowed");
//!     page = Page::new(&name);
//! }
//! drop(page);
//! ```
//!
//! A synchronous child cannot masquerade as an async template.
//! ```compile_fail
//! use razor_slices::async_template;
//! async_template! { struct Page { model: String } => { @render { razor_slices::html! { "sync" } }; } }
//! ```
//!
//! Value-producing futures require an explicit binding or interpolation.
//! ```compile_fail
//! use razor_slices::async_template;
//! async_template! { struct Page { model: String } => { @await { async { "unused" } }; } }
//! ```
//!
//! Fallible awaits cannot silently discard their Result.
//! ```compile_fail
//! use razor_slices::async_template;
//! async_template! { struct Page { model: String } => {
//!     @await { async { Ok::<(), std::io::Error>(()) } };
//! } }
//! ```
//!
//! Propagating await requires a Result with convertible error.
//! ```compile_fail
//! use razor_slices::async_template;
//! async_template! { struct Page { model: String } => { @await { async {} }?; } }
//! ```
//!
//! Application errors must convert to the runtime error contract.
//! ```compile_fail
//! use razor_slices::async_template;
//! async_template! { struct Page { model: String } => {
//!     @await { async { Err::<(), _>("unsupported error") } }?;
//! } }
//! ```
//!
//! Async values must be strings, with no silent Display/raw fallback.
//! ```compile_fail
//! use razor_slices::async_template;
//! async_template! { struct Page { model: u32 } => { {model} } }
//! ```
//!
//! Synchronous html! does not acquire async directives.
//! ```compile_fail
//! use razor_slices::html;
//! let _ = html! { @flush; };
//! ```
//!
//! Declarations cannot capture surrounding dynamic values.
//! ```compile_fail
//! use razor_slices::async_template;
//! let captured = String::from("captured");
//! async_template! { struct Page { model: String } => { {captured} } }
//! ```
//!
//! Future fields cannot be consumed by reusable rendering.
//! ```compile_fail
//! use razor_slices::async_template;
//! use std::future::Ready;
//! async_template! { struct Page { model: Ready<()> } => { @await { *model }; } }
//! ```
//!
//! Conditions and iterators retain Rust type checking.
//! ```compile_fail
//! use razor_slices::async_template;
//! async_template! { struct Page { model: String } => { if 1 { "bad" } } }
//! ```
//!
//! Invalid iteration does not become markup.
//! ```compile_fail
//! use razor_slices::async_template;
//! async_template! { struct Page { model: String } => { for value in 42 { {value} } } }
//! ```
//!
//! Conditional compilation belongs on the whole invocation.
//! ```compile_fail
//! use razor_slices::async_template;
//! async_template! { #[cfg(any())] struct Page { model: String } => {} }
//! ```
//!
//! Code-block application futures retain Rust's must-use diagnostics.
//! ```compile_fail
//! #![deny(unused_must_use)]
//! use razor_slices::async_template;
//! async_template! { struct Page { model: String } => { @{ async {}; } } }
//! ```
