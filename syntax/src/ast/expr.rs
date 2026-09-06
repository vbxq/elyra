use crate::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceKind {
    Shared,
    Mutable,
}

/// to a value, which the type grammar cannot express.
#[derive(Debug, Clone)]
pub enum AssociatedBinding {
    Type(TypeAnnotation),
    Const(i64),
}

#[derive(Debug, Clone)]
pub struct TypeAnnotation {
    pub name: String,
    pub path: Vec<String>,
    pub type_params: Vec<TypeAnnotation>,
    pub fn_params: Option<Vec<TypeAnnotation>>,
    pub fn_ret: Option<Box<TypeAnnotation>>,
    pub array_length: Option<u64>,
    /// `[t; bounds::limit]` a symbolic fixed-array length resolved from an
    pub array_length_path: Option<Vec<String>>,
    pub associated_bindings: Vec<(String, AssociatedBinding, Span)>,
    pub reference: Option<ReferenceKind>,
    pub span: Span,
}

impl TypeAnnotation {
    pub fn new(name: String, span: Span) -> Self {
        Self {
            path: vec![name.clone()],
            name,
            type_params: Vec::new(),
            fn_params: None,
            fn_ret: None,
            array_length: None,
            array_length_path: None,
            associated_bindings: Vec::new(),
            reference: None,
            span,
        }
    }

    pub fn with_params(name: String, type_params: Vec<TypeAnnotation>, span: Span) -> Self {
        Self {
            path: vec![name.clone()],
            name,
            type_params,
            fn_params: None,
            fn_ret: None,
            array_length: None,
            array_length_path: None,
            associated_bindings: Vec::new(),
            reference: None,
            span,
        }
    }

    pub fn with_param(name: String, type_param: TypeAnnotation, span: Span) -> Self {
        Self::with_params(name, vec![type_param], span)
    }

    pub fn function_type(params: Vec<TypeAnnotation>, ret: TypeAnnotation, span: Span) -> Self {
        Self {
            name: "fn".to_string(),
            path: vec!["fn".to_string()],
            type_params: Vec::new(),
            fn_params: Some(params),
            fn_ret: Some(Box::new(ret)),
            array_length: None,
            array_length_path: None,
            associated_bindings: Vec::new(),
            reference: None,
            span,
        }
    }

    pub fn fixed_array(element: TypeAnnotation, length: u64, span: Span) -> Self {
        Self {
            name: "array".to_string(),
            path: vec!["array".to_string()],
            type_params: vec![element],
            fn_params: None,
            fn_ret: None,
            array_length: Some(length),
            array_length_path: None,
            associated_bindings: Vec::new(),
            reference: None,
            span,
        }
    }

    pub fn fixed_array_symbolic(element: TypeAnnotation, path: Vec<String>, span: Span) -> Self {
        Self {
            name: "array".to_string(),
            path: vec!["array".to_string()],
            type_params: vec![element],
            fn_params: None,
            fn_ret: None,
            array_length: None,
            array_length_path: Some(path),
            associated_bindings: Vec::new(),
            reference: None,
            span,
        }
    }

    pub fn is_function_type(&self) -> bool {
        self.fn_params.is_some()
    }

    pub fn with_reference(mut self, reference: ReferenceKind, span: Span) -> Self {
        self.reference = Some(reference);
        self.span = span;
        self
    }
}

#[derive(Debug, Clone)]
pub struct Parameter {
    pub name: String,
    pub mutable: bool,
    pub type_annotation: Option<TypeAnnotation>, // None = inferred
    pub reference: Option<ReferenceKind>,
    pub span: Span,
}

impl Parameter {
    pub fn new(
        name: String,
        mutable: bool,
        type_annotation: Option<TypeAnnotation>,
        span: Span,
    ) -> Self {
        Self {
            name,
            mutable,
            type_annotation,
            reference: None,
            span,
        }
    }

    pub fn untyped(name: String, span: Span) -> Self {
        Self {
            name,
            mutable: false,
            type_annotation: None,
            reference: None,
            span,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
    pub repeat: Option<Box<Expr>>,
}

impl Expr {
    pub fn new(kind: ExprKind, span: Span) -> Self {
        Self {
            kind,
            span,
            repeat: None,
        }
    }

    pub fn with_repeat(kind: ExprKind, span: Span, repeat: Expr) -> Self {
        Self {
            kind,
            span,
            repeat: Some(Box::new(repeat)),
        }
    }
}

#[derive(Debug, Clone)]
pub enum FmtStringPart {
    Literal(String),
    Expr(Box<Expr>),
    Placeholder,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Int(i64),
    Float(f64),
    String(String),
    Bool(bool),
    Null,
    Unit,
    FmtString(Vec<FmtStringPart>),

    Identifier(String),

    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Borrow {
        mutable: bool,
        operand: Box<Expr>,
    },

    And {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Or {
        left: Box<Expr>,
        right: Box<Expr>,
    },

    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    GenericApply {
        callee: Box<Expr>,
        type_args: Vec<TypeAnnotation>,
    },
    Assign {
        name: String,
        value: Box<Expr>,
    },
    MemberAssign {
        object: Box<Expr>,
        member: String,
        value: Box<Expr>,
    },
    Grouping(Box<Expr>), // for precedence

    If {
        condition: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Box<Expr>,
    },

    Try(Box<Expr>),

    Match {
        scrutinee: Box<Expr>,
        arms: Vec<MatchArm>,
    },

    Lambda {
        params: Vec<Parameter>,
        return_type: Option<TypeAnnotation>,
        body: Vec<crate::ast::Stmt>,
    },

    Member {
        object: Box<Expr>,
        member: String,
        separator: MemberSeparator,
    }, // module.symbol

    ArrayLiteral {
        element_type: Option<TypeAnnotation>, // [...] or [...]
        elements: Vec<Expr>,
    },
    ArraySized {
        element_type: Option<TypeAnnotation>, // Array<int>(10) or Array(10) or [; 10]
        size: Box<Expr>,
    },
    VecLiteral {
        element_type: Option<TypeAnnotation>,
        elements: Vec<Expr>,
    },
    Index {
        object: Box<Expr>,
        index: Box<Expr>,
    },
    IndexAssign {
        object: Box<Expr>,
        index: Box<Expr>,
        value: Box<Expr>,
    },
    Range {
        start: Option<Box<Expr>>,
        end: Option<Box<Expr>>,
        inclusive: bool, // .. vs ..=
    },
    Slice {
        object: Box<Expr>,
        range: Box<Expr>,
    },

    StructLiteral {
        name: String,
        type_args: Vec<TypeAnnotation>,
        fields: Vec<StructFieldInit>,
    },

    EnumLiteral {
        path: Vec<String>,
        fields: Vec<StructFieldInit>,
    },

    GenericEnumLiteral {
        path: Vec<String>,
        type_args: Vec<TypeAnnotation>,
        fields: Vec<StructFieldInit>,
    },

    Cast {
        expr: Box<Expr>,
        target: TypeAnnotation,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberSeparator {
    Dot,
    Path,
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub guard: Option<Expr>,
    pub body: MatchArmBody,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum MatchArmBody {
    Expr(Box<Expr>),
    Block(Vec<crate::ast::Stmt>),
}

#[derive(Debug, Clone)]
pub struct Pattern {
    pub kind: PatternKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum PatternKind {
    Wildcard,
    Binding(String),
    Int(i64),
    String(String),
    Bool(bool),
    Variant {
        path: Vec<String>,
        type_args: Vec<TypeAnnotation>,
        fields: Vec<Pattern>,
    },
    Struct {
        path: Vec<String>,
        type_args: Vec<TypeAnnotation>,
        fields: Vec<StructPatternField>,
        has_rest: bool,
    },
    Or(Vec<Pattern>),
}

#[derive(Debug, Clone)]
pub struct StructPatternField {
    pub name: String,
    pub pattern: Pattern,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct StructFieldInit {
    pub name: String,
    pub value: Box<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Shl,
    Shr,
    BitAnd,
    BitOr,
    BitXor,
}

impl BinaryOp {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::Mod => "%",
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
            Self::Shl => "<<",
            Self::Shr => ">>",
            Self::BitAnd => "&",
            Self::BitOr => "|",
            Self::BitXor => "^",
        }
    }
}

impl std::fmt::Display for BinaryOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,    // -
    Not,    // not
    BitNot, // ~
}

impl UnaryOp {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Neg => "-",
            Self::Not => "not",
            Self::BitNot => "~",
        }
    }
}
