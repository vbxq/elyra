use super::Compiler;
use aelys_bytecode::{
    DefId, EnumFieldSchema, EnumSchema, EnumVariantSchema, FloatWidth, Function, GlobalLayout,
    IntWidth, OpCode, StructFieldSchema, StructSchema, TypeDescriptor,
};
use aelys_common::Result;
use aelys_common::error::{CompileError, CompileErrorKind};
use aelys_sema::{TypedProgram, TypedStmtKind};
use std::collections::HashMap;
use std::sync::Arc;

impl Compiler {
    pub fn compile_typed(
        mut self,
        program: &TypedProgram,
    ) -> Result<(Function, HashMap<String, bool>)> {
        let struct_schemas = program
            .type_table
            .structs_in_declaration_order()
            .map(|def| {
                let module_id = def.owner.to_string();
                let schema_id = u32::from(program.type_table.schema_index(&def.name)?);
                let origin = program
                    .type_table
                    .nominal_instance_origin(&def.name)
                    .unwrap_or(&def.name);
                let def_ordinal = program
                    .type_table
                    .struct_definition_ordinal(origin)
                    .unwrap_or(schema_id);
                let type_args = program
                    .type_table
                    .nominal_instance_args(&def.name)
                    .unwrap_or(&[])
                    .iter()
                    .map(|ty| type_descriptor(ty, &program.type_table))
                    .collect::<Option<Vec<_>>>()?;
                Some(StructSchema::with_identity(
                    schema_id,
                    DefId::from_display_name(&format!("{module_id}::{origin}"), def_ordinal),
                    type_args,
                    def.fields
                        .iter()
                        .map(|field| {
                            Some(StructFieldSchema {
                                offset: 0,
                                name: field.name.clone(),
                                ty: type_descriptor(&field.ty, &program.type_table)?,
                            })
                        })
                        .collect::<Option<Vec<_>>>()?,
                ))
            })
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| poisoned_schema_error(&self.source))?;
        self.struct_schemas = std::rc::Rc::new(struct_schemas);
        self.current.struct_schemas = self.struct_schemas.as_ref().clone();
        let enum_schemas = program
            .type_table
            .enums_in_declaration_order()
            .map(|def| {
                let module_id = def.owner.to_string();
                let schema_id = program.type_table.enum_schema_index(&def.name)?;
                let name = format!("{module_id}::{}", def.name);
                let origin = program
                    .type_table
                    .nominal_instance_origin(&def.name)
                    .unwrap_or(&def.name);
                let def_id_name = format!("{module_id}::{origin}");
                let def_ordinal = program
                    .type_table
                    .enum_definition_ordinal(origin)
                    .unwrap_or(u32::from(schema_id));
                let type_args = program
                    .type_table
                    .nominal_instance_args(&def.name)
                    .unwrap_or(&[])
                    .iter()
                    .map(|ty| type_descriptor(ty, &program.type_table))
                    .collect::<Option<Vec<_>>>()?;
                Some(EnumSchema::with_identity(
                    schema_id,
                    DefId::from_display_name(&def_id_name, def_ordinal),
                    type_args,
                    name,
                    def.variants
                        .iter()
                        .enumerate()
                        .map(|(variant_id, variant)| {
                            let fields = match &variant.fields {
                                aelys_sema::EnumVariantFieldsDef::Unit => Vec::new(),
                                aelys_sema::EnumVariantFieldsDef::Tuple(fields) => fields
                                    .iter()
                                    .enumerate()
                                    .map(|(offset, ty)| {
                                        Some(EnumFieldSchema {
                                            offset: u16::try_from(offset).ok()?,
                                            name: None,
                                            ty: type_descriptor(ty, &program.type_table)?,
                                        })
                                    })
                                    .collect::<Option<Vec<_>>>()?,
                                aelys_sema::EnumVariantFieldsDef::Named(fields) => fields
                                    .iter()
                                    .enumerate()
                                    .map(|(offset, field)| {
                                        Some(EnumFieldSchema {
                                            offset: u16::try_from(offset).ok()?,
                                            name: Some(field.name.clone()),
                                            ty: type_descriptor(&field.ty, &program.type_table)?,
                                        })
                                    })
                                    .collect::<Option<Vec<_>>>()?,
                            };
                            Some(EnumVariantSchema {
                                variant_id: u16::try_from(variant_id).ok()?,
                                name: variant.name.clone(),
                                fields: fields.into_boxed_slice(),
                            })
                        })
                        .collect::<Option<Vec<_>>>()?,
                ))
            })
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| poisoned_schema_error(&self.source))?;
        self.enum_schemas = std::rc::Rc::new(enum_schemas);
        self.current.enum_schemas = self.enum_schemas.as_ref().clone();
        for stmt in &program.stmts {
            match &stmt.kind {
                TypedStmtKind::Function(func) => {
                    self.globals.insert(func.name.clone(), false);
                    if !self.global_indices.contains_key(&func.name) {
                        let idx = self.next_global_index;
                        self.global_indices.insert(func.name.clone(), idx);
                        self.next_global_index += 1;
                    }
                }
                TypedStmtKind::ImplDecl { methods, .. } => {
                    for func in methods {
                        self.globals.insert(func.name.clone(), false);
                        if !self.global_indices.contains_key(&func.name) {
                            let idx = self.next_global_index;
                            self.global_indices.insert(func.name.clone(), idx);
                            self.next_global_index += 1;
                        }
                    }
                }
                TypedStmtKind::Let { name, mutable, .. } => {
                    self.globals.insert(name.clone(), *mutable);
                    if !self.global_indices.contains_key(name) {
                        let idx = self.next_global_index;
                        self.global_indices.insert(name.clone(), idx);
                        self.next_global_index += 1;
                    }
                }
                _ => {}
            }
        }

        if program.stmts.is_empty() {
            self.emit_return0(aelys_syntax::Span::dummy());
        } else {
            let last_idx = program.stmts.len() - 1;

            for stmt in &program.stmts[..last_idx] {
                self.compile_typed_stmt(stmt)?;
            }

            let last_stmt = &program.stmts[last_idx];
            match &last_stmt.kind {
                TypedStmtKind::Expression(expr) => {
                    let result_reg = self.alloc_register()?;
                    self.compile_typed_expr(expr, result_reg)?;
                    self.emit_a(OpCode::Return, result_reg, 0, 0, last_stmt.span);
                }
                TypedStmtKind::If {
                    condition,
                    then_branch,
                    else_branch: Some(else_branch),
                } => {
                    let result_reg = self.alloc_register()?;
                    let cond_reg = self.alloc_register()?;
                    self.compile_typed_expr(condition, cond_reg)?;
                    let else_jump = self.emit_jump_if(OpCode::JumpIfNot, cond_reg, condition.span);
                    self.free_register(cond_reg);

                    self.compile_typed_if_branch_for_return(then_branch, result_reg)?;
                    let end_jump = self.emit_jump(OpCode::Jump, then_branch.span);
                    self.patch_jump(else_jump);
                    self.compile_typed_if_branch_for_return(else_branch, result_reg)?;
                    self.patch_jump(end_jump);

                    self.emit_a(OpCode::Return, result_reg, 0, 0, last_stmt.span);
                }
                TypedStmtKind::Block(_) => {
                    let result_reg = self.alloc_register()?;
                    self.compile_typed_if_branch_for_return(last_stmt, result_reg)?;
                    self.emit_a(OpCode::Return, result_reg, 0, 0, last_stmt.span);
                }
                _ => {
                    self.compile_typed_stmt(last_stmt)?;
                    self.emit_return0(last_stmt.span);
                }
            }
        }

        self.current.num_registers = self.next_register;
        self.current.global_layout = self.build_global_layout()?;
        self.current.compute_global_layout_hash();
        self.current.finalize_bytecode();
        self.update_jit_eligibility();

        if self.current.wide_operand_error().is_some() {
            return Err(CompileError::new(
                CompileErrorKind::TooManyRegisters,
                aelys_syntax::Span::dummy(),
                self.source.clone(),
            )
            .into());
        }
        if let Some(distance) = self.current.jump_overflow() {
            return Err(CompileError::new(
                CompileErrorKind::JumpOffsetTooLarge { distance },
                aelys_syntax::Span::dummy(),
                self.source.clone(),
            )
            .into());
        }

        Ok((self.current, self.globals))
    }

    pub(super) fn build_global_layout(&self) -> Result<Arc<GlobalLayout>> {
        if self.accessed_globals.is_empty() {
            Ok(GlobalLayout::empty())
        } else {
            let Some(global_count) = usize::try_from(self.next_global_index).ok() else {
                return Err(global_layout_error(&self.source));
            };
            let mut names = vec![String::new(); global_count];
            for (name, &idx) in &self.global_indices {
                if self.accessed_globals.contains(name) {
                    let Some(index) = usize::try_from(idx).ok() else {
                        return Err(global_layout_error(&self.source));
                    };
                    let Some(slot) = names.get_mut(index) else {
                        return Err(global_layout_error(&self.source));
                    };
                    *slot = name.clone();
                }
            }
            Ok(GlobalLayout::new(names))
        }
    }
}

fn global_layout_error(source: &Arc<aelys_syntax::Source>) -> aelys_common::AelysError {
    CompileError::new(
        CompileErrorKind::TypeInferenceError(
            "global layout index exceeds the target address space".to_string(),
        ),
        aelys_syntax::Span::dummy(),
        source.clone(),
    )
    .into()
}

fn poisoned_schema_error(source: &Arc<aelys_syntax::Source>) -> aelys_common::AelysError {
    CompileError::new(
        CompileErrorKind::TypeInferenceError(
            "a poisoned type reached bytecode schema emission".to_string(),
        ),
        aelys_syntax::Span::dummy(),
        source.clone(),
    )
    .into()
}

/// escaped cycle a `none` (a hard compile error here) rather than a stack
const MAX_PROJECTION_HOPS: usize = 64;

// returns none for the poison marker so a poisoned type can never reach the bytecode schema
fn type_descriptor(
    ty: &aelys_sema::InferType,
    type_table: &aelys_sema::TypeTable,
) -> Option<TypeDescriptor> {
    type_descriptor_bounded(ty, type_table, 0)
}

fn type_descriptor_bounded(
    ty: &aelys_sema::InferType,
    type_table: &aelys_sema::TypeTable,
    projection_hops: usize,
) -> Option<TypeDescriptor> {
    use aelys_sema::InferType;
    let descriptor = match ty {
        InferType::I8 => TypeDescriptor::Int(IntWidth::I8),
        InferType::I16 => TypeDescriptor::Int(IntWidth::I16),
        InferType::I32 => TypeDescriptor::Int(IntWidth::I32),
        InferType::I64 | InferType::Numeric => TypeDescriptor::Int(IntWidth::I64),
        InferType::U8 => TypeDescriptor::Int(IntWidth::U8),
        InferType::U16 => TypeDescriptor::Int(IntWidth::U16),
        InferType::U32 => TypeDescriptor::Int(IntWidth::U32),
        InferType::U64 => TypeDescriptor::Int(IntWidth::U64),
        InferType::F32 => TypeDescriptor::Float(FloatWidth::F32),
        InferType::F64 => TypeDescriptor::Float(FloatWidth::F64),
        InferType::Bool => TypeDescriptor::Bool,
        InferType::String => TypeDescriptor::String,
        InferType::Unit => TypeDescriptor::Unit,
        InferType::Null => return None,
        InferType::Applied { name, args } => {
            let instance = type_table.nominal_instance_for(name, args)?;
            if type_table.has_enum(instance) {
                TypeDescriptor::Enum(enum_schema_id(instance, type_table)?)
            } else if type_table.has_struct(instance) {
                TypeDescriptor::Struct(struct_schema_id(instance, type_table)?)
            } else {
                return None;
            }
        }
        InferType::Dynamic | InferType::Var(_) | InferType::Param(_) => return None,
        InferType::Projection { .. } => {
            if projection_hops >= MAX_PROJECTION_HOPS {
                return None;
            }
            let resolved = type_table.resolve_projection(ty)?;
            return type_descriptor_bounded(&resolved, type_table, projection_hops + 1);
        }
        InferType::Error => TypeDescriptor::Error,
        InferType::Struct(name) if type_table.has_enum(name) => {
            TypeDescriptor::Enum(enum_schema_id(name, type_table)?)
        }
        InferType::Struct(name) if type_table.has_struct(name) => {
            TypeDescriptor::Struct(struct_schema_id(name, type_table)?)
        }
        InferType::Struct(_) => return None,
        InferType::Option(inner) => TypeDescriptor::Option(Box::new(type_descriptor_bounded(
            inner,
            type_table,
            projection_hops,
        )?)),
        InferType::Result(ok, err) => TypeDescriptor::Result(
            Box::new(type_descriptor_bounded(ok, type_table, projection_hops)?),
            Box::new(type_descriptor_bounded(err, type_table, projection_hops)?),
        ),
        InferType::Array(inner) => TypeDescriptor::Array(Box::new(type_descriptor_bounded(
            inner,
            type_table,
            projection_hops,
        )?)),
        InferType::FixedArray(inner, len) => TypeDescriptor::FixedArray(
            Box::new(type_descriptor_bounded(inner, type_table, projection_hops)?),
            u32::try_from(*len).unwrap_or(u32::MAX),
        ),
        InferType::Vec(inner) => TypeDescriptor::Vec(Box::new(type_descriptor_bounded(
            inner,
            type_table,
            projection_hops,
        )?)),
        InferType::Tuple(_) | InferType::Never => return None,
        InferType::Function { params, ret } => TypeDescriptor::Function {
            params: params
                .iter()
                .map(|param| type_descriptor_bounded(param, type_table, projection_hops))
                .collect::<Option<Vec<_>>>()?
                .into_boxed_slice(),
            ret: Box::new(type_descriptor_bounded(ret, type_table, projection_hops)?),
        },
        InferType::UntypedNative(_) | InferType::Range => return None,
        InferType::Poison => return None,
    };
    Some(descriptor)
}

fn enum_schema_id(name: &str, type_table: &aelys_sema::TypeTable) -> Option<u16> {
    let short_name = name.rsplit("::").next().unwrap_or(name);
    type_table
        .enum_schema_index(name)
        .or_else(|| type_table.enum_schema_index(short_name))
}

fn struct_schema_id(name: &str, type_table: &aelys_sema::TypeTable) -> Option<u32> {
    let short_name = name.rsplit("::").next().unwrap_or(name);
    type_table
        .schema_index(name)
        .or_else(|| type_table.schema_index(short_name))
        .map(u32::from)
}
