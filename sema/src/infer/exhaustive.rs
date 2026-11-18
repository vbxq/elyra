use crate::types::{EnumVariantFieldsDef, InferType, TypeTable};
use aelys_syntax::{Pattern, PatternKind};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
enum Ctor {
    Bool(bool),
    Int(i128, i128),
    Str(String),
    Variant(usize),
    Single,
    Opaque,
}

#[derive(Clone, Debug)]
enum Pat {
    Wild,
    Ctor(Ctor, Vec<Pat>),
    Or(Vec<Pat>),
}

#[derive(Clone, Debug)]
enum Witness {
    Wild,
    Ctor(Ctor, Vec<Witness>),
}

#[derive(Clone, Copy, PartialEq)]
enum VariantForm {
    Unit,
    Tuple,
    Named,
}

#[derive(Clone)]
struct VariantShape {
    name: String,
    builtin: bool,
    form: VariantForm,
    fields: Vec<(String, InferType)>,
}

#[derive(Clone)]
enum Shape {
    Bool,
    Int(i128, i128),
    Char,
    Str,
    Sum(Vec<VariantShape>),
    Product {
        name: String,
        tuple: bool,
        fields: Vec<(String, InferType)>,
    },
    Opaque,
}

pub(crate) struct Analysis {
    pub missing: Vec<String>,
    pub unreachable: Vec<usize>,
}

pub(crate) struct Checker<'a> {
    table: &'a TypeTable,
}

const INT_MIN: i128 = aelys_bytecode::Value::INT_MIN as i128;
const INT_MAX: i128 = aelys_bytecode::Value::INT_MAX as i128;
const SURROGATES: i128 = 0x800;
const CHAR_LAST: i128 = 0x10FFFF - SURROGATES;

// the surrogate gap holds no char, so indices skip it and a range spanning it stays contiguous
fn char_index(value: char) -> i128 {
    let code = i128::from(u32::from(value));
    if code >= 0xE000 {
        code - SURROGATES
    } else {
        code
    }
}

impl<'a> Checker<'a> {
    pub fn new(table: &'a TypeTable) -> Self {
        Self { table }
    }

    fn shape(&self, ty: &InferType) -> Shape {
        match ty {
            InferType::Bool => Shape::Bool,
            InferType::I8 => Shape::Int(i8::MIN.into(), i8::MAX.into()),
            InferType::I16 => Shape::Int(i16::MIN.into(), i16::MAX.into()),
            InferType::I32 => Shape::Int(i32::MIN.into(), i32::MAX.into()),
            InferType::U8 => Shape::Int(0, u8::MAX.into()),
            InferType::U16 => Shape::Int(0, u16::MAX.into()),
            InferType::U32 => Shape::Int(0, u32::MAX.into()),
            InferType::I64 => Shape::Int(INT_MIN, INT_MAX),
            InferType::Char => Shape::Char,
            InferType::String => Shape::Str,
            InferType::Option(inner) => Shape::Sum(vec![
                VariantShape {
                    name: "None".to_string(),
                    builtin: true,
                    form: VariantForm::Unit,
                    fields: Vec::new(),
                },
                VariantShape {
                    name: "Some".to_string(),
                    builtin: true,
                    form: VariantForm::Tuple,
                    fields: vec![(String::new(), inner.as_ref().clone())],
                },
            ]),
            InferType::Result(ok, error) => Shape::Sum(vec![
                VariantShape {
                    name: "Ok".to_string(),
                    builtin: true,
                    form: VariantForm::Tuple,
                    fields: vec![(String::new(), ok.as_ref().clone())],
                },
                VariantShape {
                    name: "Err".to_string(),
                    builtin: true,
                    form: VariantForm::Tuple,
                    fields: vec![(String::new(), error.as_ref().clone())],
                },
            ]),
            InferType::Error => Shape::Sum(vec![VariantShape {
                name: "Message".to_string(),
                builtin: true,
                form: VariantForm::Tuple,
                fields: vec![(String::new(), InferType::String)],
            }]),
            InferType::Tuple(elements) => Shape::Product {
                name: String::new(),
                tuple: true,
                fields: elements
                    .iter()
                    .enumerate()
                    .map(|(index, ty)| (index.to_string(), ty.clone()))
                    .collect(),
            },
            InferType::Struct(name) | InferType::Applied { name, .. } => {
                let args: &[InferType] = match ty {
                    InferType::Applied { args, .. } => args,
                    _ => &[],
                };
                if let Some(def) = self.table.get_enum(name) {
                    let substitutions: HashMap<String, InferType> = def
                        .type_params
                        .iter()
                        .cloned()
                        .zip(args.iter().cloned())
                        .collect();
                    return Shape::Sum(
                        def.variants
                            .iter()
                            .map(|variant| {
                                let (form, fields) = match &variant.fields {
                                    EnumVariantFieldsDef::Unit => (VariantForm::Unit, Vec::new()),
                                    EnumVariantFieldsDef::Tuple(types) => (
                                        VariantForm::Tuple,
                                        types
                                            .iter()
                                            .map(|ty| {
                                                (
                                                    String::new(),
                                                    ty.substitute_params(&substitutions),
                                                )
                                            })
                                            .collect(),
                                    ),
                                    EnumVariantFieldsDef::Named(fields) => (
                                        VariantForm::Named,
                                        fields
                                            .iter()
                                            .map(|field| {
                                                (
                                                    field.name.clone(),
                                                    field.ty.substitute_params(&substitutions),
                                                )
                                            })
                                            .collect(),
                                    ),
                                };
                                VariantShape {
                                    name: format!("{name}::{}", variant.name),
                                    builtin: false,
                                    form,
                                    fields,
                                }
                            })
                            .collect(),
                    );
                }
                if let Some(def) = self.table.get_struct(name) {
                    let substitutions: HashMap<String, InferType> = def
                        .type_params
                        .iter()
                        .cloned()
                        .zip(args.iter().cloned())
                        .collect();
                    return Shape::Product {
                        name: crate::types::tuple_arity(name)
                            .map(|_| String::new())
                            .unwrap_or_else(|| name.clone()),
                        tuple: crate::types::tuple_arity(name).is_some(),
                        fields: def
                            .fields
                            .iter()
                            .map(|field| {
                                (
                                    field.name.clone(),
                                    field.ty.substitute_params(&substitutions),
                                )
                            })
                            .collect(),
                    };
                }
                Shape::Opaque
            }
            _ => Shape::Opaque,
        }
    }

    fn field_types(&self, ty: &InferType, ctor: &Ctor) -> Vec<InferType> {
        match (self.shape(ty), ctor) {
            (Shape::Sum(variants), Ctor::Variant(index)) => variants
                .get(*index)
                .map(|variant| variant.fields.iter().map(|(_, ty)| ty.clone()).collect())
                .unwrap_or_default(),
            (Shape::Product { fields, .. }, Ctor::Single) => {
                fields.into_iter().map(|(_, ty)| ty).collect()
            }
            _ => Vec::new(),
        }
    }

    fn lower(&self, pattern: &Pattern, ty: &InferType) -> Pat {
        let shape = self.shape(ty);
        match &pattern.kind {
            PatternKind::Wildcard | PatternKind::Binding(_) => Pat::Wild,
            PatternKind::Or(alternatives) => Pat::Or(
                alternatives
                    .iter()
                    .map(|alternative| self.lower(alternative, ty))
                    .collect(),
            ),
            PatternKind::Bool(value) => Pat::Ctor(Ctor::Bool(*value), Vec::new()),
            PatternKind::String(value) => Pat::Ctor(Ctor::Str(value.clone()), Vec::new()),
            PatternKind::Char(value) => {
                let index = char_index(*value);
                Pat::Ctor(Ctor::Int(index, index), Vec::new())
            }
            PatternKind::CharRange {
                start,
                end,
                inclusive,
            } => {
                let low = start.map_or(0, char_index);
                let high = match end {
                    Some(end) if *inclusive => char_index(*end),
                    Some(end) => char_index(*end) - 1,
                    None => CHAR_LAST,
                };
                if low > high {
                    return Pat::Ctor(Ctor::Opaque, Vec::new());
                }
                Pat::Ctor(Ctor::Int(low, high), Vec::new())
            }
            PatternKind::Int(value) => {
                let value = i128::from(*value);
                Pat::Ctor(Ctor::Int(value, value), Vec::new())
            }
            PatternKind::Range {
                start,
                end,
                inclusive,
            } => {
                let (min, max) = match shape {
                    Shape::Int(min, max) => (min, max),
                    _ => (INT_MIN, INT_MAX),
                };
                let low = start.map(i128::from).unwrap_or(min).max(min);
                let high = end
                    .map(|end| i128::from(end) - i128::from(!*inclusive))
                    .unwrap_or(max)
                    .min(max);
                if low > high {
                    return Pat::Ctor(Ctor::Opaque, Vec::new());
                }
                Pat::Ctor(Ctor::Int(low, high), Vec::new())
            }
            PatternKind::Variant { path, fields, .. } => {
                let Shape::Sum(variants) = shape else {
                    return Pat::Ctor(Ctor::Opaque, Vec::new());
                };
                let Some(last) = path.last() else {
                    return Pat::Wild;
                };
                let Some(index) = variants
                    .iter()
                    .position(|variant| variant.name.rsplit("::").next() == Some(last.as_str()))
                else {
                    return Pat::Wild;
                };
                let variant = &variants[index];
                if variant.form == VariantForm::Named || fields.len() != variant.fields.len() {
                    return Pat::Wild;
                }
                Pat::Ctor(
                    Ctor::Variant(index),
                    fields
                        .iter()
                        .zip(&variant.fields)
                        .map(|(field, (_, ty))| self.lower(field, ty))
                        .collect(),
                )
            }
            PatternKind::Struct { path, fields, .. } => {
                let (ctor, declared) = match shape {
                    Shape::Sum(variants) => {
                        let Some(last) = path.last() else {
                            return Pat::Wild;
                        };
                        let Some(index) = variants.iter().position(|variant| {
                            variant.name.rsplit("::").next() == Some(last.as_str())
                        }) else {
                            return Pat::Wild;
                        };
                        (Ctor::Variant(index), variants[index].fields.clone())
                    }
                    Shape::Product { fields, .. } => (Ctor::Single, fields),
                    _ => return Pat::Ctor(Ctor::Opaque, Vec::new()),
                };
                Pat::Ctor(
                    ctor,
                    declared
                        .iter()
                        .map(|(name, ty)| {
                            fields
                                .iter()
                                .find(|field| &field.name == name)
                                .map(|field| self.lower(&field.pattern, ty))
                                .unwrap_or(Pat::Wild)
                        })
                        .collect(),
                )
            }
        }
    }

    pub fn analyze(&self, ty: &InferType, arms: &[(&Pattern, bool)]) -> Analysis {
        let lowered: Vec<Pat> = arms
            .iter()
            .map(|(pattern, _)| self.lower(pattern, ty))
            .collect();
        let mut unreachable = Vec::new();
        let mut rows: Vec<Vec<Pat>> = Vec::new();
        for (index, (pat, (_, guarded))) in lowered.iter().zip(arms).enumerate() {
            if !self.useful(&rows, std::slice::from_ref(pat), std::slice::from_ref(ty)) {
                unreachable.push(index);
            }
            if !guarded {
                rows.push(vec![pat.clone()]);
            }
        }
        let missing = self.top_level_missing(&rows, ty);
        Analysis {
            missing,
            unreachable,
        }
    }

    fn top_level_missing(&self, rows: &[Vec<Pat>], ty: &InferType) -> Vec<String> {
        let rows = expand_or_rows(rows);
        let types = std::slice::from_ref(ty);
        let shape = self.shape(ty);
        let finite: Vec<Ctor> = match &shape {
            Shape::Bool => vec![Ctor::Bool(true), Ctor::Bool(false)],
            Shape::Sum(variants) => (0..variants.len()).map(Ctor::Variant).collect(),
            _ => Vec::new(),
        };
        if finite.is_empty() {
            return self
                .witness(&rows, types)
                .map(|witness| vec![self.render(ty, &witness[0])])
                .unwrap_or_default();
        }
        let mut missing = Vec::new();
        for ctor in finite {
            let specialized = specialize_rows(&rows, &ctor, self.field_types(ty, &ctor).len());
            let fields = self.field_types(ty, &ctor);
            if let Some(witness) = self.witness(&specialized, &fields) {
                missing.push(self.render(ty, &Witness::Ctor(ctor, witness)));
            }
        }
        missing
    }

    fn column_ctors(rows: &[Vec<Pat>]) -> Vec<Ctor> {
        let mut ctors = Vec::new();
        for row in rows {
            if let Some(Pat::Ctor(ctor, _)) = row.first()
                && !ctors.contains(ctor)
            {
                ctors.push(ctor.clone());
            }
        }
        ctors
    }

    fn signature(
        &self,
        ty: &InferType,
        present: &[Ctor],
        probe: Option<&Ctor>,
    ) -> (Vec<Ctor>, Option<Ctor>) {
        match self.shape(ty) {
            Shape::Bool => {
                let all = vec![Ctor::Bool(true), Ctor::Bool(false)];
                let missing = all.iter().find(|ctor| !present.contains(ctor)).cloned();
                (all, missing)
            }
            Shape::Sum(variants) => {
                let all: Vec<Ctor> = (0..variants.len()).map(Ctor::Variant).collect();
                let missing = all.iter().find(|ctor| !present.contains(ctor)).cloned();
                (all, missing)
            }
            Shape::Product { .. } => (vec![Ctor::Single], None),
            Shape::Char => Self::int_signature(0, CHAR_LAST, present, probe),
            Shape::Int(min, max) => Self::int_signature(min, max, present, probe),
            Shape::Str | Shape::Opaque => (Vec::new(), Some(Ctor::Opaque)),
        }
    }

    fn int_signature(
        min: i128,
        max: i128,
        present: &[Ctor],
        probe: Option<&Ctor>,
    ) -> (Vec<Ctor>, Option<Ctor>) {
        let mut cuts = vec![min, max + 1];
        for ctor in present.iter().chain(probe) {
            if let Ctor::Int(low, high) = ctor {
                cuts.push((*low).clamp(min, max + 1));
                cuts.push((*high + 1).clamp(min, max + 1));
            }
        }
        cuts.sort_unstable();
        cuts.dedup();
        let pieces: Vec<Ctor> = cuts
            .windows(2)
            .map(|pair| Ctor::Int(pair[0], pair[1] - 1))
            .collect();
        let missing = pieces
            .iter()
            .find(|piece| !present.iter().any(|ctor| ctor_covers(ctor, piece)))
            .cloned();
        (pieces, missing)
    }

    fn useful(&self, rows: &[Vec<Pat>], row: &[Pat], types: &[InferType]) -> bool {
        let Some(head) = row.first() else {
            return rows.is_empty();
        };
        let rows = expand_or_rows(rows);
        match head {
            Pat::Or(alternatives) => alternatives.iter().any(|alternative| {
                let mut candidate = vec![alternative.clone()];
                candidate.extend_from_slice(&row[1..]);
                self.useful(&rows, &candidate, types)
            }),
            Pat::Ctor(ctor, _) => {
                let present = Self::column_ctors(&rows);
                let (pieces, _) = self.signature(&types[0], &present, Some(ctor));
                let probes: Vec<Ctor> = match ctor {
                    Ctor::Int(..) => pieces
                        .into_iter()
                        .filter(|piece| ctor_covers(ctor, piece))
                        .collect(),
                    _ => vec![ctor.clone()],
                };
                probes.iter().any(|probe| {
                    let fields = self.field_types(&types[0], probe);
                    let specialized = specialize_rows(&rows, probe, fields.len());
                    let Some(candidate) = specialize_row(row, probe, fields.len()) else {
                        return false;
                    };
                    let mut next_types = fields;
                    next_types.extend_from_slice(&types[1..]);
                    self.useful(&specialized, &candidate, &next_types)
                })
            }
            Pat::Wild => {
                let present = Self::column_ctors(&rows);
                let (all, missing) = self.signature(&types[0], &present, None);
                if missing.is_none() && !all.is_empty() {
                    all.iter().any(|ctor| {
                        let fields = self.field_types(&types[0], ctor);
                        let specialized = specialize_rows(&rows, ctor, fields.len());
                        let mut candidate = vec![Pat::Wild; fields.len()];
                        candidate.extend_from_slice(&row[1..]);
                        let mut next_types = fields;
                        next_types.extend_from_slice(&types[1..]);
                        self.useful(&specialized, &candidate, &next_types)
                    })
                } else {
                    self.useful(&default_rows(&rows), &row[1..], &types[1..])
                }
            }
        }
    }

    fn witness(&self, rows: &[Vec<Pat>], types: &[InferType]) -> Option<Vec<Witness>> {
        let Some(ty) = types.first() else {
            return rows.is_empty().then(Vec::new);
        };
        let rows = expand_or_rows(rows);
        let present = Self::column_ctors(&rows);
        let (all, missing) = self.signature(ty, &present, None);
        if missing.is_none() && !all.is_empty() {
            for ctor in all {
                let fields = self.field_types(ty, &ctor);
                let arity = fields.len();
                let specialized = specialize_rows(&rows, &ctor, arity);
                let mut next_types = fields;
                next_types.extend_from_slice(&types[1..]);
                if let Some(mut found) = self.witness(&specialized, &next_types) {
                    let rest = found.split_off(arity);
                    let mut result = vec![Witness::Ctor(ctor, found)];
                    result.extend(rest);
                    return Some(result);
                }
            }
            return None;
        }
        let rest = self.witness(&default_rows(&rows), &types[1..])?;
        let head = match missing {
            Some(Ctor::Opaque) | None => Witness::Wild,
            Some(_) if present.is_empty() => Witness::Wild,
            Some(ctor) => {
                let arity = self.field_types(ty, &ctor).len();
                Witness::Ctor(ctor, vec![Witness::Wild; arity])
            }
        };
        let mut result = vec![head];
        result.extend(rest);
        Some(result)
    }

    fn render(&self, ty: &InferType, witness: &Witness) -> String {
        let Witness::Ctor(ctor, fields) = witness else {
            return "_".to_string();
        };
        match (self.shape(ty), ctor) {
            (_, Ctor::Bool(value)) => value.to_string(),
            (Shape::Int(min, max), Ctor::Int(low, high)) => {
                if *low == min && *high == max {
                    "_".to_string()
                } else if low == high {
                    low.to_string()
                } else if *low == min {
                    format!("..={high}")
                } else if *high == max {
                    format!("{low}..")
                } else {
                    format!("{low}..={high}")
                }
            }
            (Shape::Sum(variants), Ctor::Variant(index)) => {
                let Some(variant) = variants.get(*index) else {
                    return "_".to_string();
                };
                let all_wild = fields.iter().all(|field| matches!(field, Witness::Wild));
                match variant.form {
                    VariantForm::Unit => variant.name.clone(),
                    VariantForm::Tuple if all_wild && !variant.builtin => {
                        format!("{}(..)", variant.name)
                    }
                    VariantForm::Tuple => format!(
                        "{}({})",
                        variant.name,
                        fields
                            .iter()
                            .zip(&variant.fields)
                            .map(|(field, (_, ty))| self.render(ty, field))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    VariantForm::Named => self.render_named(&variant.name, &variant.fields, fields),
                }
            }
            (
                Shape::Product {
                    name,
                    tuple: true,
                    fields: declared,
                },
                Ctor::Single,
            ) => {
                let _ = name;
                format!(
                    "({})",
                    fields
                        .iter()
                        .zip(&declared)
                        .map(|(field, (_, ty))| self.render(ty, field))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            (
                Shape::Product {
                    name,
                    fields: declared,
                    ..
                },
                Ctor::Single,
            ) => self.render_named(&name, &declared, fields),
            _ => "_".to_string(),
        }
    }

    fn render_named(
        &self,
        name: &str,
        declared: &[(String, InferType)],
        fields: &[Witness],
    ) -> String {
        let shown: Vec<String> = fields
            .iter()
            .zip(declared)
            .filter(|(field, _)| !matches!(field, Witness::Wild))
            .map(|(field, (field_name, ty))| format!("{field_name}: {}", self.render(ty, field)))
            .collect();
        if shown.is_empty() {
            format!("{name} {{ .. }}")
        } else {
            format!("{name} {{ {}, .. }}", shown.join(", "))
        }
    }

    pub fn is_struct(&self, ty: &InferType) -> Option<String> {
        match self.shape(ty) {
            Shape::Product {
                name, tuple: false, ..
            } => Some(name),
            _ => None,
        }
    }

    pub fn capitalized_bindings(
        &self,
        pattern: &Pattern,
        ty: &InferType,
        out: &mut Vec<(String, Option<String>, aelys_syntax::Span)>,
    ) {
        let shape = self.shape(ty);
        match &pattern.kind {
            PatternKind::Binding(name) => {
                let Shape::Sum(variants) = shape else {
                    return;
                };
                if !name.chars().next().is_some_and(char::is_uppercase) {
                    return;
                }
                let suggestion = variants
                    .iter()
                    .map(|variant| {
                        let short = variant.name.rsplit("::").next().unwrap_or(&variant.name);
                        (edit_distance(name, short), variant.name.clone())
                    })
                    .filter(|(distance, _)| *distance <= (name.chars().count() / 3).max(2))
                    .min_by_key(|(distance, _)| *distance)
                    .map(|(_, name)| name);
                out.push((name.clone(), suggestion, pattern.span));
            }
            PatternKind::Or(alternatives) => {
                for alternative in alternatives {
                    self.capitalized_bindings(alternative, ty, out);
                }
            }
            PatternKind::Variant { path, fields, .. } => {
                let Shape::Sum(variants) = shape else {
                    return;
                };
                let Some(variant) = path.last().and_then(|last| {
                    variants
                        .iter()
                        .find(|variant| variant.name.rsplit("::").next() == Some(last.as_str()))
                }) else {
                    return;
                };
                for (field, (_, ty)) in fields.iter().zip(&variant.fields) {
                    self.capitalized_bindings(field, ty, out);
                }
            }
            PatternKind::Struct { path, fields, .. } => {
                let declared = match shape {
                    Shape::Sum(variants) => path
                        .last()
                        .and_then(|last| {
                            variants.iter().find(|variant| {
                                variant.name.rsplit("::").next() == Some(last.as_str())
                            })
                        })
                        .map(|variant| variant.fields.clone())
                        .unwrap_or_default(),
                    Shape::Product { fields, .. } => fields,
                    _ => Vec::new(),
                };
                for field in fields {
                    if let Some((_, ty)) = declared.iter().find(|(name, _)| *name == field.name) {
                        self.capitalized_bindings(&field.pattern, ty, out);
                    }
                }
            }
            _ => {}
        }
    }
}

fn ctor_covers(outer: &Ctor, inner: &Ctor) -> bool {
    match (outer, inner) {
        (Ctor::Int(low, high), Ctor::Int(inner_low, inner_high)) => {
            low <= inner_low && inner_high <= high
        }
        _ => outer == inner,
    }
}

fn expand_or_rows(rows: &[Vec<Pat>]) -> Vec<Vec<Pat>> {
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        expand_or_row(row, &mut out);
    }
    out
}

fn expand_or_row(row: &[Pat], out: &mut Vec<Vec<Pat>>) {
    match row.first() {
        Some(Pat::Or(alternatives)) => {
            for alternative in alternatives {
                let mut expanded = vec![alternative.clone()];
                expanded.extend_from_slice(&row[1..]);
                expand_or_row(&expanded, out);
            }
        }
        _ => out.push(row.to_vec()),
    }
}

fn specialize_row(row: &[Pat], ctor: &Ctor, arity: usize) -> Option<Vec<Pat>> {
    let (head, rest) = row.split_first()?;
    let mut out = match head {
        Pat::Wild => vec![Pat::Wild; arity],
        Pat::Ctor(head_ctor, fields) if ctor_covers(head_ctor, ctor) => {
            let mut fields = fields.clone();
            fields.resize(arity, Pat::Wild);
            fields
        }
        _ => return None,
    };
    out.extend_from_slice(rest);
    Some(out)
}

fn specialize_rows(rows: &[Vec<Pat>], ctor: &Ctor, arity: usize) -> Vec<Vec<Pat>> {
    rows.iter()
        .filter_map(|row| specialize_row(row, ctor, arity))
        .collect()
}

fn default_rows(rows: &[Vec<Pat>]) -> Vec<Vec<Pat>> {
    rows.iter()
        .filter(|row| matches!(row.first(), Some(Pat::Wild)))
        .map(|row| row[1..].to_vec())
        .collect()
}

fn edit_distance(left: &str, right: &str) -> usize {
    let left: Vec<char> = left.to_lowercase().chars().collect();
    let right: Vec<char> = right.to_lowercase().chars().collect();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    for (i, a) in left.iter().enumerate() {
        let mut current = vec![i + 1];
        for (j, b) in right.iter().enumerate() {
            let substitution = previous[j] + usize::from(a != b);
            current.push(substitution.min(previous[j + 1] + 1).min(current[j] + 1));
        }
        previous = current;
    }
    previous[right.len()]
}
