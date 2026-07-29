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

    // contiguous block for call args
    pub fn alloc_consecutive_registers_for_call(&self, n: usize, span: Span) -> Result<u16> {
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
            return u16::try_from(start).map_err(|_| {
                CompileError::new(
                    CompileErrorKind::TooManyRegisters,
                    span,
                    self.source.clone(),
                )
                .into()
            });
        }

        Err(CompileError::new(
            CompileErrorKind::TooManyRegisters,
            span,
            self.source.clone(),
        )
        .into())
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
