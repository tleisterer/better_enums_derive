use syn::{
    Attribute, Error as SynError, Expr, ExprRange, Fields, Ident, ItemEnum, Lit, RangeLimits,
    Token, UnOp, spanned::Spanned,
};

use crate::model::{Domain, Mapping, Number, RangeValue, VariantMapping};

pub fn extract_repr(attrs: &[Attribute], enum_name: &Ident) -> Result<Ident, SynError> {
    for attr in attrs {
        if attr.path().is_ident("repr") {
            let mut repr = None;
            attr.parse_nested_meta(|meta| {
                let ty = meta.path.get_ident().ok_or_else(|| {
                    SynError::new_spanned(&meta.path, "better_enums: repr must be an integer type")
                })?;
                match ty.to_string().as_str() {
                    "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16"
                    | "u32" | "u64" | "u128" | "usize" => {
                        repr = Some(ty.clone());
                        Ok(())
                    }
                    _ => Err(SynError::new_spanned(
                        ty,
                        "better_enums: repr must be an integer type",
                    )),
                }
            })?;
            return repr.ok_or_else(|| SynError::new_spanned(attr, "better_enums: repr is missing"));
        }
    }

    Err(SynError::new_spanned(
        enum_name,
        "better_enums: repr attribute missing",
    ))
}

pub fn domain(repr: &Ident) -> Domain {
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
        _ => unreachable!("At this point only numerics are possible"),
    }
}

fn parse_number(expr: &Expr, unsigned: bool) -> Result<Number, SynError> {
    match expr {
        Expr::Lit(lit) => match &lit.lit {
            Lit::Int(value) => {
                if unsigned {
                    let number = value.base10_parse::<u128>().map_err(|_| {
                        SynError::new_spanned(expr, "better_enums: value is outside the repr range")
                    })?;
                    Ok(Number::Unsigned(number))
                } else {
                    let number = value.base10_parse::<i128>().map_err(|_| {
                        SynError::new_spanned(expr, "better_enums: value is outside the repr range")
                    })?;
                    Ok(Number::Signed(number))
                }
            }
            _ => Err(SynError::new_spanned(
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
                    .map_err(|_| {
                        SynError::new_spanned(expr, "better_enums: value is outside the repr range")
                    })?
                    .checked_neg()
                    .map(Number::Signed)
                    .ok_or_else(|| {
                        SynError::new_spanned(expr, "better_enums: value is outside the repr range")
                    }),
                Number::Signed(_) => unreachable!("Negated values must be parsed as signed"),
            }
        }
        _ => Err(SynError::new_spanned(
            expr,
            "better_enums: expected an integer literal",
        )),
    }
}

fn validate_number(value: Number, bounds: Domain, expr: &Expr) -> Result<(), SynError> {
    if (bounds.unsigned && !matches!(value, Number::Unsigned(_)))
        || (!bounds.unsigned && !matches!(value, Number::Signed(_)))
        || value < bounds.min
        || value > bounds.max
    {
        return Err(SynError::new_spanned(
            expr,
            "better_enums: value is outside the repr range",
        ));
    }
    Ok(())
}

fn parse_range(range: &ExprRange, bounds: Domain) -> Result<RangeValue, SynError> {
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
        .ok_or_else(|| SynError::new_spanned(range, "better_enums: range is empty"))?
    } else {
        end_value
    };

    if start_value > upper {
        return Err(SynError::new_spanned(range, "better_enums: range is empty"));
    }

    Ok(RangeValue {
        start,
        end,
        inclusive: matches!(range.limits, RangeLimits::Closed(_)),
        lower: start_value,
        upper,
    })
}

fn parse_mappings(expr: &Expr, bounds: Domain) -> Result<Vec<Mapping>, SynError> {
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
        _ => Err(SynError::new_spanned(
            expr,
            "better_enums: discriminant must be an integer, range, or array thereof",
        )),
    }
}

fn literal_for(value: Number) -> Expr {
    let text = match value {
        Number::Signed(value) => value.to_string(),
        Number::Unsigned(value) => value.to_string(),
    };
    syn::parse_str(&text).expect("validated discriminant should parse")
}

pub fn collect_variants(input: &mut ItemEnum, bounds: Domain) -> Result<Vec<VariantMapping>, SynError> {
    let mut next = Some(if bounds.unsigned {
        Number::Unsigned(0)
    } else {
        Number::Signed(0)
    });
    let mut result = Vec::new();

    for variant in &mut input.variants {
        if !matches!(variant.fields, Fields::Unit) {
            return Err(SynError::new_spanned(
                &*variant,
                "better_enums: variant cannot have additional data",
            ));
        }

        let mappings = if let Some((_, expr)) = &variant.discriminant {
            parse_mappings(expr, bounds)?
        } else {
            let value = next.ok_or_else(|| {
                SynError::new_spanned(
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
            return Err(SynError::new_spanned(
                &*variant,
                "better_enums: a variant must map to at least one value",
            ));
        }

        let representative = mappings[0].lower();
        variant.discriminant = Some((<Token![=]>::default(), literal_for(representative)));
        next = mappings
            .iter()
            .map(Mapping::upper)
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

pub fn validate_overlaps(variants: &[VariantMapping]) -> Result<(), SynError> {
    for (index, current) in variants.iter().enumerate() {
        if !current.valid() {
            return Err(SynError::new(
                current.span,
                format!(
                    "better_enums: mappings for {} overlap or duplicate each other",
                    current.name
                ),
            ));
        }

        for previous in &variants[..index] {
            if current.overlaps(previous) {
                return Err(SynError::new(
                    current.span,
                    format!(
                        "better_enums: mapping for {} overlaps mapping for {}",
                        current.name, previous.name
                    ),
                ));
            }
        }
    }

    Ok(())
}
