mod generate;
mod model;
mod parse;

use proc_macro2::TokenStream;
use quote::quote;
use syn::{ItemEnum, parse_macro_input};

use crate::generate::generate_code;
use crate::parse::{collect_variants, domain, extract_repr, validate_overlaps};

fn better_enums_impl(mut input: ItemEnum) -> Result<TokenStream, syn::Error> {
    if input.generics.lt_token.is_some() || input.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "better_enums: enum cannot be generic",
        ));
    }

    let repr = extract_repr(&input.attrs, &input.ident)?;
    let bounds = domain(&repr);
    let variants = collect_variants(&mut input, bounds)?;
    validate_overlaps(&variants)?;

    let code = generate_code(&input.ident, &repr, &variants);
    Ok(quote! { #input #code })
}

#[proc_macro_attribute]
pub fn better_enums(
    _attr: proc_macro::TokenStream,
    input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    match better_enums_impl(parse_macro_input!(input as ItemEnum)) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}
