#[derive(Debug)]
pub enum CompileErrorKind {
    // Lexer errors
    UnterminatedString,
    InvalidCharacter(char),
    InvalidNumber(String),
    InvalidEscape(char),
    UnterminatedFmtExpr,
    UnmatchedCloseBrace,

    // Parser errors
    UnexpectedToken {
        expected: String,
        found: String,
    },
    ExpectedExpression,
    ExpectedIdentifier,
    InvalidAssignmentTarget,
    NullIsNotInSurface,
    ExpectedPattern,
    InvalidPattern {
        reason: String,
    },
    UnknownVariant {
        variant: String,
        expected: String,
    },
    MatchArmValueRequired,
    RecursionDepthExceeded {
        max: usize,
    },
    CommentNestingTooDeep {
        max: usize,
    },

    // Compiler errors
    UndefinedVariable(String),
    VariableAlreadyDefined(String),
    AssignToImmutable(String),
    TooManyConstants,
    TooManyRegisters,
    TooManyArguments,
    TooManyUpvalues,
    JumpOffsetTooLarge {
        distance: usize,
    },
    CompilationLimitExceeded(String),
    BreakOutsideLoop,
    ContinueOutsideLoop,
    ReturnOutsideFunction,
    MissingReturnValue {
        expected: String,
    },
    AssignToLoopVariable(String),

    // 48-bit signed range for NaN-boxed ints
    IntegerOverflow {
        value: String,
        min: i64,
        max: i64,
    },

    // Module errors
    ModuleNotFound {
        module_path: String,
        searched_paths: Vec<String>,
    },
    CircularDependency {
        chain: Vec<String>,
    },
    SymbolNotPublic {
        symbol: String,
        module: String,
    },
    StdlibNotAvailable {
        module: String,
    },
    SymbolNotFound {
        symbol: String,
        module: String,
    },
    InvalidNativeModule {
        module: String,
        reason: String,
    },
    NativeChecksumMismatch {
        module: String,
        expected: String,
        actual: String,
    },
    NativeVersionMismatch {
        module: String,
        required: String,
        found: Option<String>,
    },
    SymbolConflict {
        symbol: String,
        modules: Vec<String>,
    },
    ModulePathSeparator {
        module: String,
        member: String,
    },

    NonExhaustiveMatch {
        missing: Vec<String>,
    },
    IgnoredResult,
    IgnoredOption,
    QuestionMarkOutsideResult,
    QuestionMarkTypeMismatch {
        source: String,
        target: String,
    },
    UnresolvedSumType {
        constructor: String,
    },
    UntypedSumValue {
        name: String,
    },
    InvalidSumMethod {
        method: String,
        receiver: String,
    },
    DynamicSumMethod {
        method: String,
    },

    TypeInferenceError(String),
    NamedTypeError {
        code: u16,
        message: String,
    },
}
