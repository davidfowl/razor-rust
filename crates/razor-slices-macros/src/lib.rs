//! Code generation behind `razor_slices::html!`.
//!
//! Use the runtime's public macro, which supplies a hygienic runtime path even
//! when the dependency is renamed.

use proc_macro::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{braced, parenthesized, Expr, LitStr, Result};

enum Part {
    Literal(LitStr),
    Value(Expr),
}

struct Template {
    runtime: proc_macro2::TokenStream,
    parts: Vec<Part>,
}

impl Parse for Template {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let runtime;
        parenthesized!(runtime in input);
        let runtime = runtime.parse()?;
        let mut parts = Vec::new();
        while !input.is_empty() {
            if input.peek(LitStr) {
                parts.push(Part::Literal(input.parse()?));
            } else if input.peek(syn::token::Brace) {
                let expression;
                braced!(expression in input);
                let value = expression.parse()?;
                if !expression.is_empty() {
                    return Err(expression.error("expected one Rust expression"));
                }
                parts.push(Part::Value(value));
            } else {
                return Err(input.error("expected a string literal or { Rust expression }"));
            }
        }
        Ok(Self { runtime, parts })
    }
}

/// Implementation detail of the runtime's `html!` macro.
#[proc_macro]
pub fn html(input: TokenStream) -> TokenStream {
    let Template { runtime, parts } = syn::parse_macro_input!(input as Template);
    let writer = proc_macro2::Ident::new("__razor_writer", proc_macro2::Span::mixed_site());
    let writes = parts.into_iter().map(|part| match part {
        Part::Literal(text) => quote! {
            ::std::io::Write::write_all(#writer, #text.as_bytes())?;
        },
        Part::Value(value) => quote! {
            #runtime::WriteHtmlValue::write_html_to(&(#value), #writer)?;
        },
    });
    quote! {
        #runtime::InlineSlice::new(move |#writer: &mut dyn ::std::io::Write|
            -> ::std::io::Result<()> {
            #(#writes)*
            ::std::result::Result::Ok(())
        })
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_literals_and_expressions() {
        let template: Template =
            syn::parse_str(r#"(runtime) "<p>" {name} {1 + 2} r"</p>""#).unwrap();
        assert_eq!(template.parts.len(), 4);
        assert!(syn::parse_str::<Template>("(runtime)").is_ok());
    }

    #[test]
    fn rejects_unsupported_grammar() {
        for input in [
            "(runtime) <p>Hello</p>",
            "(runtime) 42",
            "(runtime) {}",
            "(runtime) {name name}",
            "(runtime) {let name = 1;}",
            "(runtime) \"hello\",",
        ] {
            assert!(syn::parse_str::<Template>(input).is_err(), "{input}");
        }
    }
}
