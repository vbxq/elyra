use super::stack::StackFrame;
use aelys_syntax::Source;
use std::fmt;
use std::sync::Arc;

#[derive(Debug)]
pub struct RuntimeError {
    pub kind: RuntimeErrorKind,
    pub stack_trace: Vec<StackFrame>,
    pub source: Arc<Source>,
    // private so only `from_verifier` can set it: callers downstream read this
    from_verifier: bool,
}

#[derive(Debug)]
pub enum RuntimeErrorKind {
    TypeError {
        operation: &'static str,
        expected: &'static str,
        got: String,
    },
    DivisionByZero,
    UndefinedVariable(String),
    UninitializedGlobal(String),
    NotCallable(String),
    ArityMismatch {
        expected: u16,
        got: u16,
    },
    ArgumentLimitExceeded {
        count: usize,
        max: u16,
    },
    StackOverflow,
    InvalidAllocationSize {
        size: i64,
    },
    OutOfMemory {
        requested: u64,
        max: u64,
    },
    InvalidMemoryHandle,
    DoubleFree,
    UseAfterFree,
    MemoryOutOfBounds {
        offset: usize,
        size: usize,
    },
    NegativeMemoryIndex {
        value: i64,
    },
    InvalidOpcode {
        opcode: u8,
    },
    InvalidRegister {
        reg: usize,
        max: usize,
    },
    InvalidBytecode(String),
    InvalidEnumField {
        expected_schema: u16,
        actual_schema: u16,
        expected_variant: u16,
        actual_variant: u16,
        field_offset: u16,
    },
    NativeError {
        code: i32,
    },
    NativeReturnedNull,
    NativePanic {
        name: String,
    },
    SumUnwrapFailed {
        family: &'static str,
        message: Option<String>,
    },
    IntegerOverflow,
    NonFiniteCast {
        value: &'static str,
        target: &'static str,
    },
    RuntimePanic {
        message: String,
    },
    Panic {
        message: String,
    },
    InstructionBudgetExceeded {
        limit: u64,
    },
    DeadlineExceeded,
    Interrupted,
    Exit(i32),
    IndexOutOfBounds {
        index: i64,
        length: i64,
    },
    IndexOutOfBoundsU64 {
        index: u64,
        length: i64,
    },
    MapKeyNotFound {
        key: String,
    },
    InvalidCharScalar {
        value: i64,
    },
    InvalidCharScalarU64 {
        value: u64,
    },
    VecChangedDuringSort,
    ByteRangeOutOfBounds {
        operation: &'static str,
        offset: i64,
        width: usize,
        length: usize,
    },
    ByteValueOutOfRange {
        operation: &'static str,
        value: String,
        target: &'static str,
    },
    FfiLibraryNotFound {
        library: String,
        function: String,
        tried: Box<[String]>,
        reason: Box<str>,
    },
    FfiSymbolNotFound {
        library: String,
        path: String,
        symbol: String,
    },
    FfiCall {
        function: String,
        message: String,
    },
    PointerAccess {
        operation: String,
        message: String,
    },
}

pub const PANIC_EXIT_CODE: i32 = 101;

impl RuntimeError {
    pub fn new(kind: RuntimeErrorKind, stack_trace: Vec<StackFrame>, source: Arc<Source>) -> Self {
        Self {
            kind,
            stack_trace,
            source,
            from_verifier: false,
        }
    }

    pub fn from_verifier(
        kind: RuntimeErrorKind,
        stack_trace: Vec<StackFrame>,
        source: Arc<Source>,
    ) -> Self {
        Self {
            kind,
            stack_trace,
            source,
            from_verifier: true,
        }
    }

    pub fn is_verifier_verdict(&self) -> bool {
        self.from_verifier
    }
}

impl RuntimeErrorKind {
    pub fn message(&self) -> String {
        match self {
            Self::TypeError {
                operation,
                expected,
                got,
            } => {
                format!(
                    "type error in '{}': expected {}, got {}",
                    operation, expected, got
                )
            }
            Self::DivisionByZero => "division by zero".to_string(),
            Self::UndefinedVariable(name) => crate::naming::undefined_variable_message(name),
            Self::UninitializedGlobal(name) => format!(
                "global `{}` read before its initialiser ran",
                crate::naming::unscoped_global_name(name)
            ),
            Self::NotCallable(ty) => format!("'{}' is not callable", ty),
            Self::ArityMismatch { expected, got } => {
                format!("expected {} arguments, got {}", expected, got)
            }
            Self::ArgumentLimitExceeded { count, max } => {
                format!("argument count {count} exceeds maximum {max}")
            }
            Self::StackOverflow => "stack overflow".to_string(),
            Self::InvalidAllocationSize { size } => {
                format!("invalid allocation size: {} (must be > 0)", size)
            }
            Self::OutOfMemory { requested, max } => {
                format!(
                    "out of memory: requested {} bytes (max {} bytes)",
                    requested, max
                )
            }
            Self::InvalidMemoryHandle => {
                "invalid or stale memory handle (possibly from another isolate)".to_string()
            }
            Self::DoubleFree => "double free: pointer was already freed".to_string(),
            Self::UseAfterFree => "use after free: pointer was already freed".to_string(),
            Self::MemoryOutOfBounds { offset, size } => format!(
                "memory access out of bounds: offset {} exceeds size {}",
                offset, size
            ),
            Self::NegativeMemoryIndex { value } => {
                format!("negative memory index: {}", value)
            }
            Self::InvalidOpcode { opcode } => format!("invalid opcode: {}", opcode),
            Self::InvalidRegister { reg, max } => {
                format!("invalid register index: {} (max: {})", reg, max)
            }
            Self::InvalidBytecode(message) => format!("invalid bytecode: {}", message),
            Self::InvalidEnumField {
                expected_schema,
                actual_schema,
                expected_variant,
                actual_variant,
                field_offset,
            } => format!(
                "invalid enum field: expected schema {} variant {} at offset {}, got schema {} variant {}",
                expected_schema, expected_variant, field_offset, actual_schema, actual_variant
            ),
            Self::NativeError { code } => match code {
                1 => "native error: code 1 (invalid argument)".to_string(),
                2 => "native error: code 2 (integer outside the int range)".to_string(),
                5 => "native error: code 5 (byte buffer borrowed mutably while another borrow of it is live in the same native call)"
                    .to_string(),
                _ => format!("native error: code {}", code),
            },
            Self::NativeReturnedNull => {
                "native function returned the forbidden null sentinel".to_string()
            }
            Self::NativePanic { name } if name.is_empty() => "native function panicked".to_string(),
            Self::NativePanic { name } => format!("native function '{name}' panicked"),
            Self::SumUnwrapFailed { family, message } => message.clone().unwrap_or_else(|| {
                format!("{family}::unwrap failed: value was not the success variant")
            }),
            Self::IntegerOverflow => {
                "integer overflow outside the int range (i64)".to_string()
            }
            Self::NonFiniteCast { value, target } => format!("cannot convert {value} to {target}"),
            Self::RuntimePanic { message } => format!("runtime panic recovered: {message}"),
            Self::Panic { message } => format!("panic: {message}"),
            Self::InstructionBudgetExceeded { limit } => {
                format!("instruction budget exhausted after {} instructions", limit)
            }
            Self::DeadlineExceeded => "execution deadline exceeded".to_string(),
            Self::Interrupted => "execution interrupted".to_string(),
            Self::Exit(code) => format!("execution exited with status {}", code),
            Self::IndexOutOfBounds { index, length } => {
                format!(
                    "index out of bounds: index {} is out of bounds for length {}",
                    index, length
                )
            }
            Self::IndexOutOfBoundsU64 { index, length } => {
                format!(
                    "index out of bounds: index {} is out of bounds for length {}",
                    index, length
                )
            }
            Self::MapKeyNotFound { key } => format!("map key not found: {key}"),
            Self::InvalidCharScalar { value } => {
                format!("invalid char: {value} is not a Unicode scalar value")
            }
            Self::InvalidCharScalarU64 { value } => {
                format!("invalid char: {value} is not a Unicode scalar value")
            }
            Self::VecChangedDuringSort => {
                "vector changed length while sort_by was running its comparator".to_string()
            }
            Self::ByteRangeOutOfBounds {
                operation,
                offset,
                width,
                length,
            } => format!(
                "{operation}: bytes {offset}..{} are out of bounds for length {length}",
                offset.saturating_add_unsigned(*width as u64)
            ),
            Self::ByteValueOutOfRange {
                operation,
                value,
                target,
            } => format!("{operation}: value {value} does not fit in {target}"),
            Self::FfiLibraryNotFound {
                library,
                function,
                tried,
                reason,
            } => format!(
                "cannot load C library '{library}' for C function '{function}': tried {}; last error: {reason}",
                tried.join(", ")
            ),
            Self::FfiSymbolNotFound {
                library,
                path,
                symbol,
            } => format!("C library '{library}' ({path}) has no symbol '{symbol}'"),
            Self::FfiCall { function, message } => format!("C function '{function}': {message}"),
            Self::PointerAccess { operation, message } => format!("{operation}: {message}"),
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if matches!(self.kind, RuntimeErrorKind::Panic { .. }) {
            writeln!(f, "{}", self.kind.message())?;
        } else {
            writeln!(f, "error: {}", self.kind.message())?;
        }

        if let Some(frame) = self.stack_trace.first() {
            let source = frame.source.as_ref().unwrap_or(&self.source);
            writeln!(f, "  --> {}:{}:{}", source.name, frame.line, frame.column)?;

            let line_content = source.get_line(frame.line);
            let line_num_width = frame.line.to_string().len().max(2);

            writeln!(f, "{:width$} |", "", width = line_num_width)?;
            writeln!(
                f,
                "{:>width$} | {}",
                frame.line,
                line_content,
                width = line_num_width
            )?;
        }

        if !self.stack_trace.is_empty() {
            const MAX_FRAMES: usize = 50;
            writeln!(f)?;
            writeln!(f, "stack trace (most recent call first):")?;
            let display_count = self.stack_trace.len().min(MAX_FRAMES);
            for frame in &self.stack_trace[..display_count] {
                let name = frame.function_name.as_deref().unwrap_or("<script>");
                let source = frame.source.as_ref().unwrap_or(&self.source);
                writeln!(f, "  {} ({}:{})", name, source.name, frame.line)?;
            }
            if self.stack_trace.len() > MAX_FRAMES {
                writeln!(
                    f,
                    "  ... {} more frames",
                    self.stack_trace.len() - MAX_FRAMES
                )?;
            }
        }

        Ok(())
    }
}
