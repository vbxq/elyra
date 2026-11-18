#![allow(clippy::missing_safety_doc)]

use super::{CallFrame, GcRef, MAX_FRAMES, ObjectKind, VM, Value};
use crate::{
    FRAME_STATUS_CALL_IN_INTERPRETER, FRAME_STATUS_NATIVE_ERROR, FRAME_STATUS_OK, FrameCode,
    FrameCollectionKind, FrameData, FrameEntry, FrameJitContext, JitFunctionKey,
};
use aelys_common::error::RuntimeError;
use smallvec::SmallVec;
use std::sync::Arc;

const EXIT_BUDGET: u64 = 64;
const EXITS_PER_PROGRESS: u64 = 4;

#[derive(Clone, Copy)]
pub(crate) struct FrameInstall {
    entry: usize,
    data: usize,
    registers: u32,
}

enum EntryState {
    Pending,
    Ready(FrameInstall),
    Refused,
}

#[derive(Default)]
struct ExitBudget {
    entries: u64,
    exits: u64,
    progress: u64,
}

impl ExitBudget {
    // a region stays compiled while each exit is paid for by enough loop iterations or completed entries in machine code
    fn record(&mut self, exited: bool, progress: u64) -> bool {
        self.entries = self.entries.saturating_add(1);
        self.progress = self.progress.saturating_add(progress);
        if !exited {
            self.progress = self.progress.saturating_add(1);
            return false;
        }
        self.exits = self.exits.saturating_add(1);
        self.exits > EXIT_BUDGET && self.exits.saturating_mul(EXITS_PER_PROGRESS) > self.progress
    }
}

struct OsrEntry {
    ip: u32,
    state: EntryState,
    optimized: bool,
    legacy_tried: bool,
    budget: ExitBudget,
}

pub(crate) struct FrameSlot {
    function: GcRef,
    key: Option<JitFunctionKey>,
    counters: usize,
    call: EntryState,
    call_optimized: bool,
    call_budget: ExitBudget,
    osr: SmallVec<[OsrEntry; 2]>,
    codes: SmallVec<[Arc<dyn FrameCode>; 2]>,
    blocks: SmallVec<[Box<[u64]>; 2]>,
    live: bool,
    legacy_refused: bool,
    legacy_safe: Option<bool>,
}

struct PendingFrame {
    function: GcRef,
    base: usize,
    dest: u16,
    ip: usize,
}

#[derive(Default)]
struct KeyCounters {
    calls: u64,
    backedges: u64,
}

pub(crate) struct FrameJitState {
    slots: Vec<FrameSlot>,
    free: Vec<u32>,
    counter_index: std::collections::HashMap<JitFunctionKey, usize>,
    counters: Vec<KeyCounters>,
    pending: Vec<PendingFrame>,
    context: FrameJitContext,
    pub(crate) global_epoch: u64,
    osr_resume: bool,
    pub(crate) disabled: bool,
    entry_floor: usize,
    root_ceiling: usize,
    native_depth: u32,
    native_error: Option<RuntimeError>,
    pub(crate) prepared_generation: u64,
}

impl Default for FrameJitState {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
            counter_index: std::collections::HashMap::new(),
            counters: vec![KeyCounters::default()],
            pending: Vec::new(),
            context: FrameJitContext {
                vm: std::ptr::null_mut(),
                status: 0,
                resume_ip: 0,
                global_epoch: 0,
                registers_end: 0,
                depth: 0,
                scratch0: 0,
                scratch1: 0,
                globals: 0,
                globals_len: 0,
                view_generation: 0,
                progress: 0,
            },
            global_epoch: 1,
            osr_resume: false,
            disabled: false,
            entry_floor: 0,
            root_ceiling: 0,
            native_depth: 0,
            native_error: None,
            prepared_generation: 1,
        }
    }
}

pub(crate) enum FrameOutcome {
    Returned(Value),
    Exited(u32),
    Interpret,
    Legacy,
}

enum EntryKind {
    Call,
    Osr,
}

impl VM {
    pub(crate) fn bump_jit_global_epoch(&mut self) {
        self.frame_jit.global_epoch = self.frame_jit.global_epoch.wrapping_add(1).max(1);
    }

    pub(crate) fn frame_jit_active(&self) -> bool {
        self.frame_jit_usable()
    }

    // true once neither the frame compiler nor the older pipeline will take a call to this function
    pub(crate) fn frame_jit_interprets(&self, function: GcRef) -> bool {
        let Some(ObjectKind::Function(object)) = self.heap.get(function).map(|object| &object.kind)
        else {
            return false;
        };
        let slot = object.jit_slot();
        slot != 0
            && self
                .frame_jit
                .slots
                .get(slot as usize - 1)
                .is_some_and(|entry| {
                    entry.live
                        && entry.function == function
                        && matches!(entry.call, EntryState::Refused)
                        && entry.legacy_refused
                })
    }

    fn frame_jit_usable(&self) -> bool {
        !self.frame_jit.disabled
            && self.jit_executor.is_some()
            && !self.execution_control.report
            && !self.execution_control_enabled()
    }

    fn frame_slot(&mut self, function: GcRef) -> Option<usize> {
        let ObjectKind::Function(object) = &self.heap.get(function)?.kind else {
            return None;
        };
        let slot = object.jit_slot();
        if slot != 0 {
            let index = usize::try_from(slot - 1).ok()?;
            if let Some(entry) = self.frame_jit.slots.get(index)
                && entry.live
                && entry.function == function
            {
                return Some(index);
            }
        }
        let key = match self.jit_function_keys.get(&function).cloned() {
            Some(key) => Some(key),
            None if self.jit_executor.is_some() => {
                let key = JitFunctionKey::root(crate::next_jit_module_id());
                self.jit_function_keys.insert(function, key.clone());
                Some(key)
            }
            None => None,
        };
        let counters = match &key {
            Some(key) => match self.frame_jit.counter_index.get(key) {
                Some(index) => *index,
                None => {
                    let index = self.frame_jit.counters.len();
                    self.frame_jit.counters.push(KeyCounters::default());
                    self.frame_jit.counter_index.insert(key.clone(), index);
                    index
                }
            },
            None => 0,
        };
        let entry = FrameSlot {
            function,
            call: if key.is_some() {
                EntryState::Pending
            } else {
                EntryState::Refused
            },
            key,
            counters,
            call_optimized: false,
            call_budget: ExitBudget::default(),
            osr: SmallVec::new(),
            codes: SmallVec::new(),
            blocks: SmallVec::new(),
            live: true,
            legacy_refused: false,
            legacy_safe: None,
        };
        let index = if let Some(free) = self.frame_jit.free.pop() {
            let index = usize::try_from(free).ok()?;
            self.frame_jit.slots[index] = entry;
            index
        } else {
            self.frame_jit.slots.push(entry);
            self.frame_jit.slots.len() - 1
        };
        let stored = u32::try_from(index).ok()?.checked_add(1)?;
        if let ObjectKind::Function(object) = &self.heap.get(function)?.kind {
            object.set_jit_slot(stored);
        }
        Some(index)
    }

    fn install_frame_code(
        &mut self,
        index: usize,
        osr_ip: Option<u32>,
        optimized: bool,
    ) -> Option<FrameInstall> {
        let executor = self.jit_executor.as_ref()?.clone();
        let slot = self.frame_jit.slots.get(index)?;
        let key = slot.key.clone()?;
        let function = slot.function;
        let object = self.heap.get(function)?;
        let ObjectKind::Function(bytecode_function) = &object.kind else {
            return None;
        };
        let code = if optimized {
            executor.compile_frame_optimized(&key, &bytecode_function.function, osr_ip)?
        } else {
            executor.compile_frame(&key, &bytecode_function.function, osr_ip)?
        };
        let info = code.info();
        if info.num_registers != bytecode_function.function.num_registers
            || info.arity != bytecode_function.function.arity
            || info.osr_ip != osr_ip
            || info.address == 0
        {
            return None;
        }
        let schema_words = FrameData::HEADER_WORDS
            .checked_add(info.sites.len().checked_mul(FrameData::SITE_WORDS)?)?;
        let schemas = bytecode_function.schemas();
        let words = schema_words.checked_add(schemas.structs.len())?;
        let mut block = vec![0u64; words].into_boxed_slice();
        block[0] = bytecode_function.constants.as_ptr() as u64;
        block[1] = function.index() as u64;
        block[2] = std::ptr::from_ref::<crate::FrameCodeInfo>(info) as u64;
        block[3] = schema_words as u64;
        block[FrameData::VIEW_GENERATION as usize / 8] = 0;
        block[FrameData::MAPPING as usize / 8] =
            bytecode_function.function.global_layout.id() as u64;
        let materialized = schemas.runtime_ids.len() == schemas.structs.len();
        for (index, schema) in schemas.structs.iter().enumerate() {
            let valid =
                materialized && u32::try_from(index).is_ok_and(|index| index == schema.schema_id);
            block[schema_words + index] = match schemas.runtime_ids.get(index) {
                Some(id) if valid => u64::from(id.0),
                _ => u64::MAX,
            };
        }
        let install = FrameInstall {
            entry: info.address,
            data: block.as_mut_ptr() as usize,
            registers: info.num_registers,
        };
        let slot = self.frame_jit.slots.get_mut(index)?;
        slot.codes.push(code);
        slot.blocks.push(block);
        Some(install)
    }

    pub(crate) fn frame_jit_call(
        &mut self,
        function: GcRef,
        argument_start: usize,
    ) -> FrameOutcome {
        if !self.frame_jit_usable() {
            return FrameOutcome::Legacy;
        }
        let Some(index) = self.frame_slot(function) else {
            return FrameOutcome::Legacy;
        };
        let install = match self.frame_jit.slots[index].call {
            EntryState::Ready(install) => install,
            EntryState::Refused if self.frame_jit.slots[index].legacy_refused => {
                return FrameOutcome::Interpret;
            }
            EntryState::Refused => return FrameOutcome::Legacy,
            EntryState::Pending => {
                let threshold = self
                    .jit_executor
                    .as_ref()
                    .map_or(u64::MAX, |executor| executor.frame_call_threshold());
                let counters = &mut self.frame_jit.counters[self.frame_jit.slots[index].counters];
                counters.calls = counters.calls.saturating_add(1);
                if counters.calls < threshold {
                    return FrameOutcome::Interpret;
                }
                match self.install_frame_code(index, None, false) {
                    Some(install) => {
                        self.frame_jit.slots[index].call = EntryState::Ready(install);
                        install
                    }
                    None => {
                        self.frame_jit.slots[index].call = EntryState::Refused;
                        let slot = &self.frame_jit.slots[index];
                        if let Some(key) = slot.key.clone() {
                            let calls = self.frame_jit.counters[slot.counters].calls;
                            self.jit_call_counts.insert(key, calls.saturating_sub(1));
                        }
                        return FrameOutcome::Legacy;
                    }
                }
            }
        };
        if self.frames.len() >= MAX_FRAMES {
            return FrameOutcome::Interpret;
        }
        let (outcome, progress) = self.enter_frame_code(install, argument_start, EntryKind::Call);
        let exited = matches!(outcome, FrameOutcome::Exited(_));
        if exited && let Some(executor) = self.jit_executor.as_ref() {
            executor.record_frame_exit();
        }
        let slot = &mut self.frame_jit.slots[index];
        if slot.call_budget.record(exited, progress) {
            slot.call = EntryState::Refused;
        }
        outcome
    }

    fn enter_frame_code(
        &mut self,
        install: FrameInstall,
        window_start: usize,
        kind: EntryKind,
    ) -> (FrameOutcome, u64) {
        let Some(window_end) = window_start.checked_add(install.registers as usize) else {
            return (FrameOutcome::Interpret, 0);
        };
        if window_end > self.registers.len() {
            return (FrameOutcome::Interpret, 0);
        }
        let depth = match kind {
            EntryKind::Call => MAX_FRAMES.saturating_sub(self.frames.len() + 1),
            EntryKind::Osr => MAX_FRAMES.saturating_sub(self.frames.len()),
        };
        self.register_current_view();
        let expected_globals = self.current_global_mapping_id;
        self.frame_jit.pending.clear();
        self.frame_jit.native_error = None;
        self.frame_jit.entry_floor = window_start;
        let registers = self.registers.as_mut_ptr();
        let registers_end = registers as usize + self.registers.len() * 8;
        let vm = std::ptr::from_mut::<VM>(self).cast::<std::ffi::c_void>();
        let epoch = self.frame_jit.global_epoch;
        let (globals, globals_len) = (
            self.globals_by_index.as_ptr() as u64,
            self.globals_by_index.len() as u64,
        );
        let view_generation = self.frame_jit.prepared_generation;
        let context = &mut self.frame_jit.context;
        context.vm = vm;
        context.status = FRAME_STATUS_OK;
        context.resume_ip = 0;
        context.global_epoch = epoch;
        context.registers_end = registers_end as u64;
        context.depth = depth as u64;
        context.globals = globals;
        context.globals_len = globals_len;
        context.view_generation = view_generation;
        context.progress = 0;
        let context = std::ptr::from_mut::<FrameJitContext>(context);
        // the window lies inside the register stack and nothing reallocates it while machine code runs
        let result = unsafe {
            let entry = std::mem::transmute::<usize, FrameEntry>(install.entry);
            entry(
                context,
                registers.add(window_start).cast::<u64>(),
                install.data as *mut u64,
            )
        };
        self.restore_interpreter_globals(expected_globals);
        let progress = self.frame_jit.context.progress;
        let outcome = match self.frame_jit.context.status {
            FRAME_STATUS_OK => FrameOutcome::Returned(Value::from_raw(result)),
            _ => FrameOutcome::Exited(
                u32::try_from(self.frame_jit.context.resume_ip).unwrap_or(u32::MAX),
            ),
        };
        (outcome, progress)
    }

    pub(crate) fn restore_interpreter_globals(&mut self, expected: usize) {
        if self.current_global_mapping_id == expected {
            return;
        }
        if let Some(frame) = self.frames.last() {
            let function = frame.function;
            self.prepare_globals_for_function(function);
        }
    }

    pub(crate) fn push_pending_jit_frames(&mut self) -> Result<(), RuntimeError> {
        if self.frame_jit.pending.is_empty() && self.frame_jit.native_error.is_none() {
            return Ok(());
        }
        self.sync_current_function_globals();
        let pending = std::mem::take(&mut self.frame_jit.pending);
        let mut innermost = None;
        for frame in pending.iter().rev() {
            let Some(object) = self.heap.get(frame.function) else {
                return Err(self.runtime_error(aelys_common::error::RuntimeErrorKind::UseAfterFree));
            };
            let ObjectKind::Function(function) = &object.kind else {
                return Err(self.runtime_error(
                    aelys_common::error::RuntimeErrorKind::InvalidBytecode(
                        "machine-code frame is not a function".to_string(),
                    ),
                ));
            };
            let mut call_frame = CallFrame::with_return_dest(
                frame.function,
                frame.base,
                frame.dest,
                function.function.bytecode.as_ptr(),
                function.function.bytecode.len(),
                function.constants.as_ptr(),
                function.constants.len(),
                function.num_registers(),
            );
            call_frame.global_mapping_id = function.function.global_layout.id();
            call_frame.ip = frame.ip;
            if self.frames.len() >= MAX_FRAMES {
                return Err(
                    self.runtime_error(aelys_common::error::RuntimeErrorKind::StackOverflow)
                );
            }
            self.frames.push(call_frame);
            innermost = Some(frame.function);
        }
        self.frame_jit.pending = pending;
        self.frame_jit.pending.clear();
        if let Some(function) = innermost {
            self.prepare_globals_for_function(function);
        }
        if let Some(mut error) = self.frame_jit.native_error.take() {
            error.stack_trace = self.build_stack_trace();
            return Err(error);
        }
        Ok(())
    }

    #[inline(never)]
    pub(crate) fn frame_jit_backedge(
        &mut self,
        function: GcRef,
        bytecode_ip: usize,
    ) -> Option<Result<Option<Value>, RuntimeError>> {
        if !self.frame_jit_usable() {
            return None;
        }
        let index = self.frame_slot(function)?;
        let ip = u32::try_from(bytecode_ip).ok()?;
        let threshold = crate::JIT_TIER1_BACKEDGE_THRESHOLD;
        self.frame_jit.slots[index].key.as_ref()?;
        let counters = &mut self.frame_jit.counters[self.frame_jit.slots[index].counters];
        counters.backedges = counters.backedges.saturating_add(1);
        let backedges = counters.backedges;
        let slot = &mut self.frame_jit.slots[index];
        let position = slot.osr.iter().position(|entry| entry.ip == ip);
        let install = match position.map(|position| &slot.osr[position].state) {
            Some(EntryState::Ready(install)) => *install,
            Some(EntryState::Refused) => {
                let entry = &mut slot.osr[position?];
                if entry.legacy_tried {
                    if let Some(frame) = self.frames.last_mut() {
                        frame.jit_quiet_ip = bytecode_ip;
                    }
                    return Some(Ok(None));
                }
                entry.legacy_tried = true;
                return None;
            }
            Some(EntryState::Pending) | None => {
                if backedges < threshold {
                    return Some(Ok(None));
                }
                let install = self.install_frame_code(index, Some(ip), false);
                let slot = &mut self.frame_jit.slots[index];
                let state = match install {
                    Some(install) => EntryState::Ready(install),
                    None => EntryState::Refused,
                };
                match position {
                    Some(position) => {
                        slot.osr[position].state = state;
                        slot.osr[position].legacy_tried = install.is_none();
                    }
                    None => slot.osr.push(OsrEntry {
                        ip,
                        state,
                        legacy_tried: install.is_none(),
                        optimized: false,
                        budget: ExitBudget::default(),
                    }),
                }
                if install.is_none()
                    && let Some(frame) = self.frames.last_mut()
                {
                    frame.jit_backedges_initialized = true;
                    frame.jit_backedge_base = threshold.saturating_sub(1);
                    frame.jit_backedges = 0;
                }
                install?
            }
        };
        let frame = self.frames.last()?;
        let base = frame.base;
        let (outcome, progress) = self.enter_frame_code(install, base, EntryKind::Osr);
        let slot = &mut self.frame_jit.slots[index];
        let mut promote = false;
        if let Some(entry) = slot.osr.iter_mut().find(|entry| entry.ip == ip) {
            if entry
                .budget
                .record(matches!(outcome, FrameOutcome::Exited(_)), progress)
            {
                entry.state = EntryState::Refused;
            } else if !entry.optimized
                && entry.budget.progress >= FrameData::TIER_UP as u64
                && matches!(entry.state, EntryState::Ready(_))
            {
                entry.optimized = true;
                promote = true;
            }
        }
        if promote && let Some(install) = self.install_frame_code(index, Some(ip), true) {
            let slot = &mut self.frame_jit.slots[index];
            if let Some(entry) = slot.osr.iter_mut().find(|entry| entry.ip == ip) {
                entry.state = EntryState::Ready(install);
            }
        }
        match outcome {
            FrameOutcome::Returned(value) => {
                if let Some(executor) = self.jit_executor.as_ref() {
                    executor.record_frame_osr_return();
                }
                Some(Ok(Some(value)))
            }
            FrameOutcome::Exited(resume) => {
                if let Some(executor) = self.jit_executor.as_ref() {
                    executor.record_frame_exit();
                }
                if let Some(frame) = self.frames.last_mut() {
                    frame.ip = resume as usize;
                }
                if let Err(error) = self.push_pending_jit_frames() {
                    return Some(Err(error));
                }
                self.frame_jit.osr_resume = true;
                Some(Ok(Some(Value::null())))
            }
            FrameOutcome::Interpret | FrameOutcome::Legacy => Some(Ok(None)),
        }
    }

    // the older pipeline runs every qualified global through call_value, which restarts the interpreter at register 0
    pub(crate) fn legacy_jit_is_safe(&mut self, function: GcRef) -> bool {
        let Some(index) = self.frame_slot(function) else {
            return false;
        };
        if let Some(safe) = self.frame_jit.slots[index].legacy_safe {
            return safe;
        }
        let names = match self.heap.get(function).map(|object| &object.kind) {
            Some(ObjectKind::Function(object)) => {
                let layout = &object.function.global_layout;
                let words = object.function.bytecode.as_slice();
                let mut names = Vec::new();
                let mut ip = 0usize;
                while ip < words.len() {
                    let word = words[ip];
                    let Some(opcode) = u8::try_from(word >> 24)
                        .ok()
                        .and_then(aelys_bytecode::OpCode::from_u8)
                    else {
                        break;
                    };
                    let index = match opcode {
                        aelys_bytecode::OpCode::CallGlobal => Some(((word >> 8) & 0xff) as usize),
                        aelys_bytecode::OpCode::GetGlobalIdx => Some((word & 0xffff) as usize),
                        aelys_bytecode::OpCode::GetGlobalIdxWide => {
                            words.get(ip + 1).map(|index| *index as usize)
                        }
                        _ => None,
                    };
                    if let Some(name) = index.and_then(|index| layout.names().get(index))
                        && name.contains("::")
                    {
                        names.push(name.clone());
                    }
                    ip += opcode.extension_words() + 1;
                }
                names
            }
            _ => return false,
        };
        let safe = names.iter().all(|name| {
            let value = self.global_by_name(name);
            value
                .as_ptr()
                .and_then(|pointer| self.heap.get(GcRef::new(pointer)))
                .is_some_and(|object| matches!(object.kind, ObjectKind::Native(_)))
        });
        self.frame_jit.slots[index].legacy_safe = Some(safe);
        safe
    }

    pub(crate) fn mark_frame_legacy_refused(&mut self, function: GcRef) {
        let Some(ObjectKind::Function(object)) = self.heap.get(function).map(|object| &object.kind)
        else {
            return;
        };
        let slot = object.jit_slot();
        if let Some(entry) = usize::try_from(slot)
            .ok()
            .and_then(|slot| slot.checked_sub(1))
            .and_then(|index| self.frame_jit.slots.get_mut(index))
            && entry.function == function
        {
            entry.legacy_refused = true;
        }
    }

    pub(crate) fn take_frame_osr_resume(&mut self) -> bool {
        std::mem::take(&mut self.frame_jit.osr_resume)
    }

    pub(crate) fn frame_jit_root_range(&self) -> Option<std::ops::Range<usize>> {
        let jit = &self.frame_jit;
        (jit.native_depth > 0 && jit.root_ceiling > jit.entry_floor)
            .then(|| jit.entry_floor..jit.root_ceiling.min(self.registers.len()))
    }

    pub(crate) fn sweep_frame_jit(&mut self) {
        if self.frame_jit.native_depth > 0 {
            return;
        }
        for index in 0..self.frame_jit.slots.len() {
            let slot = &self.frame_jit.slots[index];
            if !slot.live || self.heap.get(slot.function).is_some() {
                continue;
            }
            let slot = &mut self.frame_jit.slots[index];
            slot.live = false;
            slot.codes.clear();
            slot.blocks.clear();
            slot.osr.clear();
            if let Ok(free) = u32::try_from(index) {
                self.frame_jit.free.push(free);
            }
        }
    }

    pub(crate) fn reset_frame_jit(&mut self) {
        let disabled = self.frame_jit.disabled;
        let epoch = self.frame_jit.global_epoch;
        let prepared = self.frame_jit.prepared_generation;
        for slot in &self.frame_jit.slots {
            if let Some(object) = self.heap.get(slot.function)
                && let ObjectKind::Function(function) = &object.kind
            {
                function.set_jit_slot(0);
            }
        }
        self.frame_jit = FrameJitState::default();
        self.frame_jit.disabled = disabled;
        self.frame_jit.global_epoch = epoch.wrapping_add(1).max(1);
        self.frame_jit.prepared_generation = prepared + 1;
    }

    fn global_by_name(&mut self, name: &str) -> Value {
        if name.is_empty() {
            return Value::null();
        }
        self.sync_current_function_globals();
        self.globals.get(name).copied().unwrap_or_else(Value::null)
    }

    fn promote_loop(&mut self, function: GcRef, ip: u32) -> bool {
        let Some(index) = self.frame_slot(function) else {
            return false;
        };
        let slot = &mut self.frame_jit.slots[index];
        let position = match slot.osr.iter().position(|entry| entry.ip == ip) {
            Some(position) => position,
            None => {
                slot.osr.push(OsrEntry {
                    ip,
                    state: EntryState::Pending,
                    legacy_tried: false,
                    optimized: false,
                    budget: ExitBudget::default(),
                });
                slot.osr.len() - 1
            }
        };
        let entry = &mut slot.osr[position];
        if entry.optimized {
            return matches!(entry.state, EntryState::Ready(_));
        }
        if matches!(entry.state, EntryState::Refused) {
            return false;
        }
        entry.optimized = true;
        let Some(install) = self.install_frame_code(index, Some(ip), true) else {
            return false;
        };
        self.frame_jit.slots[index].osr[position].state = EntryState::Ready(install);
        true
    }

    fn promote_call(&mut self, function: GcRef) {
        let Some(index) = self.frame_slot(function) else {
            return;
        };
        let slot = &mut self.frame_jit.slots[index];
        let EntryState::Ready(old) = slot.call else {
            return;
        };
        if slot.call_optimized {
            return;
        }
        slot.call_optimized = true;
        let Some(install) = self.install_frame_code(index, None, true) else {
            return;
        };
        self.frame_jit.slots[index].call = EntryState::Ready(install);
        for slot in &mut self.frame_jit.slots {
            for block in &mut slot.blocks {
                let sites = (block[3] as usize).saturating_sub(FrameData::HEADER_WORDS)
                    / FrameData::SITE_WORDS;
                for site in 0..sites {
                    let entry = FrameData::HEADER_WORDS + site * FrameData::SITE_WORDS + 2;
                    if block.get(entry) == Some(&(old.entry as u64)) {
                        block[entry] = 0;
                    }
                }
            }
        }
    }

    fn refresh_frame_context(&mut self) {
        let context = &mut self.frame_jit.context;
        context.globals = self.globals_by_index.as_ptr() as u64;
        context.globals_len = self.globals_by_index.len() as u64;
        context.view_generation = self.frame_jit.prepared_generation;
        context.global_epoch = self.frame_jit.global_epoch;
    }

    fn frame_native_call(
        &mut self,
        native: &aelys_bytecode::NativeFunction,
        window: *const u64,
        argument_count: usize,
        frame_end: usize,
    ) -> Result<Value, RuntimeError> {
        let mut arguments = SmallVec::<[Value; 8]>::new();
        for index in 0..argument_count {
            arguments.push(Value::from_raw(unsafe { *window.add(index) }));
        }
        let previous_ceiling = self.frame_jit.root_ceiling;
        self.frame_jit.root_ceiling = previous_ceiling.max(frame_end);
        self.frame_jit.native_depth += 1;
        let registers = self.registers.as_ptr();
        let result = self.call_cached_native(native, &arguments);
        self.frame_jit.native_depth -= 1;
        self.register_current_view();
        self.frame_jit.root_ceiling = previous_ceiling;
        if self.registers.as_ptr() != registers {
            self.frame_jit.disabled = true;
        }
        result
    }
}

fn site_words(data: *mut u64, site: usize) -> *mut u64 {
    unsafe { data.add(FrameData::HEADER_WORDS + site * FrameData::SITE_WORDS) }
}

fn frame_info(data: *const u64) -> &'static crate::FrameCodeInfo {
    // word 2 points at the info of a code kept alive by the slot that owns this block
    unsafe { &*(*data.add(2) as *const crate::FrameCodeInfo) }
}

fn context_vm<'a>(context: *mut FrameJitContext) -> (&'a mut FrameJitContext, &'a mut VM) {
    unsafe {
        let context = &mut *context;
        let vm = &mut *context.vm.cast::<VM>();
        (context, vm)
    }
}

pub unsafe extern "C" fn aelys_frame_view(context: *mut FrameJitContext, data: *mut u64) -> u64 {
    let (_, vm) = context_vm(context);
    let function = GcRef::new(unsafe { *data.add(1) } as usize);
    let layout = match vm.heap.get(function).map(|object| &object.kind) {
        Some(ObjectKind::Function(function)) => Arc::clone(&function.function.global_layout),
        _ => return 0,
    };
    let view = vm.frame_prepared_view(&layout);
    let (values, length) = view.unwrap_or((std::ptr::null(), 0));
    unsafe {
        *data.add(FrameData::VIEW_VALUES as usize / 8) = values as u64;
        *data.add(FrameData::VIEW_LEN as usize / 8) = length as u64;
        *data.add(FrameData::VIEW_GENERATION as usize / 8) = vm.frame_jit.prepared_generation;
    }
    vm.refresh_frame_context();
    1
}

pub unsafe extern "C" fn aelys_frame_tier_up(context: *mut FrameJitContext, data: *mut u64) -> u64 {
    let (_, vm) = context_vm(context);
    unsafe {
        *data.add(FrameData::ENTRY_COUNT as usize / 8) = i64::MIN as u64;
    }
    let function = GcRef::new(unsafe { *data.add(1) } as usize);
    vm.promote_call(function);
    0
}

pub unsafe extern "C" fn aelys_frame_promote_loop(
    context: *mut FrameJitContext,
    data: *mut u64,
    ip: u64,
) -> u64 {
    let (_, vm) = context_vm(context);
    let function = GcRef::new(unsafe { *data.add(1) } as usize);
    let Ok(ip) = u32::try_from(ip) else {
        return 0;
    };
    u64::from(vm.promote_loop(function, ip))
}

pub unsafe extern "C" fn aelys_frame_set_global(
    context: *mut FrameJitContext,
    data: *mut u64,
    index: u64,
    value: u64,
) -> u64 {
    let (_, vm) = context_vm(context);
    let (Ok(index), Ok(mapping)) = (
        usize::try_from(index),
        usize::try_from(unsafe { *data.add(FrameData::MAPPING as usize / 8) }),
    ) else {
        return 0;
    };
    let stored = vm.frame_store_global(mapping, index, Value::from_raw(value));
    vm.refresh_frame_context();
    u64::from(stored)
}

pub unsafe extern "C" fn aelys_frame_call(
    context: *mut FrameJitContext,
    data: *mut u64,
    site: u64,
    callee: u64,
    window: *mut u64,
    argument_count: u64,
) -> u64 {
    let (context_ref, vm) = context_vm(context);
    let interpret = |context: &mut FrameJitContext| {
        context.status = FRAME_STATUS_CALL_IN_INTERPRETER;
        0
    };
    let info = frame_info(data);
    let Some(site) = usize::try_from(site)
        .ok()
        .filter(|site| *site < info.sites.len())
    else {
        return interpret(context_ref);
    };
    let callee_value = Value::from_raw(callee);
    let words = site_words(data, site);
    unsafe {
        *words.add(1) = callee_value.raw();
        *words.add(2) = 0;
    }
    let Some(pointer) = callee_value.as_ptr() else {
        return interpret(context_ref);
    };
    let function = GcRef::new(pointer);
    let native = match vm.heap.get(function).map(|object| &object.kind) {
        Some(ObjectKind::Function(target)) => {
            if u64::from(target.arity()) != argument_count {
                return interpret(context_ref);
            }
            None
        }
        Some(ObjectKind::Native(native)) => Some(native.clone()),
        _ => return interpret(context_ref),
    };
    if let Some(native) = native {
        let Ok(count) = usize::try_from(argument_count) else {
            return interpret(context_ref);
        };
        let base = (window as usize - vm.registers.as_ptr() as usize) / 8;
        let frame_end = base.saturating_add(info.num_registers as usize);
        let result = vm.frame_native_call(&native, window, count, frame_end);
        vm.refresh_frame_context();
        return match result {
            Ok(value) => value.raw(),
            Err(error) => {
                vm.frame_jit.native_error = Some(error);
                context_ref.status = FRAME_STATUS_NATIVE_ERROR;
                0
            }
        };
    }
    if vm.ensure_function_verified(function).is_err() {
        return interpret(context_ref);
    }
    let Some(index) = vm.frame_slot(function) else {
        return interpret(context_ref);
    };
    let install = match vm.frame_jit.slots[index].call {
        EntryState::Ready(install) => install,
        EntryState::Refused => return interpret(context_ref),
        EntryState::Pending => match vm.install_frame_code(index, None, false) {
            Some(install) => {
                vm.frame_jit.slots[index].call = EntryState::Ready(install);
                install
            }
            None => {
                vm.frame_jit.slots[index].call = EntryState::Refused;
                return interpret(context_ref);
            }
        },
    };
    let window_end = window as u64 + u64::from(install.registers) * 8;
    if window_end > context_ref.registers_end || context_ref.depth == 0 {
        return interpret(context_ref);
    }
    unsafe {
        *words.add(2) = install.entry as u64;
        *words.add(3) = install.data as u64;
        *words.add(4) = u64::from(install.registers);
    }
    context_ref.depth -= 1;
    let result = unsafe {
        let entry = std::mem::transmute::<usize, FrameEntry>(install.entry);
        entry(context, window, install.data as *mut u64)
    };
    let (context_ref, _) = context_vm(context);
    context_ref.depth += 1;
    result
}

pub unsafe extern "C" fn aelys_frame_push(
    context: *mut FrameJitContext,
    callee: u64,
    window: *mut u64,
    dest: u64,
) -> u64 {
    let (context, vm) = context_vm(context);
    let Some(pointer) = Value::from_raw(callee).as_ptr() else {
        return 0;
    };
    let base = (window as usize - vm.registers.as_ptr() as usize) / 8;
    vm.frame_jit.pending.push(PendingFrame {
        function: GcRef::new(pointer),
        base,
        dest: u16::try_from(dest).unwrap_or(u16::MAX),
        ip: usize::try_from(context.resume_ip).unwrap_or(usize::MAX),
    });
    0
}

pub unsafe extern "C" fn aelys_frame_collection(
    context: *mut FrameJitContext,
    value: u64,
    kind: u64,
) -> u64 {
    let (context, vm) = context_vm(context);
    let Some(kind) = FrameCollectionKind::from_u64(kind) else {
        return 0;
    };
    let Some(pointer) = Value::from_raw(value).as_ptr() else {
        return 0;
    };
    let Some(object) = vm.heap.get(GcRef::new(pointer)) else {
        return 0;
    };
    // writes go through these addresses from machine code while no rust borrow of the storage is live
    let (address, length) = match (kind, &object.kind) {
        (FrameCollectionKind::ArrayInts, ObjectKind::Array(array)) => match array.data.as_ints() {
            Some(values) => (values.as_ptr() as u64, values.len()),
            None => return 0,
        },
        (FrameCollectionKind::ArrayFloats, ObjectKind::Array(array)) => {
            match array.data.as_floats() {
                Some(values) => (values.as_ptr() as u64, values.len()),
                None => return 0,
            }
        }
        (FrameCollectionKind::ArrayBools, ObjectKind::Array(array)) => {
            match array.data.as_bools() {
                Some(values) => (values.as_ptr() as u64, values.len()),
                None => return 0,
            }
        }
        (FrameCollectionKind::ArrayObjects, ObjectKind::Array(array)) => {
            match array.data.as_objects() {
                Some(values) => (values.as_ptr() as u64, values.len()),
                None => return 0,
            }
        }
        (FrameCollectionKind::VecInts, ObjectKind::Vec(vector)) => match vector.data.as_ints() {
            Some(values) => (values.as_ptr() as u64, values.len()),
            None => return 0,
        },
        (FrameCollectionKind::VecFloats, ObjectKind::Vec(vector)) => {
            match vector.data.as_floats() {
                Some(values) => (values.as_ptr() as u64, values.len()),
                None => return 0,
            }
        }
        (FrameCollectionKind::VecBools, ObjectKind::Vec(vector)) => match vector.data.as_bools() {
            Some(values) => (values.as_ptr() as u64, values.len()),
            None => return 0,
        },
        (FrameCollectionKind::VecObjects, ObjectKind::Vec(vector)) => {
            match vector.data.as_objects() {
                Some(values) => (values.as_ptr() as u64, values.len()),
                None => return 0,
            }
        }
        (FrameCollectionKind::Length, ObjectKind::Array(array)) => (1, array.len()),
        (FrameCollectionKind::Length, ObjectKind::Vec(vector)) => (1, vector.len()),
        _ => return 0,
    };
    if address == 0 {
        return 0;
    }
    context.scratch0 = address;
    context.scratch1 = length as u64;
    1
}

pub unsafe extern "C" fn aelys_frame_struct(
    context: *mut FrameJitContext,
    data: *mut u64,
    value: u64,
    schema_index: u64,
) -> u64 {
    let (context, vm) = context_vm(context);
    let Ok(schema_index) = usize::try_from(schema_index) else {
        return 0;
    };
    let (offset, function) = unsafe { (*data.add(3) as usize, *data.add(1) as usize) };
    let Some(ObjectKind::Function(function)) =
        vm.heap.get(GcRef::new(function)).map(|object| &object.kind)
    else {
        return 0;
    };
    if schema_index >= function.function.struct_schemas.len() {
        return 0;
    }
    let runtime_id = unsafe { *data.add(offset + schema_index) };
    let Some(pointer) = Value::from_raw(value).as_ptr() else {
        return 0;
    };
    let Some(ObjectKind::Struct(structure)) =
        vm.heap.get(GcRef::new(pointer)).map(|object| &object.kind)
    else {
        return 0;
    };
    if u64::from(structure.schema_id.0) != runtime_id {
        return 0;
    }
    context.scratch0 = structure.slots.as_ptr() as u64;
    context.scratch1 = structure.slots.len() as u64;
    1
}

pub const FRAME_ENUM_INVALID: u64 = u64::MAX;

// returns the variant id plus one for an enum of this schema, 0 for any other value, FRAME_ENUM_INVALID where the interpreter reports a malformed object
pub unsafe extern "C" fn aelys_frame_enum(
    context: *mut FrameJitContext,
    data: *mut u64,
    value: u64,
    schema_index: u64,
) -> u64 {
    let (context, vm) = context_vm(context);
    let Some(pointer) = Value::from_raw(value).as_ptr() else {
        return 0;
    };
    let Some(ObjectKind::Enum(value)) = vm.heap.get(GcRef::new(pointer)).map(|object| &object.kind)
    else {
        return 0;
    };
    let function = unsafe { *data.add(1) } as usize;
    let Some(ObjectKind::Function(function)) =
        vm.heap.get(GcRef::new(function)).map(|object| &object.kind)
    else {
        return FRAME_ENUM_INVALID;
    };
    let schemas = function.schemas();
    let Some(schema) = schemas.enums.get(usize::from(value.enum_id)) else {
        return FRAME_ENUM_INVALID;
    };
    let Some(variant) = schema.variants.get(usize::from(value.variant_id)) else {
        return FRAME_ENUM_INVALID;
    };
    if usize::from(value.slot_count) != value.slots.len()
        || value.slots.len() != variant.fields.len()
        || schema.schema_id != value.enum_id
        || variant.variant_id != value.variant_id
    {
        return FRAME_ENUM_INVALID;
    }
    if u64::from(value.enum_id) != schema_index {
        return 0;
    }
    context.scratch0 = value.slots.as_ptr() as u64;
    context.scratch1 = value.slots.len() as u64;
    u64::from(value.variant_id) + 1
}

pub unsafe extern "C" fn aelys_frame_element(
    context: *mut FrameJitContext,
    collection: u64,
    index: u64,
    array: u64,
) -> u64 {
    let (context, vm) = context_vm(context);
    let Some(pointer) = Value::from_raw(collection).as_ptr() else {
        return 0;
    };
    let Ok(index) = usize::try_from(index) else {
        return 0;
    };
    let element = match vm.heap.get(GcRef::new(pointer)).map(|object| &object.kind) {
        Some(ObjectKind::Vec(vector)) if array == 0 && index < vector.len() => vector.get(index),
        Some(ObjectKind::Array(values)) if array != 0 && index < values.len() => values.get(index),
        _ => None,
    };
    let Some(element) = element else {
        return 0;
    };
    context.scratch0 = element.raw();
    1
}

impl VM {
    // the only write barrier machine code reaches: element stores and pushes into an existing Vec or Array slot
    fn frame_element_barrier(&mut self, owner: GcRef, index: usize, value: Value) {
        self.heap.write_barrier_element(owner, index, value);
    }

    fn frame_vec_push(&mut self, reference: GcRef, value: Value) -> bool {
        let Some(ObjectKind::Vec(vector)) = self.heap.get(reference).map(|object| &object.kind)
        else {
            return false;
        };
        if !vector.accepts(value) {
            return false;
        }
        let index = vector.len();
        // only a push that grows the storage reaches the interpreter's collection point
        if index >= vector.capacity() {
            let bytes = u64::try_from(self.heap.bytes_allocated()).unwrap_or(u64::MAX);
            if bytes > self.config.max_heap_bytes
                || self.heap.major_collection_active()
                || self.heap.should_collect()
            {
                return false;
            }
        }
        let Some(ObjectKind::Vec(vector)) = self.heap.get(reference).map(|object| &object.kind)
        else {
            return false;
        };
        let object_storage = matches!(vector.type_tag(), aelys_bytecode::object::TypeTag::Object);
        let Some(previous) = vector.try_size_bytes() else {
            return false;
        };
        let additional = if index < vector.capacity() {
            0
        } else {
            index.max(1)
        };
        let Some(needed) = index.checked_add(additional).and_then(|len| {
            aelys_bytecode::object::AelysVec::size_bytes_for(vector.type_tag(), len)
        }) else {
            return false;
        };
        let growth = u64::try_from(needed.saturating_sub(previous)).unwrap_or(u64::MAX);
        if self.ensure_heap_capacity(growth).is_err() {
            return false;
        }
        let current = match self.heap.get_mut(reference).map(|object| &mut object.kind) {
            Some(ObjectKind::Vec(vector)) => {
                if (additional > 0 && !vector.try_reserve_exact(additional)) || !vector.push(value)
                {
                    return false;
                }
                vector.try_size_bytes()
            }
            _ => return false,
        };
        let Some(current) = current else {
            return false;
        };
        self.heap.account_reallocation(previous, current);
        if object_storage {
            self.frame_element_barrier(reference, index, value);
        }
        true
    }
}

pub unsafe extern "C" fn aelys_frame_store_reference(
    context: *mut FrameJitContext,
    owner: u64,
    slot: *mut u64,
    value: u64,
    index: u64,
) -> u64 {
    let (_, vm) = context_vm(context);
    let Some(pointer) = Value::from_raw(owner).as_ptr() else {
        return 0;
    };
    let Ok(index) = usize::try_from(index) else {
        return 0;
    };
    // slot was derived from the owner's storage by a resolve helper during this invocation
    unsafe { *slot = value };
    vm.frame_element_barrier(GcRef::new(pointer), index, Value::from_raw(value));
    1
}

pub unsafe extern "C" fn aelys_frame_vec_push(
    context: *mut FrameJitContext,
    vector: u64,
    value: u64,
) -> u64 {
    let (_, vm) = context_vm(context);
    let Some(pointer) = Value::from_raw(vector).as_ptr() else {
        return 0;
    };
    u64::from(vm.frame_vec_push(GcRef::new(pointer), Value::from_raw(value)))
}

// (left << shift) / right exactly, 0 where the interpreter traps on a zero divisor or a result outside the 48-bit range
pub unsafe extern "C" fn aelys_frame_shl_div(
    context: *mut FrameJitContext,
    left: i64,
    right: i64,
    shift: i64,
) -> u64 {
    let (context, _) = context_vm(context);
    if right == 0 {
        return 0;
    }
    let exact = (i128::from(left) << (shift & 63)) / i128::from(right);
    if exact < i128::from(Value::INT_MIN) || exact > i128::from(Value::INT_MAX) {
        return 0;
    }
    context.scratch0 = exact as u64;
    1
}

pub extern "C" fn aelys_frame_fmod(left: f64, right: f64) -> f64 {
    left % right
}
