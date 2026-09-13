use super::{
    CompileErrorKind, RejectedBytecodeArtifact, RejectedBytecodeOrigin, RejectedBytecodeStage,
};

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
            Self::TypeNestingTooDeep { max } => {
                format!("type annotation nesting too deep (max {} levels)", max)
            }
            Self::BorrowingReceiverDeferred { form } => format!(
                "borrowing receiver '{form}' is deferred to Stage 3\n   = help: Stage 2 methods take the receiver by value; write 'self'"
            ),
            Self::TraitObjectDeferred { trait_name } => format!(
                "trait object 'dyn {trait_name}' is deferred to Stage 3\n   = help: take a generic type parameter bound by {trait_name} instead"
            ),
            Self::InvalidDefaultMethod => {
                "'default fn' is only allowed on a method of a generic trait impl\n   \
                 = help: drop 'default', or move the method to a generic 'impl<T> Trait for Type<T>'"
                    .to_string()
            }
            Self::UndefinedVariable(name) => format!(
                "undefined variable '{}'",
                crate::naming::unscoped_global_name(name)
            ),
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
            Self::SymbolConflict {
                symbol,
                modules,
                carried_by,
                repair,
            } => {
                let note = if carried_by.is_empty() {
                    String::new()
                } else {
                    format!(
                        "\n   = note: carried into this file by: {}",
                        carried_by.join(", ")
                    )
                };
                let hint = match repair {
                    super::kind::SymbolConflictRepair::Alias => {
                        "use 'as' alias to disambiguate".to_string()
                    }
                    super::kind::SymbolConflictRepair::NameOne => format!(
                        "import '{}' from one module only; a selective import has no 'as' form",
                        symbol
                    ),
                    super::kind::SymbolConflictRepair::RenameLocal => format!(
                        "rename this file's '{}'; a selective import has no 'as' form to alias the other",
                        symbol
                    ),
                };
                format!(
                    "symbol '{}' is exported by multiple modules: {}{}\n   = hint: {}",
                    symbol,
                    modules.join(", "),
                    note,
                    hint
                )
            }
            Self::TypeNotExportable {
                module,
                name,
                reason,
            } => format!(
                "'{}' cannot be exported from module '{}': {}",
                name, module, reason
            ),
            Self::ModulePathSeparator { module, member } => format!(
                "module members are reached with '::'; write '{}::{}'",
                module, member
            ),
            Self::PrivateFieldAccess(detail) => format!(
                "private field '{}.{}' cannot be {}: owner module '{}', current module '{}'; {}\n   = help: declare the field `pub` or access it from the owner module or a descendant",
                detail.structure,
                detail.field,
                detail.operation,
                detail.owner,
                detail.current,
                detail.reason
            ),
            Self::PrivateFieldConstruction(detail) => format!(
                "private field '{}.{}' cannot be used in {}: owner module '{}', current module '{}'; {}\n   = help: declare the field `pub`, use a public constructor, or omit it with a rest pattern",
                detail.structure,
                detail.field,
                detail.operation,
                detail.owner,
                detail.current,
                detail.reason
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
            Self::EmittedBytecodeRejected {
                output,
                reason,
                origin,
                stage,
                artifact,
            } => {
                let verdict = match (origin, stage) {
                    (RejectedBytecodeOrigin::Compiler, RejectedBytecodeStage::Verifier) => {
                        format!(
                            "the compiler emitted bytecode the Aelys verifier refuses: {reason}"
                        )
                    }
                    (RejectedBytecodeOrigin::Compiler, RejectedBytecodeStage::Reader) => {
                        format!("the compiler emitted bytecode it cannot read back: {reason}")
                    }
                    (RejectedBytecodeOrigin::Compiler, RejectedBytecodeStage::Loader) => {
                        format!("the compiler emitted bytecode that cannot be loaded: {reason}")
                    }
                    (RejectedBytecodeOrigin::Assembly, RejectedBytecodeStage::Verifier) => {
                        format!("the Aelys verifier refuses the assembled bytecode: {reason}")
                    }
                    (RejectedBytecodeOrigin::Assembly, RejectedBytecodeStage::Reader) => {
                        format!("the assembled bytecode cannot be read back: {reason}")
                    }
                    (RejectedBytecodeOrigin::Assembly, RejectedBytecodeStage::Loader) => {
                        format!("the assembled bytecode cannot be loaded: {reason}")
                    }
                };
                let artifact = match artifact {
                    RejectedBytecodeArtifact::Absent => {
                        format!("   = note: no '{output}' was written")
                    }
                    RejectedBytecodeArtifact::PreviousLeftInPlace => {
                        format!("   = note: the previous '{output}' was left untouched")
                    }
                };
                let blame = match origin {
                    RejectedBytecodeOrigin::Compiler => {
                        "   = note: this is a defect in the compiler, not in the program it compiled\n   \
                         = help: report the program that produced this message"
                    }
                    RejectedBytecodeOrigin::Assembly => {
                        "   = help: fix the assembly, or produce it with 'aelys-cli asm'"
                    }
                };
                format!("{verdict}\n{artifact}\n{blame}")
            }
            Self::BytecodeEncodingRefused {
                output,
                reason,
                longest_name,
                artifact,
            } => {
                let artifact = match artifact {
                    RejectedBytecodeArtifact::Absent => {
                        format!("   = note: no '{output}' was written")
                    }
                    RejectedBytecodeArtifact::PreviousLeftInPlace => {
                        format!("   = note: the previous '{output}' was left untouched")
                    }
                };
                let widest = match longest_name {
                    Some(name) => format!(
                        "\n   = note: the longest name the program declares is '{name}'\n   \
                         = help: shorten the names the bytecode has to carry"
                    ),
                    None => String::new(),
                };
                format!("the program cannot be encoded as bytecode: {reason}\n{artifact}{widest}")
            }
        }
    }
}
