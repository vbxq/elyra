use super::super::Compiler;
use crate::compiler::call::util::call_window_available;
use aelys_bytecode::OpCode;
use aelys_common::Result;
use aelys_common::error::{CompileError, CompileErrorKind};
use aelys_sema::{InferType, TypedFmtStringPart};
use aelys_syntax::Span;

impl Compiler {
    pub(super) fn compile_typed_call(
        &mut self,
        callee: &aelys_sema::TypedExpr,
        args: &[aelys_sema::TypedExpr],
        dest: u16,
        span: Span,
    ) -> Result<()> {
        use aelys_sema::TypedExprKind;

        let call_arity = self.checked_call_arity(args.len(), span)?;
        if call_arity > u16::from(u8::MAX) {
            return self.compile_typed_call_fallback(callee, args, dest, span);
        }

        if self.compile_typed_sum_constructor(callee, args, dest, span)? {
            return Ok(());
        }

        if let TypedExprKind::StructMethod {
            object,
            symbol,
            separator,
            ..
        } = &callee.kind
        {
            let has_receiver = *separator == aelys_syntax::MemberSeparator::Dot;
            let total_args = args.len() + usize::from(has_receiver);
            let arg_start = dest.checked_add(1).ok_or_else(|| {
                aelys_common::error::CompileError::new(
                    CompileErrorKind::TooManyRegisters,
                    span,
                    self.source.clone(),
                )
            })?;
            let call_global = self.get_or_create_global_index(symbol);
            self.accessed_globals.insert(symbol.clone());
            if !call_window_available(
                &self.register_pool,
                self.next_register,
                arg_start,
                total_args,
            ) {
                return self.compile_typed_call_fallback(callee, args, dest, span);
            }
            let mut reserved = Vec::with_capacity(total_args);
            for index in 0..total_args {
                let register =
                    arg_start + u16::try_from(index).expect("the call window was range checked");
                self.register_pool[register as usize] = true;
                self.next_register = self.next_register.max(u32::from(register) + 1);
                reserved.push(register);
            }
            let mut position = 0;
            if has_receiver {
                self.compile_typed_expr(object, reserved[0])?;
                position = 1;
            }
            for (index, arg) in args.iter().enumerate() {
                self.compile_typed_expr(arg, reserved[position + index])?;
            }
            let arity = self.checked_call_arity(total_args, span)?;
            self.emit_call_global_cached(dest, call_global, arity, symbol, span);
            for register in reserved.into_iter().rev() {
                self.free_register(register);
            }
            return Ok(());
        }

        if let Some((fmt_parts, placeholder_count)) = Self::get_typed_fmt_placeholders(args)
            && placeholder_count > 0
        {
            return self.compile_typed_call_with_fmt_placeholders(
                callee,
                args,
                fmt_parts,
                placeholder_count,
                dest,
                span,
            );
        }

        if let TypedExprKind::Member {
            object,
            member,
            separator,
        } = &callee.kind
        {
            if self.compile_typed_sum_method_call(object, member, args, dest, span)? {
                return Ok(());
            }

            if matches!(
                member.as_str(),
                "iter" | "collect" | "map" | "filter" | "fold"
            ) {
                return self.compile_typed_collection_pipeline(
                    object, member, args, &callee.ty, dest, span,
                );
            }

            if matches!(
                &object.ty,
                InferType::Array(_) | InferType::FixedArray(_, _)
            ) {
                match member.as_str() {
                    "len" if args.is_empty() => {
                        return self.compile_array_len(object, dest, span);
                    }
                    "is_empty" if args.is_empty() => {
                        return self.compile_collection_is_empty(object, dest, span);
                    }
                    "get" if args.len() == 1 => {
                        return self.compile_typed_collection_get(object, &args[0], dest, span);
                    }
                    _ => {}
                }
            }

            if let InferType::Vec(inner) = &object.ty {
                match member.as_str() {
                    "len" if args.is_empty() => {
                        return self.compile_vec_len(object, dest, span);
                    }
                    "is_empty" if args.is_empty() => {
                        return self.compile_collection_is_empty(object, dest, span);
                    }
                    "push" if args.len() == 1 => {
                        return self.compile_vec_push(object, inner, &args[0], dest, span);
                    }
                    "pop" if args.is_empty() => {
                        return self.compile_vec_pop(object, inner, dest, span);
                    }
                    "get" if args.len() == 1 => {
                        return self.compile_typed_collection_get(object, &args[0], dest, span);
                    }
                    "capacity" if args.is_empty() => {
                        return self.compile_vec_capacity(object, dest, span);
                    }
                    "reserve" if args.len() == 1 => {
                        return self.compile_vec_reserve(object, &args[0], dest, span);
                    }
                    _ => {}
                }
            }

            if matches!(&object.ty, InferType::String)
                && let Some(expected_args) = Self::string_method_arity(member)
                && args.len() == expected_args
            {
                return self.compile_string_method_call(object, member, args, dest, span);
            }

            if member == "to_string" && args.is_empty() {
                return self.compile_tostring_method(object, dest, span);
            }

            // module alias calls must be checked before dynamic dispatch,
            if *separator == aelys_syntax::MemberSeparator::Path
                && let Some(module_name) = Self::typed_path_name(object)
                && self.has_module_alias(&module_name)
            {
                let qualified_name = format!("{}::{}", module_name, member);
                let global_idx = self.get_or_create_global_index(&qualified_name);
                self.accessed_globals.insert(qualified_name.clone());

                let arg_start = match dest.checked_add(1) {
                    Some(s) => s,
                    None => {
                        return self.compile_typed_call_fallback(callee, args, dest, span);
                    }
                };

                if call_window_available(
                    &self.register_pool,
                    self.next_register,
                    arg_start,
                    args.len(),
                ) {
                    for i in 0..args.len() {
                        let arg_reg = arg_start
                            + u16::try_from(i).expect("register offset was range checked");
                        self.register_pool[arg_reg as usize] = true;
                        if u32::from(arg_reg) >= self.next_register {
                            self.next_register = u32::from(arg_reg) + 1;
                        }
                    }

                    for (i, arg) in args.iter().enumerate() {
                        let arg_reg = arg_start
                            + u16::try_from(i).expect("register offset was range checked");
                        self.compile_typed_expr(arg, arg_reg)?;
                    }

                    let nargs = self.checked_call_arity(args.len(), span)?;
                    self.emit_call_global_cached(dest, global_idx, nargs, &qualified_name, span);

                    for i in (0..args.len()).rev() {
                        let arg_reg = arg_start
                            + u16::try_from(i).expect("register offset was range checked");
                        self.free_register(arg_reg);
                    }

                    return Ok(());
                }
            }

            if *separator == aelys_syntax::MemberSeparator::Dot
                && let Some(module_name) = Self::typed_path_name(object)
                && self.has_module_alias(&module_name)
            {
                return Err(aelys_common::error::AelysError::Compile(
                    aelys_common::error::CompileError::new(
                        aelys_common::error::CompileErrorKind::ModulePathSeparator {
                            module: module_name.clone(),
                            member: member.clone(),
                        },
                        span,
                        self.source.clone(),
                    ),
                ));
            }

            if matches!(&object.ty, InferType::Dynamic | InferType::Var(_)) {
                match member.as_str() {
                    "len" if args.is_empty() => {
                        return self.compile_vec_len(object, dest, span);
                    }
                    "is_empty" if args.is_empty() => {
                        return self.compile_collection_is_empty(object, dest, span);
                    }
                    "push" if args.len() == 1 => {
                        return self.compile_vec_push(
                            object,
                            &InferType::Dynamic,
                            &args[0],
                            dest,
                            span,
                        );
                    }
                    "pop" if args.is_empty() => {
                        return self.compile_vec_pop(object, &InferType::Dynamic, dest, span);
                    }
                    "capacity" if args.is_empty() => {
                        return self.compile_vec_capacity(object, dest, span);
                    }
                    "reserve" if args.len() == 1 => {
                        return self.compile_vec_reserve(object, &args[0], dest, span);
                    }
                    _ => {}
                }

                if let Some(expected_args) = Self::string_method_arity(member)
                    && args.len() == expected_args
                {
                    return self.compile_string_method_call(object, member, args, dest, span);
                }
            }
        }

        if let TypedExprKind::Identifier(name) = &callee.kind {
            if Self::is_builtin(name) {
                return self.compile_typed_builtin_call(name, args, dest, span);
            }

            if self.resolve_variable(name).is_none() && self.resolve_upvalue(name).is_none() {
                if !self.globals.contains_key(name) && !self.known_globals.contains(name) {
                    return self.compile_typed_call_fallback(callee, args, dest, span);
                }
                let actual_name = self.resolve_global_name(name).to_string();
                let global_idx = self.get_or_create_global_index(name);
                self.accessed_globals.insert(actual_name.clone());

                let arg_start = match dest.checked_add(1) {
                    Some(s) => s,
                    None => return self.compile_typed_call_fallback(callee, args, dest, span),
                };

                if call_window_available(
                    &self.register_pool,
                    self.next_register,
                    arg_start,
                    args.len(),
                ) {
                    for i in 0..args.len() {
                        let arg_reg = arg_start
                            + u16::try_from(i).expect("register offset was range checked");
                        self.register_pool[arg_reg as usize] = true;
                        if u32::from(arg_reg) >= self.next_register {
                            self.next_register = u32::from(arg_reg) + 1;
                        }
                    }

                    for (i, arg) in args.iter().enumerate() {
                        let arg_reg = arg_start
                            + u16::try_from(i).expect("register offset was range checked");
                        self.compile_typed_expr(arg, arg_reg)?;
                    }

                    let nargs = self.checked_call_arity(args.len(), span)?;
                    self.emit_call_global_cached(dest, global_idx, nargs, name, span);

                    for i in (0..args.len()).rev() {
                        let arg_reg = arg_start
                            + u16::try_from(i).expect("register offset was range checked");
                        self.free_register(arg_reg);
                    }

                    return Ok(());
                }
            }
        }

        if let aelys_sema::TypedExprKind::Identifier(name) = &callee.kind
            && let Some((callee_reg, _mutable)) = self.resolve_variable(name)
        {
            let nargs = args.len();
            let Some(arg_start) = dest.checked_add(1) else {
                return self.compile_typed_call_fallback(callee, args, dest, span);
            };
            if call_window_available(&self.register_pool, self.next_register, arg_start, nargs) {
                for i in 0..nargs {
                    let reg_idx = (arg_start
                        + u16::try_from(i).expect("register offset was range checked"))
                        as usize;
                    self.register_pool[reg_idx] = true;
                    let reg = u16::try_from(reg_idx).expect("register index was range checked");
                    if u32::from(reg) >= self.next_register {
                        self.next_register = u32::from(reg) + 1;
                    }
                }
                for (i, arg) in args.iter().enumerate() {
                    self.compile_typed_expr(
                        arg,
                        arg_start + u16::try_from(i).expect("register offset was range checked"),
                    )?;
                }
                let call_arity = self.checked_call_arity(nargs, span)?;
                self.emit_c(OpCode::CallCached, dest, callee_reg, call_arity, span);
                for i in (0..nargs).rev() {
                    self.register_pool[(arg_start
                        + u16::try_from(i).expect("register offset was range checked"))
                        as usize] = false;
                }
                return Ok(());
            }
        }

        self.compile_typed_call_fallback(callee, args, dest, span)
    }

    pub(super) fn compile_typed_call_fallback(
        &mut self,
        callee: &aelys_sema::TypedExpr,
        args: &[aelys_sema::TypedExpr],
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let receiver = match &callee.kind {
            aelys_sema::TypedExprKind::StructMethod {
                object, separator, ..
            } if *separator == aelys_syntax::MemberSeparator::Dot => Some(object.as_ref()),
            _ => None,
        };
        let leading = usize::from(receiver.is_some());
        let nargs = args.len().saturating_add(leading);
        let callee_reg =
            self.alloc_consecutive_registers_for_call(nargs.saturating_add(1), span)?;

        for i in 0..=nargs {
            let reg = callee_reg + u16::try_from(i).expect("register offset was range checked");
            self.register_pool[reg as usize] = true;
            if u32::from(reg) >= self.next_register {
                self.next_register = u32::from(reg) + 1;
            }
        }

        self.compile_typed_expr(callee, callee_reg)?;

        // the successful allocation above bounds this walk, so no offset here can leave the window
        let mut arg_reg = callee_reg;
        if let Some(object) = receiver {
            arg_reg += 1;
            self.compile_typed_expr(object, arg_reg)?;
        }
        for arg in args {
            arg_reg += 1;
            self.compile_typed_expr(arg, arg_reg)?;
        }

        let call_arity = self.checked_call_arity(nargs, span)?;
        self.emit_c(OpCode::Call, dest, callee_reg, call_arity, span);

        for i in (0..=nargs).rev() {
            let reg = callee_reg + u16::try_from(i).expect("register offset was range checked");
            self.free_register(reg);
        }

        Ok(())
    }

    fn string_method_arity(method: &str) -> Option<usize> {
        match method {
            "len" | "char_len" | "chars" | "bytes" | "to_upper" | "to_lower" | "capitalize"
            | "trim" | "trim_start" | "trim_end" | "is_empty" | "is_whitespace" | "is_numeric"
            | "is_alphabetic" | "is_alphanumeric" | "reverse" | "lines" | "line_count" => Some(0),
            "char_at" | "byte_at" | "contains" | "starts_with" | "ends_with" | "find" | "rfind"
            | "count" | "split" | "repeat" | "concat" => Some(1),
            "substr" | "replace" | "replace_first" | "pad_left" | "pad_right" => Some(2),
            "join" => Some(1),
            _ => None,
        }
    }

    fn compile_string_method_call(
        &mut self,
        object: &aelys_sema::TypedExpr,
        method: &str,
        args: &[aelys_sema::TypedExpr],
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let qualified_name = format!("string::{}", method);
        let total_args = 1 + args.len(); // self + extra args

        let global_idx = self.get_or_create_global_index(&qualified_name);
        self.accessed_globals.insert(qualified_name.clone());

        if global_idx <= 255 {
            let arg_start = match dest.checked_add(1) {
                Some(s) => s,
                None => {
                    return self.compile_string_method_call_fallback(
                        object,
                        &qualified_name,
                        args,
                        dest,
                        span,
                    );
                }
            };

            let mut can_use_callglobal = true;
            for i in 0..total_args {
                let arg_reg = match arg_start
                    .checked_add(u16::try_from(i).expect("register offset was range checked"))
                {
                    Some(r) => r,
                    None => {
                        can_use_callglobal = false;
                        break;
                    }
                };
                if (arg_reg as usize) >= self.register_pool.len()
                    || self.register_pool[arg_reg as usize]
                {
                    can_use_callglobal = false;
                    break;
                }
            }

            if can_use_callglobal {
                for i in 0..total_args {
                    let arg_reg =
                        arg_start + u16::try_from(i).expect("register offset was range checked");
                    self.register_pool[arg_reg as usize] = true;
                    if u32::from(arg_reg) >= self.next_register {
                        self.next_register = u32::from(arg_reg) + 1;
                    }
                }

                self.compile_typed_expr(object, arg_start)?;

                for (i, arg) in args.iter().enumerate() {
                    let arg_reg = arg_start
                        + 1
                        + u16::try_from(i).expect("register offset was range checked");
                    self.compile_typed_expr(arg, arg_reg)?;
                }

                let call_arity = self.checked_call_arity(total_args, span)?;
                self.emit_call_global_cached(dest, global_idx, call_arity, &qualified_name, span);

                for i in (0..total_args).rev() {
                    let arg_reg =
                        arg_start + u16::try_from(i).expect("register offset was range checked");
                    self.free_register(arg_reg);
                }

                return Ok(());
            }
        }

        self.compile_string_method_call_fallback(object, &qualified_name, args, dest, span)
    }

    fn compile_string_method_call_fallback(
        &mut self,
        object: &aelys_sema::TypedExpr,
        qualified_name: &str,
        args: &[aelys_sema::TypedExpr],
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let total_args = 1 + args.len();
        let call_arity = self.checked_call_arity(total_args, span)?;
        let callee_reg =
            self.alloc_consecutive_registers_for_call(total_args.saturating_add(1), span)?;

        for i in 0..=total_args {
            let reg = callee_reg + u16::try_from(i).expect("register offset was range checked");
            self.register_pool[reg as usize] = true;
            if u32::from(reg) >= self.next_register {
                self.next_register = u32::from(reg) + 1;
            }
        }

        let global_idx = self.get_or_create_global_index(qualified_name);
        self.accessed_globals.insert(qualified_name.to_string());
        self.emit_get_global_index(callee_reg, global_idx, span);

        self.compile_typed_expr(object, callee_reg + 1)?;

        for (i, arg) in args.iter().enumerate() {
            let arg_reg =
                callee_reg + 2 + u16::try_from(i).expect("register offset was range checked");
            self.compile_typed_expr(arg, arg_reg)?;
        }

        self.emit_a(OpCode::Call, dest, callee_reg, call_arity, span);

        for i in (0..=total_args).rev() {
            let reg = callee_reg + u16::try_from(i).expect("register offset was range checked");
            self.free_register(reg);
        }

        Ok(())
    }

    fn compile_tostring_method(
        &mut self,
        object: &aelys_sema::TypedExpr,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let qualified_name = "__tostring";
        let global_idx = self.get_or_create_global_index(qualified_name);
        self.accessed_globals.insert(qualified_name.to_string());

        if global_idx <= 255 {
            let arg_start = match dest.checked_add(1) {
                Some(s)
                    if (s as usize) < self.register_pool.len()
                        && !self.register_pool[s as usize] =>
                {
                    s
                }
                _ => {
                    return self.compile_tostring_method_fallback(object, dest, span);
                }
            };

            self.register_pool[arg_start as usize] = true;
            if u32::from(arg_start) >= self.next_register {
                self.next_register = u32::from(arg_start) + 1;
            }

            self.compile_typed_expr(object, arg_start)?;

            self.emit_call_global_cached(dest, global_idx, 1, qualified_name, span);

            self.free_register(arg_start);
            return Ok(());
        }

        self.compile_tostring_method_fallback(object, dest, span)
    }

    fn compile_tostring_method_fallback(
        &mut self,
        object: &aelys_sema::TypedExpr,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let callee_reg = self.alloc_consecutive_registers_for_call(2, span)?;

        self.register_pool[callee_reg as usize] = true;
        self.register_pool[(callee_reg + 1) as usize] = true;
        if u32::from(callee_reg) + 1 >= self.next_register {
            self.next_register = u32::from(callee_reg) + 2;
        }

        let global_idx = self.get_or_create_global_index("__tostring");
        self.accessed_globals.insert("__tostring".to_string());
        self.emit_get_global_index(callee_reg, global_idx, span);
        self.compile_typed_expr(object, callee_reg + 1)?;

        self.emit_a(OpCode::Call, dest, callee_reg, 1, span);

        self.free_register(callee_reg + 1);
        self.free_register(callee_reg);

        Ok(())
    }

    fn get_typed_fmt_placeholders(
        args: &[aelys_sema::TypedExpr],
    ) -> Option<(&[TypedFmtStringPart], usize)> {
        use aelys_sema::TypedExprKind;
        if args.is_empty() {
            return None;
        }
        if let TypedExprKind::FmtString(parts) = &args[0].kind {
            let count = parts
                .iter()
                .filter(|p| matches!(p, TypedFmtStringPart::Placeholder))
                .count();
            return Some((parts, count));
        }
        None
    }

    fn compile_typed_call_with_fmt_placeholders(
        &mut self,
        callee: &aelys_sema::TypedExpr,
        args: &[aelys_sema::TypedExpr],
        fmt_parts: &[TypedFmtStringPart],
        placeholder_count: usize,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let extra_args_needed = placeholder_count;
        let extra_args_available = args.len() - 1;

        if extra_args_available < extra_args_needed {
            return Err(CompileError::new(
                CompileErrorKind::TypeInferenceError(format!(
                    "format string has {} placeholder(s) but only {} argument(s) provided",
                    extra_args_needed, extra_args_available
                )),
                span,
                self.source.clone(),
            )
            .into());
        }

        let fmt_extra_args = &args[1..1 + extra_args_needed];
        let remaining_args = &args[1 + extra_args_needed..];

        let total_args = 1 + remaining_args.len();
        let call_arity = self.checked_call_arity(total_args, span)?;
        let func_reg =
            self.alloc_consecutive_registers_for_call(total_args.saturating_add(1), span)?;

        for i in 0..=total_args {
            let reg = func_reg + u16::try_from(i).expect("register offset was range checked");
            self.register_pool[reg as usize] = true;
            if u32::from(reg) >= self.next_register {
                self.next_register = u32::from(reg) + 1;
            }
        }

        self.compile_typed_expr(callee, func_reg)?;

        let fmt_reg = func_reg + 1;
        self.compile_typed_fmt_string(fmt_parts, fmt_extra_args, fmt_reg, args[0].span)?;

        for (i, arg) in remaining_args.iter().enumerate() {
            let arg_reg =
                func_reg + 2 + u16::try_from(i).expect("register offset was range checked");
            self.compile_typed_expr(arg, arg_reg)?;
        }

        self.emit_a(OpCode::Call, dest, func_reg, call_arity, span);

        for i in (0..=total_args).rev() {
            let reg = func_reg + u16::try_from(i).expect("register offset was range checked");
            self.free_register(reg);
        }

        Ok(())
    }
}
