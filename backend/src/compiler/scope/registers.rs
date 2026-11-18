use super::super::Compiler;
use aelys_common::Result;
use aelys_common::error::{CompileError, CompileErrorKind};
use aelys_syntax::Span;

impl Compiler {
    pub fn alloc_register(&mut self) -> Result<u16> {
        let start = self.register_search_start.min(self.register_pool.len());
        let register_index = self.register_pool[start..]
            .iter()
            .position(|used| !used)
            .map(|offset| start + offset)
            .or_else(|| self.register_pool[..start].iter().position(|used| !used))
            .ok_or_else(|| {
                CompileError::new(
                    CompileErrorKind::TooManyRegisters,
                    Span::dummy(),
                    self.source.clone(),
                )
            })?;
        self.register_pool[register_index] = true;
        self.register_search_start = register_index.saturating_add(1);
        let register = u16::try_from(register_index).map_err(|_| {
            CompileError::new(
                CompileErrorKind::TooManyRegisters,
                Span::dummy(),
                self.source.clone(),
            )
        })?;
        let high_water = u32::from(register) + 1;
        self.next_register = self.next_register.max(high_water);
        Ok(register)
    }

    pub fn free_register(&mut self, reg: u16) {
        let index = usize::from(reg);
        self.register_pool[index] = false;
        self.register_search_start = self.register_search_start.min(index);
    }

    pub fn alloc_consecutive_registers(&self, n: usize, span: Span) -> Result<u16> {
        let pool_len = self.register_pool.len();

        'outer: for start in 0..pool_len {
            if start + n > pool_len {
                break;
            }
            for offset in 0..n {
                if self.register_pool[start + offset] {
                    continue 'outer;
                }
            }
            return self.window_register(start, span);
        }

        Err(CompileError::new(
            CompileErrorKind::TooManyRegisters,
            span,
            self.source.clone(),
        )
        .into())
    }

    pub(crate) fn live_register_top(&self) -> usize {
        let scan_end = usize::try_from(self.next_register)
            .unwrap_or(usize::MAX)
            .min(self.register_pool.len());
        self.register_pool[..scan_end]
            .iter()
            .rposition(|used| *used)
            .map_or(0, |index| index + 1)
    }

    // the callee frame covers every register from the window base upward, so a window never sits below a live register
    pub fn alloc_call_window(&self, n: usize, span: Span) -> Result<u16> {
        let start = self.live_register_top();
        if start.saturating_add(n) > self.register_pool.len() {
            return Err(CompileError::new(
                CompileErrorKind::TooManyRegisters,
                span,
                self.source.clone(),
            )
            .into());
        }
        self.window_register(start, span)
    }

    fn window_register(&self, start: usize, span: Span) -> Result<u16> {
        u16::try_from(start).map_err(|_| {
            CompileError::new(
                CompileErrorKind::TooManyRegisters,
                span,
                self.source.clone(),
            )
            .into()
        })
    }

    pub fn finish_registers(&mut self) -> Result<()> {
        self.current.num_registers = self.next_register;
        if let Some((register, window_base, span)) = self.call_window_fault.take() {
            return Err(CompileError::new(
                CompileErrorKind::CallWindowOverlap {
                    register,
                    window_base,
                },
                span,
                self.source.clone(),
            )
            .into());
        }
        if let Some((_, line)) = self.current.own_wide_operand_fault() {
            let start = self.source.get_line_start_offset(line);
            let end = start + self.source.get_line(line).len();
            return Err(CompileError::new(
                CompileErrorKind::TooManyRegisters,
                Span::new(start, end, line, 1),
                self.source.clone(),
            )
            .into());
        }
        Ok(())
    }

    pub fn alloc_consecutive_from(&mut self, start: u16, count: usize) -> Result<u16> {
        let start_usize = usize::from(start);
        let count_usize = count;
        let end_usize = start_usize + count_usize;

        if end_usize > self.register_pool.len() {
            return Err(CompileError::new(
                CompileErrorKind::TooManyRegisters,
                Span::dummy(),
                self.source.clone(),
            )
            .into());
        }

        // Safety check: verify none of the registers are already in use
        for i in start_usize..end_usize {
            if self.register_pool[i] {
                return Err(CompileError::new(
                    CompileErrorKind::TooManyRegisters,
                    Span::dummy(),
                    self.source.clone(),
                )
                .into());
            }
        }

        for i in start_usize..end_usize {
            self.register_pool[i] = true;
        }

        let high_water = u32::try_from(end_usize).unwrap_or(u32::MAX);
        self.next_register = self.next_register.max(high_water);
        Ok(start)
    }
}
