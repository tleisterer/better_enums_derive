use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{
    Attribute, Error, Expr, ExprRange, Fields, Ident, ItemEnum, Lit, RangeLimits, UnOp,
    parse_macro_input, spanned::Spanned,
};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Number {
    Signed(i128),
    Unsigned(u128),
}

#[derive(Clone, Copy)]
struct Domain {
    min: Number,
    max: Number,
    unsigned: bool,
}

#[derive(Clone)]
struct RangeValue {
    start: Option<Expr>,
    end: Option<Expr>,
    inclusive: bool,
    lower: Number,
    upper: Number,
}

#[derive(Clone)]
enum Mapping {
    Single { expr: Expr, value: Number },
    Range(Box<RangeValue>),
}

struct VariantMapping {
    name: Ident,
    span: Span,
    mappings: Vec<Mapping>,
}

fn extract_repr(attrs: &[Attribute]) -> Result<Ident, Error> {
    for attr in attrs {
        if attr.path().is_ident("repr") {
            let mut repr = None;
            attr.parse_nested_meta(|meta| {
                let ty = meta.path.get_ident().ok_or_else(|| {
                    Error::new_spanned(&meta.path, "better_enums: repr must be an integer type")
                })?;
                if matches!(
                    ty.to_string().as_str(),
                    "i8" | "i16"
                        | "i32"
                        | "i64"
                        | "i128"
                        | "isize"
                        | "u8"
                        | "u16"
                        | "u32"
                        | "u64"
                        | "u128"
                        | "usize"
                ) {
                    repr = Some(ty.clone());
                    Ok(())
                } else {
                    Err(Error::new_spanned(
                        ty,
                        "better_enums: repr must be an integer type",
                    ))
                }
            })?;
            return repr.ok_or_else(|| Error::new_spanned(attr, "better_enums: repr is missing"));
        }
    }
    Err(Error::new(
        Span::call_site(),
        "better_enums: repr attribute missing",
    ))
}

fn domain(repr: &Ident) -> Domain {
    match repr.to_string().as_str() {
        "i8" => Domain {
            min: Number::Signed(i8::MIN as i128),
            max: Number::Signed(i8::MAX as i128),
            unsigned: false,
        },
        "i16" => Domain {
            min: Number::Signed(i16::MIN as i128),
            max: Number::Signed(i16::MAX as i128),
            unsigned: false,
        },
        "i32" => Domain {
            min: Number::Signed(i32::MIN as i128),
            max: Number::Signed(i32::MAX as i128),
            unsigned: false,
        },
        "i64" => Domain {
            min: Number::Signed(i64::MIN as i128),
            max: Number::Signed(i64::MAX as i128),
            unsigned: false,
        },
        "i128" => Domain {
            min: Number::Signed(i128::MIN),
            max: Number::Signed(i128::MAX),
            unsigned: false,
        },
        "isize" => Domain {
            min: Number::Signed(isize::MIN as i128),
            max: Number::Signed(isize::MAX as i128),
            unsigned: false,
        },
        "u8" => Domain {
            min: Number::Unsigned(0),
            max: Number::Unsigned(u8::MAX as u128),
            unsigned: true,
        },
        "u16" => Domain {
            min: Number::Unsigned(0),
            max: Number::Unsigned(u16::MAX as u128),
            unsigned: true,
        },
        "u32" => Domain {
            min: Number::Unsigned(0),
            max: Number::Unsigned(u32::MAX as u128),
            unsigned: true,
        },
        "u64" => Domain {
            min: Number::Unsigned(0),
            max: Number::Unsigned(u64::MAX as u128),
            unsigned: true,
        },
        "u128" => Domain {
            min: Number::Unsigned(0),
            max: Number::Unsigned(u128::MAX),
            unsigned: true,
        },
        "usize" => Domain {
            min: Number::Unsigned(0),
            max: Number::Unsigned(usize::MAX as u128),
            unsigned: true,
        },
        _ => unreachable!(),
    }
}

fn parse_number(expr: &Expr, unsigned: bool) -> Result<Number, Error> {
    match expr {
        Expr::Lit(lit) => match &lit.lit {
            Lit::Int(value) => {
                let number = value.base10_parse::<u128>().map_err(|_| {
                    Error::new_spanned(expr, "better_enums: integer literal is too large")
                })?;
                if unsigned {
                    Ok(Number::Unsigned(number))
                } else {
                    i128::try_from(number).map(Number::Signed).map_err(|_| {
                        Error::new_spanned(
                            expr,
                            "better_enums: signed integer literal is too large",
                        )
                    })
                }
            }
            _ => Err(Error::new_spanned(
                expr,
                "better_enums: expected an integer literal",
            )),
        },
        Expr::Unary(unary) if matches!(unary.op, UnOp::Neg(_)) => {
            let value = parse_number(&unary.expr, true)?;
            match value {
                Number::Unsigned(value) if value == (i128::MAX as u128) + 1 => {
                    Ok(Number::Signed(i128::MIN))
                }
                Number::Unsigned(value) => i128::try_from(value)
                    .ok()
                    .and_then(|value| value.checked_neg())
                    .map(Number::Signed)
                    .ok_or_else(|| {
                        Error::new_spanned(expr, "better_enums: integer literal is too small")
                    }),
                Number::Signed(value) => value.checked_neg().map(Number::Signed).ok_or_else(|| {
                    Error::new_spanned(expr, "better_enums: integer literal is too small")
                }),
            }
        }
        _ => Err(Error::new_spanned(
            expr,
            "better_enums: expected an integer literal",
        )),
    }
}

fn validate_number(value: Number, bounds: Domain, expr: &Expr) -> Result<(), Error> {
    if (bounds.unsigned && !matches!(value, Number::Unsigned(_)))
        || (!bounds.unsigned && !matches!(value, Number::Signed(_)))
        || value < bounds.min
        || value > bounds.max
    {
        return Err(Error::new_spanned(
            expr,
            "better_enums: value is outside the repr range",
        ));
    }
    Ok(())
}

fn parse_range(range: &ExprRange, bounds: Domain) -> Result<RangeValue, Error> {
    let start = range.start.as_deref().cloned();
    let end = range.end.as_deref().cloned();
    let start_value = start
        .as_ref()
        .map(|expr| parse_number(expr, bounds.unsigned))
        .transpose()?
        .unwrap_or(bounds.min);
    let end_value = end
        .as_ref()
        .map(|expr| parse_number(expr, bounds.unsigned))
        .transpose()?
        .unwrap_or(bounds.max);
    if let Some(expr) = &start {
        validate_number(start_value, bounds, expr)?;
    }
    if let Some(expr) = &end {
        validate_number(end_value, bounds, expr)?;
    }
    let upper = if end.is_some() && matches!(range.limits, RangeLimits::HalfOpen(_)) {
        match end_value {
            Number::Signed(value) => value.checked_sub(1).map(Number::Signed),
            Number::Unsigned(value) => value.checked_sub(1).map(Number::Unsigned),
        }
        .ok_or_else(|| Error::new_spanned(range, "better_enums: range is empty"))?
    } else {
        end_value
    };
    if start_value > upper {
        return Err(Error::new_spanned(range, "better_enums: range is empty"));
    }
    Ok(RangeValue {
        start,
        end,
        inclusive: matches!(range.limits, RangeLimits::Closed(_)),
        lower: start_value,
        upper,
    })
}

fn parse_mappings(expr: &Expr, bounds: Domain) -> Result<Vec<Mapping>, Error> {
    match expr {
        Expr::Lit(_) | Expr::Unary(_) => {
            let value = parse_number(expr, bounds.unsigned)?;
            validate_number(value, bounds, expr)?;
            Ok(vec![Mapping::Single {
                expr: expr.clone(),
                value,
            }])
        }
        Expr::Range(range) => Ok(vec![Mapping::Range(Box::new(parse_range(range, bounds)?))]),
        Expr::Array(array) => array
            .elems
            .iter()
            .map(|element| parse_mappings(element, bounds))
            .try_fold(Vec::new(), |mut all, result| {
                all.extend(result?);
                Ok(all)
            }),
        _ => Err(Error::new_spanned(
            expr,
            "better_enums: discriminant must be an integer, range, or array thereof",
        )),
    }
}

fn lower(mapping: &Mapping) -> Number {
    match mapping {
        Mapping::Single { value, .. } => *value,
        Mapping::Range(range) => range.lower,
    }
}
fn upper(mapping: &Mapping) -> Number {
    match mapping {
        Mapping::Single { value, .. } => *value,
        Mapping::Range(range) => range.upper,
    }
}
fn overlaps(left: &Mapping, right: &Mapping) -> bool {
    lower(left) <= upper(right) && lower(right) <= upper(left)
}

fn literal_for(value: Number) -> Expr {
    let text = match value {
        Number::Signed(value) => value.to_string(),
        Number::Unsigned(value) => value.to_string(),
    };
    syn::parse_str(&text).expect("validated discriminant should parse")
}

fn collect_variants(input: &mut ItemEnum, bounds: Domain) -> Result<Vec<VariantMapping>, Error> {
    let mut next = Some(bounds.min);
    let mut result = Vec::new();
    for variant in &mut input.variants {
        if !matches!(variant.fields, Fields::Unit) {
            return Err(Error::new_spanned(
                &*variant,
                "better_enums: variant cannot have additional data",
            ));
        }
        let mappings = if let Some((_, expr)) = &variant.discriminant {
            parse_mappings(expr, bounds)?
        } else {
            let value = next.ok_or_else(|| {
                Error::new_spanned(
                    &*variant,
                    "better_enums: no implicit value remains in repr range",
                )
            })?;
            vec![Mapping::Single {
                expr: literal_for(value),
                value,
            }]
        };
        if mappings.is_empty() {
            return Err(Error::new_spanned(
                &*variant,
                "better_enums: a variant must map to at least one value",
            ));
        }
        let representative = lower(&mappings[0]);
        variant.discriminant = Some((syn::token::Eq::default(), literal_for(representative)));
        next = mappings
            .iter()
            .map(upper)
            .max()
            .and_then(|value| match value {
                Number::Signed(value) => value.checked_add(1).map(Number::Signed),
                Number::Unsigned(value) => value.checked_add(1).map(Number::Unsigned),
            })
            .filter(|value| *value <= bounds.max);
        result.push(VariantMapping {
            name: variant.ident.clone(),
            span: variant.span(),
            mappings,
        });
    }
    Ok(result)
}

fn validate_overlaps(variants: &[VariantMapping]) -> Result<(), Error> {
    for (index, current) in variants.iter().enumerate() {
        for current_mapping in &current.mappings {
            for previous in &variants[..index] {
                if previous
                    .mappings
                    .iter()
                    .any(|previous_mapping| overlaps(current_mapping, previous_mapping))
                {
                    return Err(Error::new(
                        current.span,
                        format!(
                            "better_enums: mapping for {} overlaps mapping for {}",
                            current.name, previous.name
                        ),
                    ));
                }
            }
        }
        for (left_index, left) in current.mappings.iter().enumerate() {
            if current.mappings[left_index + 1..]
                .iter()
                .any(|right| overlaps(left, right))
            {
                return Err(Error::new(
                    current.span,
                    format!(
                        "better_enums: mappings for {} overlap or duplicate each other",
                        current.name
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn condition(mapping: &Mapping) -> TokenStream {
    match mapping {
        Mapping::Single { expr, .. } => quote!(value == #expr),
        Mapping::Range(range) => match (range.start.as_ref(), range.end.as_ref()) {
            (Some(start), Some(end)) if range.inclusive => {
                quote!((#start..=#end).contains(&value))
            }
            (Some(start), Some(end)) => quote!((#start..#end).contains(&value)),
            (Some(start), None) => quote!(value >= #start),
            (None, Some(end)) if range.inclusive => quote!(value <= #end),
            (None, Some(end)) => quote!(value < #end),
            (None, None) => quote!(true),
        },
    }
}

fn generate_error(enum_name: &Ident, repr: &Ident) -> (Ident, TokenStream) {
    let error_name = format_ident!("{}Error", enum_name);
    (
        error_name.clone(),
        quote! {
            #[derive(Debug, Clone, Copy, PartialEq, Eq)]
            pub struct #error_name { pub value: #repr }
            impl std::fmt::Display for #error_name { fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(formatter, "{} is not a valid discriminant", self.value) } }
            impl std::error::Error for #error_name {}
        },
    )
}

fn generate_try_from(
    enum_name: &Ident,
    repr: &Ident,
    error_name: &Ident,
    variants: &[VariantMapping],
) -> TokenStream {
    let arms = variants.iter().map(|variant| {
        let conditions = variant.mappings.iter().map(condition);
        let name = &variant.name;
        quote! { if #(#conditions)||* { return Ok(#enum_name::#name); } }
    });
    quote! { impl std::convert::TryFrom<#repr> for #enum_name { type Error = #error_name; fn try_from(value: #repr) -> Result<Self, Self::Error> { #(#arms)* Err(#error_name { value }) } } }
}

fn better_enums_impl(mut input: ItemEnum) -> Result<TokenStream, Error> {
    if input.generics.lt_token.is_some() || input.generics.where_clause.is_some() {
        return Err(Error::new_spanned(
            &input.generics,
            "better_enums: enum cannot be generic",
        ));
    }
    let repr = extract_repr(&input.attrs)?;
    let bounds = domain(&repr);
    let variants = collect_variants(&mut input, bounds)?;
    validate_overlaps(&variants)?;
    let (error_name, error) = generate_error(&input.ident, &repr);
    let conversion = generate_try_from(&input.ident, &repr, &error_name, &variants);
    Ok(quote! { #input #error #conversion })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn diagnostic(source: &str) -> String {
        better_enums_impl(syn::parse_str(source).unwrap())
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn rejects_empty_mappings() {
        assert!(
            diagnostic("#[repr(u8)] enum Empty { Value = [] }")
                .contains("must map to at least one value")
        );
    }

    #[test]
    fn rejects_duplicate_mappings() {
        assert!(
            diagnostic("#[repr(u8)] enum Duplicate { Value = [1, 1] }")
                .contains("overlap or duplicate")
        );
    }

    #[test]
    fn rejects_overlapping_mappings() {
        assert!(
            diagnostic("#[repr(u8)] enum Overlap { First = 1..10, Second = 9..20 }")
                .contains("overlaps mapping for First")
        );
    }
}
