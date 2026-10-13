//! Code generation behind `razor_slices::html!`.
//!
//! Use the runtime's public macro, which supplies a hygienic runtime path even
//! when the dependency is renamed.

use proc_macro::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{braced, parenthesized, Expr, LitStr, Pat, Result, Token};

enum Part {
    Literal(LitStr),
    Value(Expr),
    If {
        condition: Expr,
        body: Vec<Part>,
        otherwise: Vec<Part>,
    },
    For {
        pattern: Pat,
        iterator: Expr,
        body: Vec<Part>,
    },
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
        let parts = parse_parts(input)?;
        Ok(Self { runtime, parts })
    }
}

fn parse_body(input: ParseStream<'_>) -> Result<Vec<Part>> {
    let body;
    braced!(body in input);
    parse_parts(&body)
}

fn parse_if(input: ParseStream<'_>) -> Result<Part> {
    input.parse::<Token![if]>()?;
    let condition = input.call(Expr::parse_without_eager_brace)?;
    let body = parse_body(input)?;
    let otherwise = if input.peek(Token![else]) {
        input.parse::<Token![else]>()?;
        if input.peek(Token![if]) {
            vec![parse_if(input)?]
        } else {
            parse_body(input)?
        }
    } else {
        Vec::new()
    };
    Ok(Part::If {
        condition,
        body,
        otherwise,
    })
}

fn parse_parts(input: ParseStream<'_>) -> Result<Vec<Part>> {
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
        } else if input.peek(Token![if]) {
            parts.push(parse_if(input)?);
        } else if input.peek(Token![for]) {
            input.parse::<Token![for]>()?;
            let pattern = input.call(Pat::parse_single)?;
            input.parse::<Token![in]>()?;
            let iterator = input.call(Expr::parse_without_eager_brace)?;
            let body = parse_body(input)?;
            parts.push(Part::For {
                pattern,
                iterator,
                body,
            });
        } else {
            return Err(input.error("expected a string literal, { Rust expression }, if, or for"));
        }
    }
    Ok(parts)
}

fn generate_parts(
    parts: Vec<Part>,
    runtime: &proc_macro2::TokenStream,
    writer: &proc_macro2::Ident,
) -> proc_macro2::TokenStream {
    let writes = parts.into_iter().map(|part| match part {
        Part::Literal(text) => quote! {
            ::std::io::Write::write_all(#writer, #text.as_bytes())?;
        },
        Part::Value(value) => quote! {
            #runtime::WriteHtmlValue::write_html_to(&(#value), #writer)?;
        },
        Part::If {
            condition,
            body,
            otherwise,
        } => {
            let body = generate_parts(body, runtime, writer);
            let otherwise = generate_parts(otherwise, runtime, writer);
            quote! { if #condition { #body } else { #otherwise } }
        }
        Part::For {
            pattern,
            iterator,
            body,
        } => {
            let body = generate_parts(body, runtime, writer);
            quote! { for #pattern in #iterator { #body } }
        }
    });
    quote! { #(#writes)* }
}

/// Implementation detail of the runtime's `html!` macro.
#[proc_macro]
pub fn html(input: TokenStream) -> TokenStream {
    let Template { runtime, parts } = syn::parse_macro_input!(input as Template);
    let writer = proc_macro2::Ident::new("__razor_writer", proc_macro2::Span::mixed_site());
    let writes = generate_parts(parts, &runtime, &writer);
    quote! {
        #runtime::InlineSlice::new(move |#writer: &mut dyn ::std::io::Write|
            -> ::std::io::Result<()> {
            #writes
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

    #[test]
    fn parses_nested_control_flow_and_rust_headers() {
        for input in [
            r#"(runtime) if true { for (a, b) in &pairs { if a == b { {a} } else if false { "x" } else {} } }"#,
            r#"(runtime) if let Some(value) = item { {value} }"#,
            r#"(runtime) for value in (Collection { values: vec![] }) { {value} }"#,
            r#"(runtime) if (Thing { flag: true }).flag {}"#,
        ] {
            assert!(syn::parse_str::<Template>(input).is_ok(), "{input}");
        }
    }

    #[test]
    fn rejects_invalid_control_flow() {
        for input in [
            "(runtime) else {}",
            "(runtime) if {}",
            "(runtime) if true",
            "(runtime) if true {} else false {}",
            "(runtime) for item items {}",
            "(runtime) for in items {}",
            "(runtime) for item in {}",
            "(runtime) if true { for item in items { 42 } }",
            "(runtime) if true {} else {} else {}",
            "(runtime) for item in items {} else {}",
        ] {
            assert!(syn::parse_str::<Template>(input).is_err(), "{input}");
        }
    }
}
