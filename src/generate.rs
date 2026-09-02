use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::Ident;

use crate::model::{Mapping, VariantMapping};

fn condition(mapping: &Mapping) -> TokenStream {
    match mapping {
        Mapping::Single { expr, .. } => quote!(value == #expr),
        Mapping::Range(range) => match (range.start.as_ref(), range.end.as_ref()) {
            (Some(start), Some(end)) => {
                if range.inclusive {
                    quote!((#start..=#end).contains(&value))
                } else {
                    quote!((#start..#end).contains(&value))
                }
            }
            (Some(start), None) => quote!(value >= #start),
            (None, Some(end)) => {
                if range.inclusive {
                    quote!(value <= #end)
                } else {
                    quote!(value < #end)
                }
            }
            (None, None) => quote!(true),
        },
    }
}

pub fn generate_code(enum_name: &Ident, repr: &Ident, variants: &[VariantMapping]) -> TokenStream {
    let arms = variants.iter().map(|variant| {
        let conditions = variant.mappings.iter().map(condition);
        let name = &variant.name;
        quote! { if #(#conditions)||* { return Ok(#enum_name::#name); } }
    });

    let krate =
        crate_name("better_enums").expect("If this crate is not included something went wrong");

    let krate = match krate {
        FoundCrate::Itself => quote! { crate },
        FoundCrate::Name(name) => {
            let ident = Ident::new(&name, Span::call_site());
            quote! { #ident }
        }
    };

    quote! {
        impl std::convert::TryFrom<#repr> for #enum_name {
            type Error = #krate::generate::BetterEnumsError<#repr>;
            fn try_from(value: #repr) -> Result<Self, Self::Error> {
                #(#arms)*
                Err(Self::Error { value })
            }
        }
    }
}
