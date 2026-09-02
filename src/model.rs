use proc_macro2::Span;
use syn::{Expr, Ident};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Number {
    Signed(i128),
    Unsigned(u128),
}

#[derive(Clone, Copy)]
pub struct Domain {
    pub min: Number,
    pub max: Number,
    pub unsigned: bool,
}

#[derive(Clone)]
pub struct RangeValue {
    pub start: Option<Expr>,
    pub end: Option<Expr>,
    pub inclusive: bool,
    pub lower: Number,
    pub upper: Number,
}

#[derive(Clone)]
pub enum Mapping {
    Single { expr: Expr, value: Number },
    Range(Box<RangeValue>),
}

impl Mapping {
    pub fn lower(&self) -> Number {
        match self {
            Self::Single { value, .. } => *value,
            Self::Range(range) => range.lower,
        }
    }

    pub fn upper(&self) -> Number {
        match self {
            Self::Single { value, .. } => *value,
            Self::Range(range) => range.upper,
        }
    }

    pub fn overlaps(&self, other: &Self) -> bool {
        self.lower() <= other.upper() && other.lower() <= self.upper()
    }
}

pub struct VariantMapping {
    pub name: Ident,
    pub span: Span,
    pub mappings: Vec<Mapping>,
}

impl VariantMapping {
    pub fn overlaps(&self, other: &Self) -> bool {
        self.mappings.iter().any(|mapping| {
            other
                .mappings
                .iter()
                .any(|other_mapping| mapping.overlaps(other_mapping))
        })
    }

    pub fn valid(&self) -> bool {
        self.mappings.iter().enumerate().all(|(index, mapping)| {
            self.mappings[index + 1..]
                .iter()
                .all(|other_mapping| !mapping.overlaps(other_mapping))
        })
    }
}
