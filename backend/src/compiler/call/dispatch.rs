use super::super::Compiler;
use super::util::call_window_available;
use aelys_common::Result;
use aelys_common::error::{CompileError, CompileErrorKind};
use aelys_syntax::Span;
use aelys_syntax::ast::{Expr, ExprKind, FmtStringPart};

impl Compiler {
    // call dispatch: try specialized opcodes first, fall back to generic call.
    pub fn compile_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        dest: u16,
        span: Span,
    ) -> Result<()> {
        if args.len() > usize::from(u16::MAX) {
            return Err(CompileError::new(
                CompileErrorKind::TooManyArguments,
                span,
                self.source.clone(),
            )
            .into());
        }
        if args.len() > usize::from(u8::MAX) {
            return self.compile_call_generic(callee, args, dest, span);
        }

        if let Some((fmt_parts, placeholder_count)) = Self::get_fmt_string_placeholders(args)
            && placeholder_count > 0
        {
            return self.compile_call_with_fmt_placeholders(
                callee,
                args,
                fmt_parts,
                placeholder_count,
                dest,
                span,
            );
        }

        if self.try_compile_builtin_call(callee, args, dest, span)? {
            return Ok(());
        }

        if self.try_compile_upvalue_call(callee, args, dest, span)? {
            return Ok(());
        }

        if self.try_compile_module_call(callee, args, dest, span)? {
            return Ok(());
        }

        // callcached: when the callee is in a local register (e.g., let f = func; f())
        if self.try_compile_cached_call(callee, args, dest, span)? {
            return Ok(());
        }

        if self.try_compile_global_call(callee, args, dest, span)? {
            return Ok(());
        }

        self.compile_call_generic(callee, args, dest, span)
    }

    fn get_fmt_string_placeholders(args: &[Expr]) -> Option<(&[FmtStringPart], usize)> {
        if args.is_empty() {
            return None;
        }
        if let ExprKind::FmtString(parts) = &args[0].kind {
            let count = parts
                .iter()
                .filter(|p| matches!(p, FmtStringPart::Placeholder))
                .count();
            return Some((parts, count));
        }
        None
    }

    fn compile_call_with_fmt_placeholders(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        fmt_parts: &[FmtStringPart],
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

        self.compile_expr(callee, func_reg)?;

        let fmt_reg = func_reg + 1;
        self.compile_fmt_string(fmt_parts, fmt_extra_args, fmt_reg, args[0].span)?;

        for (i, arg) in remaining_args.iter().enumerate() {
            let arg_reg =
                func_reg + 2 + u16::try_from(i).expect("register offset was range checked");
            self.compile_expr(arg, arg_reg)?;
        }

        self.emit_c(
            aelys_bytecode::OpCode::Call,
            dest,
            func_reg,
            call_arity,
            span,
        );

        for i in (0..=total_args).rev() {
            let reg = func_reg + u16::try_from(i).expect("register offset was range checked");
            self.free_register(reg);
        }

        Ok(())
    }

    pub(super) fn reserve_arg_registers(&mut self, start: u16, args_len: usize) -> bool {
        if !call_window_available(&self.register_pool, self.next_register, start, args_len) {
            return false;
        }
        for i in 0..args_len {
            let arg_reg = start + u16::try_from(i).expect("register offset was range checked");
            self.register_pool[arg_reg as usize] = true;
            if u32::from(arg_reg) >= self.next_register {
                self.next_register = u32::from(arg_reg) + 1;
            }
        }
        true
    }

    pub(super) fn release_arg_registers(&mut self, start: u16, args_len: usize) {
        for i in (0..args_len).rev() {
            let arg_reg = start + u16::try_from(i).expect("register offset was range checked");
            self.free_register(arg_reg);
        }
    }

    pub(super) fn checked_arg_start(&self, dest: u16) -> Option<u16> {
        dest.checked_add(1)
    }

    pub(super) fn is_member_call(
        callee: &Expr,
    ) -> Option<(String, String, aelys_syntax::MemberSeparator)> {
        if let ExprKind::Member {
            object,
            member,
            separator,
        } = &callee.kind
            && let Some(module_name) = Self::path_name(object)
        {
            return Some((module_name, member.clone(), *separator));
        }
        None
    }

    pub(super) fn path_name(expr: &Expr) -> Option<String> {
        match &expr.kind {
            ExprKind::Identifier(name) => Some(name.clone()),
            ExprKind::Member {
                object,
                member,
                separator: aelys_syntax::MemberSeparator::Path,
            } => {
                let mut path = Self::path_name(object)?;
                path.push_str("::");
                path.push_str(member);
                Some(path)
            }
            _ => None,
        }
    }

    pub(crate) fn has_module_alias(&self, path: &str) -> bool {
        self.module_aliases.contains(path)
            || path
                .split_once("::")
                .is_some_and(|(root, _)| self.module_aliases.contains(root))
    }
}
