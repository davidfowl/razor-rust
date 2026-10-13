//! Code generation behind `razor_slices::html!` and `async_template!`.
//!
//! Use the runtime's public macro, which supplies a hygienic runtime path even
//! when the dependency is renamed.

use proc_macro::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{
    braced, parenthesized, Block, Expr, Fields, ItemStruct, LitStr, Pat, Result, Stmt, Token,
};

mod directive {
    syn::custom_keyword!(flush);
    syn::custom_keyword!(render);
    syn::custom_keyword!(section);
}

#[derive(Clone, Copy)]
enum Syntax {
    Sync,
    Async,
}

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
    Code(Vec<Stmt>),
    Await {
        future: Expr,
        propagate: bool,
    },
    Flush,
    Render(Expr),
    Section {
        provider: Expr,
        name: Expr,
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
        let parts = parse_parts(input, Syntax::Sync)?;
        Ok(Self { runtime, parts })
    }
}

fn parse_body(input: ParseStream<'_>, syntax: Syntax) -> Result<Vec<Part>> {
    let body;
    braced!(body in input);
    parse_parts(&body, syntax)
}

fn parse_if(input: ParseStream<'_>, syntax: Syntax) -> Result<Part> {
    input.parse::<Token![if]>()?;
    let condition = input.call(Expr::parse_without_eager_brace)?;
    let body = parse_body(input, syntax)?;
    let otherwise = if input.peek(Token![else]) {
        input.parse::<Token![else]>()?;
        if input.peek(Token![if]) {
            vec![parse_if(input, syntax)?]
        } else {
            parse_body(input, syntax)?
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

fn parse_expression(input: ParseStream<'_>) -> Result<Expr> {
    let expression;
    braced!(expression in input);
    let value = expression.parse()?;
    if !expression.is_empty() {
        return Err(expression.error("expected one Rust expression"));
    }
    Ok(value)
}

fn parse_directive(input: ParseStream<'_>) -> Result<Part> {
    input.parse::<Token![@]>()?;
    if input.peek(syn::token::Brace) {
        let statements;
        braced!(statements in input);
        return Ok(Part::Code(statements.call(Block::parse_within)?));
    }
    let part = if input.peek(Token![await]) {
        input.parse::<Token![await]>()?;
        let future = parse_expression(input)?;
        let propagate = input.peek(Token![?]);
        if propagate {
            input.parse::<Token![?]>()?;
        }
        Part::Await { future, propagate }
    } else if input.peek(directive::flush) {
        input.parse::<directive::flush>()?;
        Part::Flush
    } else if input.peek(directive::render) {
        input.parse::<directive::render>()?;
        Part::Render(parse_expression(input)?)
    } else if input.peek(directive::section) {
        input.parse::<directive::section>()?;
        let provider = parse_expression(input)?;
        input.parse::<Token![,]>()?;
        let name = parse_expression(input)?;
        Part::Section { provider, name }
    } else {
        return Err(input.error("expected Rust block, await, flush, render, or section after @"));
    };
    input.parse::<Token![;]>()?;
    Ok(part)
}

fn parse_parts(input: ParseStream<'_>, syntax: Syntax) -> Result<Vec<Part>> {
    let mut parts = Vec::new();
    while !input.is_empty() {
        if input.peek(LitStr) {
            parts.push(Part::Literal(input.parse()?));
        } else if input.peek(syn::token::Brace) {
            parts.push(Part::Value(parse_expression(input)?));
        } else if input.peek(Token![if]) {
            parts.push(parse_if(input, syntax)?);
        } else if input.peek(Token![for]) {
            input.parse::<Token![for]>()?;
            let pattern = input.call(Pat::parse_single)?;
            input.parse::<Token![in]>()?;
            let iterator = input.call(Expr::parse_without_eager_brace)?;
            let body = parse_body(input, syntax)?;
            parts.push(Part::For {
                pattern,
                iterator,
                body,
            });
        } else if matches!(syntax, Syntax::Async) && input.peek(Token![@]) {
            parts.push(parse_directive(input)?);
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
    syntax: Syntax,
) -> proc_macro2::TokenStream {
    let writes = parts.into_iter().map(|part| match part {
        Part::Literal(text) => match syntax {
            Syntax::Sync => quote! { ::std::io::Write::write_all(#writer, #text.as_bytes())?; },
            Syntax::Async => quote! { #writer.write_literal(#text).await?; },
        },
        Part::Value(value) => match syntax {
            Syntax::Sync => quote! {
                #runtime::WriteHtmlValue::write_html_to(&(#value), #writer)?;
            },
            Syntax::Async => quote! { #writer.write_html_value(&(#value)).await?; },
        },
        Part::If {
            condition,
            body,
            otherwise,
        } => {
            let body = generate_parts(body, runtime, writer, syntax);
            let otherwise = generate_parts(otherwise, runtime, writer, syntax);
            quote! { if #condition { #body } else { #otherwise } }
        }
        Part::For {
            pattern,
            iterator,
            body,
        } => {
            let body = generate_parts(body, runtime, writer, syntax);
            quote! { for #pattern in #iterator { #body } }
        }
        Part::Code(statements) => quote! { #(#statements)* },
        Part::Await { future, propagate } => {
            let question = propagate.then(|| quote! { ? });
            quote! { let (): () = (#future).await #question; }
        }
        Part::Flush => quote! { #writer.flush().await?; },
        Part::Render(child) => quote! {
            #writer.write_slice(&(#child)).await?;
        },
        Part::Section { provider, name } => quote! {
            #writer.write_section(&(#provider), &(#name)).await?;
        },
    });
    quote! { #(#writes)* }
}

/// Implementation detail of the runtime's `html!` macro.
#[proc_macro]
pub fn html(input: TokenStream) -> TokenStream {
    let Template { runtime, parts } = syn::parse_macro_input!(input as Template);
    let writer = proc_macro2::Ident::new("__razor_writer", proc_macro2::Span::mixed_site());
    let writes = generate_parts(parts, &runtime, &writer, Syntax::Sync);
    quote! {
        #runtime::InlineSlice::new(move |#writer: &mut dyn ::std::io::Write|
            -> ::std::io::Result<()> {
            #writes
            ::std::result::Result::Ok(())
        })
    }
    .into()
}

struct AsyncTemplate {
    runtime: proc_macro2::TokenStream,
    declaration: ItemStruct,
    parts: Vec<Part>,
}

impl Parse for AsyncTemplate {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let runtime;
        parenthesized!(runtime in input);
        let runtime = runtime.parse()?;
        let declaration: ItemStruct = input.parse()?;
        match &declaration.fields {
            Fields::Named(fields)
                if fields.named.len() == 1
                    && fields.named[0]
                        .ident
                        .as_ref()
                        .is_some_and(|name| name == "model") => {}
            _ => {
                return Err(syn::Error::new_spanned(
                    &declaration,
                    "expected a struct with exactly one named model field",
                ));
            }
        }
        for attribute in declaration
            .attrs
            .iter()
            .chain(declaration.fields.iter().flat_map(|field| &field.attrs))
        {
            if attribute.path().is_ident("cfg") || attribute.path().is_ident("cfg_attr") {
                return Err(syn::Error::new_spanned(
                    attribute,
                    "apply conditional compilation to the async_template! invocation",
                ));
            }
        }
        input.parse::<Token![=>]>()?;
        let parts = parse_body(input, Syntax::Async)?;
        if !input.is_empty() {
            return Err(input.error("unexpected tokens after async template body"));
        }
        Ok(Self {
            runtime,
            declaration,
            parts,
        })
    }
}

/// Implementation detail of the runtime's `async_template!` declaration macro.
#[proc_macro]
pub fn async_template(input: TokenStream) -> TokenStream {
    let AsyncTemplate {
        runtime,
        declaration,
        parts,
    } = syn::parse_macro_input!(input as AsyncTemplate);
    let name = &declaration.ident;
    let visibility = &declaration.vis;
    let field = declaration
        .fields
        .iter()
        .next()
        .expect("validated model field");
    let model = field.ident.as_ref().expect("validated named field");
    let model_type = &field.ty;
    let (impl_generics, type_generics, where_clause) = declaration.generics.split_for_impl();
    let writer = proc_macro2::Ident::new("__razor_writer", proc_macro2::Span::mixed_site());
    let mut writer_type_name = String::from("__RazorWriter");
    while declaration.generics.params.iter().any(|param| match param {
        syn::GenericParam::Type(param) => param.ident == writer_type_name,
        syn::GenericParam::Const(param) => param.ident == writer_type_name,
        syn::GenericParam::Lifetime(_) => false,
    }) {
        writer_type_name.push('_');
    }
    let writer_type = proc_macro2::Ident::new(&writer_type_name, proc_macro2::Span::mixed_site());
    let writes = generate_parts(parts, &runtime, &writer, Syntax::Async);
    quote! {
        #declaration
        impl #impl_generics #name #type_generics #where_clause {
            #visibility fn new(#model: #model_type) -> Self {
                Self { #model }
            }
        }
        impl #impl_generics #runtime::AsyncSlice for #name #type_generics
            #where_clause
        {
            async fn render_into<#writer_type: #runtime::AsyncSliceWriter + ::std::marker::Send>(
                &self,
                #writer: &mut #runtime::AsyncHtmlWriter<'_, #writer_type>,
            ) -> ::std::result::Result<(), #runtime::AsyncRenderError> {
                let #model = &self.#model;
                let _ = #model;
                #writes
                ::std::result::Result::Ok(())
            }
        }
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

    #[test]
    fn parses_typed_declarations_and_nested_async_directives() {
        for input in [
            r#"(runtime) pub struct Page<'a> { model: &'a Model } => {
                @flush;
                @{ let result = work(model).await?; }
                if true { for value in &result {
                    @await { next(value) }?;
                    {value}
                } } else { @await { async {} }; }
                @render { child };
                @section { provider }, { "header" };
            }"#,
            "(runtime) struct Empty { model: () } => {}",
            "(runtime) struct Generic<T> where T: Sync { model: T } => {}",
        ] {
            assert!(syn::parse_str::<AsyncTemplate>(input).is_ok(), "{input}");
        }
    }

    #[test]
    fn rejects_invalid_declarations_and_directives() {
        for input in [
            "(runtime) struct Page; => {}",
            "(runtime) struct Page(String); => {}",
            "(runtime) struct Page { other: String } => {}",
            "(runtime) struct Page { model: String, other: String } => {}",
            "(runtime) struct Page { model: String } {}",
            "(runtime) struct Page { model: String } => {} trailing",
            "(runtime) struct Page { model: String } => { @unknown; }",
            "(runtime) struct Page { model: String } => { @flush }",
            "(runtime) struct Page { model: String } => { @await {} ; }",
            "(runtime) struct Page { model: String } => { @await {a b}; }",
            "(runtime) struct Page { model: String } => { @render {child} }",
            "(runtime) struct Page { model: String } => { @section {child}; }",
            "(runtime) struct Page { model: String } => { if true { @flush } }",
        ] {
            assert!(syn::parse_str::<AsyncTemplate>(input).is_err(), "{input}");
        }
        assert!(syn::parse_str::<Template>("(runtime) @flush;").is_err());
    }
}
