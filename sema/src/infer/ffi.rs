use super::TypeInference;
use crate::constraint::{Constraint, ConstraintReason, TypeError, TypeErrorKind};
use crate::typed_ast::{TypedExpr, TypedExprKind, TypedExternFunction, TypedStmtKind};
use crate::types::{InferType, ParamModes};
use aelys_common::ffi::{FfiSignature, FfiType};
use aelys_syntax::{
    Expr, ExprKind, ExternBlock, ExternFunction, MemberSeparator, ReferenceKind, Span, Stmt,
    StmtKind, TypeAnnotation,
};
use std::rc::Rc;

#[derive(Debug, Clone)]
pub(crate) struct FfiDecl {
    pub(crate) library: String,
    pub(crate) signature: FfiSignature,
    pub(crate) params: Vec<InferType>,
    pub(crate) ret: InferType,
    pub(crate) valid: bool,
}

enum Refusal {
    Callback,
    Type { help: &'static str },
}

const SCALAR_HELP: &str = "an extern signature takes an integer type, `f32`, `float`, `bool`, `ptr`, `cstr`, `&[u8]` or `&mut [u8]`";
const BUFFER_HELP: &str = "a byte buffer is `&[u8]` or `&mut [u8]` (a `Vec<u8>` lent for the call); any other pointer is a `ptr`";
const STRUCT_HELP: &str = "structs are not passed by value: pass a pointer to their bytes (`&[u8]`, `&mut [u8]` or a `ptr`)";

fn written(ann: &TypeAnnotation) -> String {
    let mut text = String::new();
    match ann.reference {
        Some(ReferenceKind::Mutable) => text.push_str("&mut "),
        Some(ReferenceKind::Shared) => text.push('&'),
        None => {}
    }
    if let Some(params) = &ann.fn_params {
        let params: Vec<String> = params.iter().map(written).collect();
        text.push_str(&format!("fn({})", params.join(", ")));
        if let Some(ret) = &ann.fn_ret {
            text.push_str(&format!(" -> {}", written(ret)));
        }
        return text;
    }
    if ann.name == "slice" {
        let inner = ann.type_params.first().map(written).unwrap_or_default();
        text.push_str(&format!("[{inner}]"));
        return text;
    }
    if let Some(length) = ann.array_length {
        let inner = ann.type_params.first().map(written).unwrap_or_default();
        text.push_str(&format!("[{inner}; {length}]"));
        return text;
    }
    text.push_str(&ann.path.join("::"));
    if !ann.type_params.is_empty() {
        let params: Vec<String> = ann.type_params.iter().map(written).collect();
        text.push_str(&format!("<{}>", params.join(", ")));
    }
    text
}

fn classify(ann: &TypeAnnotation, is_return: bool) -> Result<(FfiType, InferType), Refusal> {
    if ann.fn_params.is_some() {
        return Err(if is_return {
            Refusal::Type {
                help: "a C function pointer is a `ptr`",
            }
        } else {
            Refusal::Callback
        });
    }
    if let Some(reference) = ann.reference {
        let bytes = ann.name == "slice"
            && ann.type_params.len() == 1
            && matches!(ann.type_params[0].name.as_str(), "u8" | "uint8")
            && ann.type_params[0].type_params.is_empty();
        if bytes && !is_return {
            let ffi = match reference {
                ReferenceKind::Shared => FfiType::Bytes,
                ReferenceKind::Mutable => FfiType::BytesMut,
            };
            return Ok((ffi, InferType::Vec(Box::new(InferType::U8))));
        }
        return Err(Refusal::Type {
            help: if is_return {
                "a returned pointer is a `ptr`"
            } else {
                BUFFER_HELP
            },
        });
    }
    if ann.array_length.is_some() || ann.array_length_path.is_some() {
        return Err(Refusal::Type { help: BUFFER_HELP });
    }
    if ann.path.len() == 1 && ann.type_params.is_empty() {
        let found = match ann.name.as_str() {
            "int" => Some((FfiType::I64, InferType::I64)),
            "i64" | "int64" => Some((FfiType::I64, InferType::Int64)),
            "i8" | "int8" => Some((FfiType::I8, InferType::I8)),
            "i16" | "int16" => Some((FfiType::I16, InferType::I16)),
            "i32" | "int32" => Some((FfiType::I32, InferType::I32)),
            "u8" | "uint8" => Some((FfiType::U8, InferType::U8)),
            "u16" | "uint16" => Some((FfiType::U16, InferType::U16)),
            "u32" | "uint32" => Some((FfiType::U32, InferType::U32)),
            "u64" | "uint64" => Some((FfiType::U64, InferType::U64)),
            "float" | "f64" | "float64" => Some((FfiType::F64, InferType::F64)),
            "f32" | "float32" => Some((FfiType::F32, InferType::F32)),
            "bool" => Some((FfiType::Bool, InferType::Bool)),
            "ptr" => Some((FfiType::Ptr, InferType::Ptr)),
            "cstr" => Some((FfiType::CStr, InferType::String)),
            "unit" | "void" if is_return => Some((FfiType::Void, InferType::Unit)),
            _ => None,
        };
        if let Some(found) = found {
            return Ok(found);
        }
        let help = match ann.name.as_str() {
            "string" => {
                "a C string is `cstr`: the Elyra string crosses as a NUL-terminated copy, and a returned `cstr` is copied back into a string"
            }
            "char" => "pass the scalar value as an integer: `c as u32`",
            "unit" | "void" => "a parameter cannot be unit",
            name if name.chars().next().is_some_and(char::is_uppercase) => STRUCT_HELP,
            _ => SCALAR_HELP,
        };
        return Err(Refusal::Type { help });
    }
    let help = match ann.name.to_lowercase().as_str() {
        "vec" | "array" => BUFFER_HELP,
        "option" | "result" | "map" => SCALAR_HELP,
        name if ann.name.chars().next().is_some_and(char::is_uppercase) && name != "map" => {
            STRUCT_HELP
        }
        _ => SCALAR_HELP,
    };
    Err(Refusal::Type { help })
}

fn variadic_class(ty: &InferType) -> Option<FfiType> {
    Some(match ty {
        InferType::I8 | InferType::I16 | InferType::I32 | InferType::U8 | InferType::U16 => {
            FfiType::I32
        }
        InferType::Bool => FfiType::I32,
        InferType::U32 => FfiType::U32,
        InferType::I64 | InferType::Int64 => FfiType::I64,
        InferType::U64 => FfiType::U64,
        InferType::F32 | InferType::F64 => FfiType::F64,
        InferType::Ptr => FfiType::Ptr,
        InferType::String => FfiType::CStr,
        _ => return None,
    })
}

fn variadic_value_type(class: FfiType, written: &InferType) -> InferType {
    match class {
        FfiType::CStr => InferType::String,
        _ => written.clone(),
    }
}

fn byte_modes(signature: &FfiSignature) -> ParamModes {
    ParamModes::from_flags(
        signature
            .params
            .iter()
            .map(|param| *param == FfiType::BytesMut),
    )
}

fn ptr_method_shape(member: &str, arity: usize) -> Option<(Vec<PtrOperand>, InferType)> {
    use PtrOperand::{AnyInt, Float, IntOf, Ptr};
    if let Some(width) = member.strip_prefix("read_") {
        if arity != 1 {
            return None;
        }
        let output = match width {
            "f32" | "f64" => InferType::F64,
            "ptr" => InferType::Ptr,
            other => int_width_type(other)?,
        };
        return Some((vec![AnyInt], output));
    }
    if let Some(width) = member.strip_prefix("write_") {
        if arity != 2 {
            return None;
        }
        let value = match width {
            "f32" | "f64" => Float,
            "ptr" => Ptr,
            other => IntOf(int_width_type(other)?),
        };
        return Some((vec![AnyInt, value], InferType::Unit));
    }
    match (member, arity) {
        ("is_null", 0) => Some((Vec::new(), InferType::Bool)),
        ("addr", 0) => Some((Vec::new(), InferType::U64)),
        ("offset", 1) => Some((vec![AnyInt], InferType::Ptr)),
        _ => None,
    }
}

pub(crate) fn int_width_type(width: &str) -> Option<InferType> {
    Some(match width {
        "i8" => InferType::I8,
        "i16" => InferType::I16,
        "i32" => InferType::I32,
        "i64" => InferType::Int64,
        "u8" => InferType::U8,
        "u16" => InferType::U16,
        "u32" => InferType::U32,
        "u64" => InferType::U64,
        "int" => InferType::I64,
        _ => return None,
    })
}

pub(crate) fn is_ptr_method(member: &str) -> bool {
    (0..=2).any(|arity| ptr_method_shape(member, arity).is_some())
}

#[derive(Clone)]
enum PtrOperand {
    AnyInt,
    IntOf(InferType),
    Float,
    Ptr,
}

impl TypeInference {
    pub(super) fn collect_extern_declarations(&mut self, stmts: &[Stmt]) {
        let functions: std::collections::HashSet<&str> = stmts
            .iter()
            .filter_map(|stmt| match &stmt.kind {
                StmtKind::Function(function) => Some(function.name.as_str()),
                StmtKind::Let { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        for stmt in stmts {
            let StmtKind::Extern(block) = &stmt.kind else {
                continue;
            };
            self.top_level_externs
                .insert((stmt.span.start, stmt.span.end));
            for function in &block.functions {
                if functions.contains(function.name.as_str())
                    || self.ffi_functions.contains_key(&function.name)
                {
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::DuplicateExtern {
                            name: function.name.clone(),
                        },
                        span: function.span,
                        reason: ConstraintReason::Other("extern declaration".to_string()),
                    });
                    continue;
                }
                self.declare_extern(block, function);
            }
        }
    }

    fn refuse_extern_type(
        &mut self,
        function: &ExternFunction,
        position: String,
        ann: &TypeAnnotation,
        refusal: Refusal,
        param: Option<&str>,
    ) {
        let kind = match (refusal, param) {
            (Refusal::Callback, Some(param)) => TypeErrorKind::ExternCallback {
                function: function.name.clone(),
                param: param.to_string(),
            },
            (Refusal::Callback, None) => TypeErrorKind::ExternUnsupportedType {
                function: function.name.clone(),
                position,
                written: written(ann),
                help: "a C function pointer is a `ptr`".to_string(),
            },
            (Refusal::Type { help }, _) => TypeErrorKind::ExternUnsupportedType {
                function: function.name.clone(),
                position,
                written: written(ann),
                help: help.to_string(),
            },
        };
        self.errors.push(TypeError {
            kind,
            span: ann.span,
            reason: ConstraintReason::Other("extern signature".to_string()),
        });
    }

    fn declare_extern(&mut self, block: &ExternBlock, function: &ExternFunction) {
        let mut valid = true;
        if function.is_pub {
            valid = false;
            self.errors.push(TypeError {
                kind: TypeErrorKind::ExternExport {
                    name: function.name.clone(),
                },
                span: function.span,
                reason: ConstraintReason::Other("extern declaration".to_string()),
            });
        }
        let mut ffi_params = Vec::with_capacity(function.params.len());
        let mut params = Vec::with_capacity(function.params.len());
        for (index, param) in function.params.iter().enumerate() {
            match classify(&param.type_annotation, false) {
                Ok((ffi, ty)) => {
                    ffi_params.push(ffi);
                    params.push(ty);
                }
                Err(refusal) => {
                    valid = false;
                    self.refuse_extern_type(
                        function,
                        format!("parameter {} `{}`", index + 1, param.name),
                        &param.type_annotation,
                        refusal,
                        Some(&param.name),
                    );
                    ffi_params.push(FfiType::Ptr);
                    params.push(InferType::Poison);
                }
            }
        }
        let (ffi_ret, ret) = match &function.return_type {
            None => (FfiType::Void, InferType::Unit),
            Some(ann) => match classify(ann, true) {
                Ok(found) => found,
                Err(refusal) => {
                    valid = false;
                    self.refuse_extern_type(
                        function,
                        "return type".to_string(),
                        ann,
                        refusal,
                        None,
                    );
                    (FfiType::Void, InferType::Poison)
                }
            },
        };
        let signature = FfiSignature {
            fixed: ffi_params.len(),
            params: ffi_params,
            variadic: function.variadic,
            ret: ffi_ret,
        };
        if !function.variadic {
            let modes = byte_modes(&signature);
            let references = signature
                .params
                .iter()
                .map(|param| match param {
                    FfiType::Bytes => Some(ReferenceKind::Shared),
                    FfiType::BytesMut => Some(ReferenceKind::Mutable),
                    _ => None,
                })
                .collect();
            self.function_reference_modes
                .insert(function.name.clone(), references);
            self.env.define_function(
                function.name.clone(),
                Rc::new(InferType::Function {
                    params: params.clone(),
                    modes,
                    ret: Box::new(ret.clone()),
                }),
            );
        }
        self.ffi_functions.insert(
            function.name.clone(),
            FfiDecl {
                library: block.library.clone(),
                signature,
                params,
                ret,
                valid,
            },
        );
    }

    pub(super) fn infer_extern_stmt(&mut self, block: &ExternBlock, span: Span) -> TypedStmtKind {
        if !self.top_level_externs.contains(&(span.start, span.end)) {
            self.errors.push(TypeError {
                kind: TypeErrorKind::ExternNotTopLevel,
                span,
                reason: ConstraintReason::Other("extern block".to_string()),
            });
            return TypedStmtKind::Extern(Vec::new());
        }
        let functions = block
            .functions
            .iter()
            .filter_map(|function| {
                let decl = self.ffi_functions.get(&function.name)?;
                (decl.valid && !decl.signature.variadic && decl.library == block.library).then(
                    || TypedExternFunction {
                        name: function.name.clone(),
                        library: block.library.clone(),
                        symbol: function.name.clone(),
                        signature: decl.signature.clone(),
                    },
                )
            })
            .collect();
        TypedStmtKind::Extern(functions)
    }

    pub(super) fn variadic_extern_value(&mut self, name: &str, span: Span) -> bool {
        let variadic = !self.env.binds_value(name)
            && self
                .ffi_functions
                .get(name)
                .is_some_and(|decl| decl.signature.variadic);
        if variadic {
            self.errors.push(TypeError {
                kind: TypeErrorKind::VariadicExternValue {
                    name: name.to_string(),
                },
                span,
                reason: ConstraintReason::Other("variable lookup".to_string()),
            });
        }
        variadic
    }

    pub(super) fn infer_extern_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
    ) -> Option<(TypedExprKind, InferType)> {
        let ExprKind::Identifier(name) = &callee.kind else {
            return None;
        };
        if self.env.binds_value(name) {
            return None;
        }
        let decl = self.ffi_functions.get(name)?.clone();
        let fixed = decl.signature.fixed;
        if args.len() < fixed || (!decl.signature.variadic && args.len() != fixed) {
            self.errors.push(TypeError {
                kind: TypeErrorKind::ArityMismatch {
                    expected: fixed,
                    found: args.len(),
                },
                span: callee.span,
                reason: ConstraintReason::Other(format!("call to extern function `{name}`")),
            });
        }
        let mut typed_args = Vec::with_capacity(args.len());
        let mut extras = Vec::new();
        for (index, arg) in args.iter().enumerate() {
            let ffi = decl.signature.params.get(index).copied();
            let expected = match ffi {
                Some(FfiType::Bytes) if index < fixed => Some(ReferenceKind::Shared),
                Some(FfiType::BytesMut) if index < fixed => Some(ReferenceKind::Mutable),
                _ => None,
            };
            let mut typed = self.infer_call_argument(arg, expected);
            if index < fixed {
                let reason = ConstraintReason::Argument {
                    func_name: name.clone(),
                    arg_index: index,
                };
                self.check_extern_argument(
                    decl.signature.params[index],
                    &decl.params[index],
                    &mut typed,
                    reason,
                );
            } else if decl.signature.variadic {
                match variadic_class(&typed.ty) {
                    Some(class) => extras.push(class),
                    None => {
                        if !matches!(typed.ty, InferType::Poison) {
                            self.errors.push(TypeError {
                                kind: TypeErrorKind::VariadicArgument {
                                    function: name.clone(),
                                    found: typed.ty.clone(),
                                },
                                span: typed.span,
                                reason: ConstraintReason::Other("variadic argument".to_string()),
                            });
                        }
                        extras.push(FfiType::I64);
                    }
                }
            }
            typed_args.push(typed);
        }
        let callee_name = if decl.signature.variadic {
            let mut params = decl.signature.params.clone();
            params.extend(extras.iter().copied());
            let signature = FfiSignature {
                params,
                fixed,
                variadic: true,
                ret: decl.signature.ret,
            };
            let spelled: Vec<&str> = extras.iter().map(|class| class.spelling()).collect();
            let instance = format!("{name}(...{})", spelled.join(","));
            if decl.valid && self.ffi_instance_names.insert(instance.clone()) {
                self.ffi_variadic_instances.push(TypedExternFunction {
                    name: instance.clone(),
                    library: decl.library.clone(),
                    symbol: name.clone(),
                    signature,
                });
            }
            instance
        } else {
            name.clone()
        };
        let params: Vec<InferType> = typed_args
            .iter()
            .enumerate()
            .map(|(index, arg)| match extras.get(index.wrapping_sub(fixed)) {
                Some(class) if index >= fixed => variadic_value_type(*class, &arg.ty),
                _ => arg.ty.clone(),
            })
            .collect();
        let typed_callee = TypedExpr::new(
            TypedExprKind::Identifier(callee_name),
            InferType::Function {
                params,
                modes: byte_modes(&decl.signature),
                ret: Box::new(decl.ret.clone()),
            },
            callee.span,
        );
        Some((
            TypedExprKind::Call {
                callee: Box::new(typed_callee),
                args: typed_args,
            },
            decl.ret,
        ))
    }

    fn check_extern_argument(
        &mut self,
        ffi: FfiType,
        expected: &InferType,
        arg: &mut TypedExpr,
        reason: ConstraintReason,
    ) {
        if matches!(arg.ty, InferType::Poison) || matches!(expected, InferType::Poison) {
            return;
        }
        if ffi.is_integer() {
            self.fit_int_literal(arg, expected);
            self.require_any_of(arg, expected, InferType::all_integer_types(), reason);
        } else if ffi.is_float() {
            if let TypedExprKind::Float(value) = &arg.kind
                && ffi == FfiType::F32
                && value.is_finite()
                && value.abs() <= f64::from(f32::MAX)
            {
                arg.ty = InferType::F32;
            }
            self.require_any_of(arg, expected, vec![InferType::F64, InferType::F32], reason);
        } else {
            self.constraints.push(Constraint::equal(
                expected.clone(),
                arg.ty.clone(),
                arg.span,
                reason,
            ));
        }
    }

    fn require_any_of(
        &mut self,
        arg: &TypedExpr,
        expected: &InferType,
        options: Vec<InferType>,
        reason: ConstraintReason,
    ) {
        match &arg.ty {
            InferType::Var(_) => self.constraints.push(Constraint::one_of(
                arg.ty.clone(),
                options,
                arg.span,
                reason,
            )),
            found if options.contains(found) => {}
            found => self.errors.push(TypeError {
                kind: TypeErrorKind::Mismatch {
                    expected: expected.clone(),
                    found: found.clone(),
                },
                span: arg.span,
                reason,
            }),
        }
    }

    pub(super) fn take_variadic_extern_stmt(&mut self) -> Option<crate::typed_ast::TypedStmt> {
        if self.ffi_variadic_instances.is_empty() {
            return None;
        }
        Some(crate::typed_ast::TypedStmt::new(
            TypedStmtKind::Extern(std::mem::take(&mut self.ffi_variadic_instances)),
            Span::dummy(),
        ))
    }

    pub(super) fn infer_ptr_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
    ) -> Option<(TypedExprKind, InferType)> {
        let ExprKind::Member {
            object,
            member,
            separator,
        } = &callee.kind
        else {
            return None;
        };
        match separator {
            MemberSeparator::Path => self.infer_ptr_function(object, member, args, callee.span),
            MemberSeparator::Dot => {
                if !is_ptr_method(member) {
                    return None;
                }
                let typed_object = self.infer_receiver(object);
                if typed_object.ty != InferType::Ptr {
                    return None;
                }
                let Some((operands, output)) = ptr_method_shape(member, args.len()) else {
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::NoSuchMember {
                            receiver: InferType::Ptr,
                            member: member.clone(),
                        },
                        span: callee.span,
                        reason: ConstraintReason::Other("ptr method".to_string()),
                    });
                    return Some((TypedExprKind::Unit, InferType::Poison));
                };
                let typed_args =
                    self.infer_ptr_operands(&format!("ptr::{member}"), args, &operands);
                let mut params = vec![];
                params.extend(typed_args.iter().map(|arg| arg.ty.clone()));
                let typed_callee = TypedExpr::new(
                    TypedExprKind::Member {
                        object: Box::new(typed_object),
                        member: member.clone(),
                        separator: MemberSeparator::Dot,
                    },
                    InferType::Function {
                        params,
                        modes: ParamModes::plain(),
                        ret: Box::new(output.clone()),
                    },
                    callee.span,
                );
                Some((
                    TypedExprKind::Call {
                        callee: Box::new(typed_callee),
                        args: typed_args,
                    },
                    output,
                ))
            }
        }
    }

    fn infer_ptr_function(
        &mut self,
        object: &Expr,
        member: &str,
        args: &[Expr],
        span: Span,
    ) -> Option<(TypedExprKind, InferType)> {
        let ExprKind::Identifier(head) = &object.kind else {
            return None;
        };
        if head != "ptr"
            || self.module_aliases.contains(head)
            || self.env.lookup(head).is_some()
            || self.type_table.has_nominal(head)
        {
            return None;
        }
        let (operands, output) = match member {
            "null" => (Vec::new(), InferType::Ptr),
            "alloc" => (vec![PtrOperand::AnyInt], InferType::Ptr),
            "free" => (vec![PtrOperand::Ptr], InferType::Unit),
            "to_string" => (vec![PtrOperand::Ptr], InferType::String),
            "from_addr" => (vec![PtrOperand::AnyInt], InferType::Ptr),
            _ => {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::NoSuchMember {
                        receiver: InferType::Ptr,
                        member: member.to_string(),
                    },
                    span,
                    reason: ConstraintReason::Other("ptr function".to_string()),
                });
                return Some((TypedExprKind::Unit, InferType::Poison));
            }
        };
        if args.len() != operands.len() {
            self.errors.push(TypeError {
                kind: TypeErrorKind::ArityMismatch {
                    expected: operands.len(),
                    found: args.len(),
                },
                span,
                reason: ConstraintReason::Other(format!("ptr::{member}")),
            });
        }
        let typed_args = self.infer_ptr_operands(&format!("ptr::{member}"), args, &operands);
        let typed_callee = TypedExpr::new(
            TypedExprKind::Member {
                object: Box::new(TypedExpr::new(
                    TypedExprKind::Unit,
                    InferType::Ptr,
                    object.span,
                )),
                member: member.to_string(),
                separator: MemberSeparator::Path,
            },
            InferType::Function {
                params: typed_args.iter().map(|arg| arg.ty.clone()).collect(),
                modes: ParamModes::plain(),
                ret: Box::new(output.clone()),
            },
            span,
        );
        Some((
            TypedExprKind::Call {
                callee: Box::new(typed_callee),
                args: typed_args,
            },
            output,
        ))
    }

    fn infer_ptr_operands(
        &mut self,
        function: &str,
        args: &[Expr],
        operands: &[PtrOperand],
    ) -> Vec<TypedExpr> {
        args.iter()
            .enumerate()
            .map(|(index, arg)| {
                let mut typed = self.infer_expr(arg);
                let reason = ConstraintReason::Argument {
                    func_name: function.to_string(),
                    arg_index: index,
                };
                match operands.get(index) {
                    Some(PtrOperand::AnyInt) => self.require_any_of(
                        &typed,
                        &InferType::I64,
                        InferType::all_integer_types(),
                        reason,
                    ),
                    Some(PtrOperand::IntOf(expected)) => {
                        self.fit_int_literal(&mut typed, expected);
                        self.require_any_of(
                            &typed,
                            expected,
                            InferType::all_integer_types(),
                            reason,
                        )
                    }
                    Some(PtrOperand::Float) => self.require_any_of(
                        &typed,
                        &InferType::F64,
                        vec![InferType::F64, InferType::F32],
                        reason,
                    ),
                    Some(PtrOperand::Ptr) => self.constraints.push(Constraint::equal(
                        InferType::Ptr,
                        typed.ty.clone(),
                        typed.span,
                        reason,
                    )),
                    None => {}
                }
                typed
            })
            .collect()
    }
}
