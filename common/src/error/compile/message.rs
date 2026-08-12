use super::CompileErrorKind;

impl CompileErrorKind {
    pub fn message(&self) -> String {
        match self {
            Self::UnterminatedString => "unterminated string literal".to_string(),
            Self::InvalidCharacter(c) => format!("invalid character '{}'", c),
            Self::InvalidNumber(s) => format!("invalid number '{}'", s),
            Self::InvalidEscape(c) => format!("invalid escape sequence '\\{}'", c),
            Self::UnterminatedFmtExpr => {
                "unterminated expression in format string (missing '}')".to_string()
            }
            Self::UnmatchedCloseBrace => "unmatched '}' in string (use '}}' to escape)".to_string(),
            Self::UnexpectedToken { expected, found } => {
                format!("expected {}, found {}", expected, found)
            }
            Self::ExpectedExpression => "expected expression".to_string(),
            Self::ExpectedIdentifier => "expected identifier".to_string(),
            Self::InvalidAssignmentTarget => "invalid assignment target".to_string(),
            Self::NullIsNotInSurface => {
                "null is not part of Aelys; use Option for absence or Result for failure"
                    .to_string()
            }
            Self::ExpectedPattern => "expected a match pattern".to_string(),
            Self::InvalidPattern { reason } => format!("invalid match pattern: {reason}"),
            Self::UnknownVariant { variant, expected } => {
                format!("unknown variant '{variant}' for {expected}")
            }
            Self::MatchArmValueRequired => {
                "match arm must produce a value or diverge with return, break, or continue"
                    .to_string()
            }
            Self::RecursionDepthExceeded { max } => {
                format!("expression nesting too deep (max {} levels)", max)
            }
            Self::CommentNestingTooDeep { max } => {
                format!("block comment nesting too deep (max {} levels)", max)
            }
            Self::UndefinedVariable(name) => format!("undefined variable '{}'", name),
            Self::VariableAlreadyDefined(name) => {
                format!("variable '{}' already defined in this scope", name)
            }
            Self::AssignToImmutable(name) => {
                format!("cannot assign to immutable variable '{}'", name)
            }
            Self::TooManyConstants => "too many constants in function".to_string(),
            Self::TooManyRegisters => "too many local variables in function".to_string(),
            Self::TooManyArguments => "too many arguments in function call".to_string(),
            Self::TooManyUpvalues => "too many captured variables (max 255)".to_string(),
            Self::JumpOffsetTooLarge { distance } => {
                format!(
                    "jump offset {} exceeds the bytecode encoding limit",
                    distance
                )
            }
            Self::CompilationLimitExceeded(message) => {
                format!("compilation limit exceeded: {message}")
            }
            Self::BreakOutsideLoop => "'break' outside of loop".to_string(),
            Self::ContinueOutsideLoop => "'continue' outside of loop".to_string(),
            Self::ReturnOutsideFunction => "'return' outside of function".to_string(),
            Self::MissingReturnValue { expected } => {
                format!("function can fall through without returning {expected}")
            }
            Self::AssignToLoopVariable(name) => {
                format!("cannot assign to loop variable '{}'", name)
            }
            Self::IntegerOverflow { value, min, max } => format!(
                "integer literal '{}' exceeds 48-bit signed range ({} to {})",
                value, min, max
            ),
            Self::ModuleNotFound {
                module_path,
                searched_paths,
            } => {
                format!(
                    "module not found: '{}'\n   = note: searched in: {}",
                    module_path,
                    searched_paths.join(", ")
                )
            }
            Self::CircularDependency { chain } => {
                format!("circular dependency detected: {}", chain.join(" -> "))
            }
            Self::SymbolNotPublic { symbol, module } => format!(
                "'{}' is not public in module '{}'\n   = help: add 'pub' before the declaration in {}.aelys",
                symbol, module, module
            ),
            Self::StdlibNotAvailable { module } => format!(
                "standard library module '{}' is not yet implemented\n   = note: standard library will be available in a future version",
                module
            ),
            Self::SymbolNotFound { symbol, module } => {
                format!("symbol '{}' not found in module '{}'", symbol, module)
            }
            Self::InvalidNativeModule { module, reason } => {
                format!("invalid native module '{}': {}", module, reason)
            }
            Self::NativeChecksumMismatch {
                module,
                expected,
                actual,
            } => format!(
                "native module '{}' checksum mismatch\n   \
                 = expected: {}\n   \
                 = actual:   {}\n   \
                 = hint: the module file may have been modified or corrupted",
                module, expected, actual
            ),
            Self::NativeVersionMismatch {
                module,
                required,
                found,
            } => {
                let found_str = found.as_deref().unwrap_or("(none)");
                format!(
                    "native module '{}' version constraint not satisfied\n   \
                     = required: {}\n   \
                     = found:    {}",
                    module, required, found_str
                )
            }
            Self::TypeInferenceError(msg) => format!("type error: {}", msg),
            Self::SymbolConflict { symbol, modules } => {
                format!(
                    "symbol '{}' is exported by multiple modules: {}\n   = hint: use 'as' alias to disambiguate",
                    symbol,
                    modules.join(", ")
                )
            }
            Self::ModulePathSeparator { module, member } => format!(
                "module members are reached with '::'; write '{}::{}'",
                module, member
            ),
            Self::NonExhaustiveMatch { missing } => format!(
                "non-exhaustive match; missing {}\n   = help: add a missing arm or '_'",
                missing.join(", ")
            ),
            Self::IgnoredResult => {
                "unused Result value; handle it with match, return, ?, or a consuming method"
                    .to_string()
            }
            Self::IgnoredOption => {
                "unused Option value; handle it with match, return, or a consuming method"
                    .to_string()
            }
            Self::QuestionMarkOutsideResult => {
                "cannot use '?' here; the enclosing function must return Option or Result"
                    .to_string()
            }
            Self::QuestionMarkTypeMismatch { source, target } => format!(
                "cannot propagate {source} with '?' from a function returning {target}\n   = help: use map_err for an explicit error conversion"
            ),
            Self::UnresolvedSumType { constructor } => format!(
                "cannot infer the type carried by {constructor}\n   = help: add an Option<T> or Result<T, E> annotation"
            ),
            Self::UntypedSumValue { name } => format!(
                "cannot use untyped native value '{name}' as Option or Result\n   = help: wrap it in a typed Aelys function"
            ),
            Self::InvalidSumMethod { method, receiver } => {
                format!("method '{method}' is not available on {receiver}")
            }
            Self::DynamicSumMethod { method } => format!(
                "dynamic value cannot use sum method '{method}'; annotate it as Option<T> or Result<T, E>"
            ),
            Self::NamedTypeError { message, .. } => message.clone(),
        }
    }
}
