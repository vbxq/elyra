#[derive(Debug)]
pub enum CompileErrorKind {
    UnterminatedString,
    InvalidCharacter(char),
    InvalidNumber(String),
    InvalidEscape(char),
    UnterminatedFmtExpr,
    UnmatchedCloseBrace,

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
    TypeNestingTooDeep {
        max: usize,
    },

    BorrowingReceiverDeferred {
        form: String,
    },
    TraitObjectDeferred {
        trait_name: String,
    },
    NegativeImplDeferred,
    SpecializationDeferred,

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

    IntegerOverflow {
        value: String,
        min: i64,
        max: i64,
    },

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
        // the modules this file named that bring a declaration it never named
        carried_by: Vec<String>,
        repair: SymbolConflictRepair,
    },
    TypeNotExportable {
        module: String,
        name: String,
        reason: String,
    },
    ModulePathSeparator {
        module: String,
        member: String,
    },
    PrivateFieldAccess(Box<PrivateFieldDetail>),
    PrivateFieldConstruction(Box<PrivateFieldDetail>),

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

#[derive(Debug)]
pub struct PrivateFieldDetail {
    pub structure: String,
    pub field: String,
    pub owner: String,
    pub current: String,
    pub operation: String,
    pub reason: String,
}

/// a selective `needs` has no `as` form, so only the whole-module clash is aliasable
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolConflictRepair {
    Alias,
    NameOne,
    RenameLocal,
}
