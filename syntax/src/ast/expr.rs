use crate::Span;

#[derive(Debug, Clone)]
pub struct TypeAnnotation {
    pub name: String,
    pub type_params: Vec<TypeAnnotation>,
    pub fn_params: Option<Vec<TypeAnnotation>>,
    pub fn_ret: Option<Box<TypeAnnotation>>,
    pub span: Span,
}

impl TypeAnnotation {
    pub fn new(name: String, span: Span) -> Self {
        Self {
            name,
            type_params: Vec::new(),
            fn_params: None,
            fn_ret: None,
            span,
        }
    }

    pub fn with_params(name: String, type_params: Vec<TypeAnnotation>, span: Span) -> Self {
        Self {
            name,
            type_params,
            fn_params: None,
            fn_ret: None,
            span,
        }
    }

    pub fn with_param(name: String, type_param: TypeAnnotation, span: Span) -> Self {
        Self::with_params(name, vec![type_param], span)
    }

    pub fn function_type(params: Vec<TypeAnnotation>, ret: TypeAnnotation, span: Span) -> Self {
        Self {
            name: "fn".to_string(),
            type_params: Vec::new(),
            fn_params: Some(params),
            fn_ret: Some(Box::new(ret)),
            span,
        }
    }

    pub fn is_function_type(&self) -> bool {
        self.fn_params.is_some()
    }
}

#[derive(Debug, Clone)]
pub struct Parameter {
    pub name: String,
    pub mutable: bool,
    pub type_annotation: Option<TypeAnnotation>, // None = inferred
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
            span,
        }
    }

    pub fn untyped(name: String, span: Span) -> Self {
        Self {
            name,
            mutable: false,
            type_annotation: None,
            span,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

impl Expr {
    pub fn new(kind: ExprKind, span: Span) -> Self {
        Self { kind, span }
    }
}

/// Part of a format string in the AST (after parsing expressions)
#[derive(Debug, Clone)]
pub enum FmtStringPart {
    Literal(String),
    Expr(Box<Expr>),
    Placeholder,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    // literals
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

    // short-circuit (separate from Binary because different codegen)
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
    Assign {
        name: String,
        value: Box<Expr>,
    },
    Grouping(Box<Expr>), // for precedence

    // ternary: cond ? then : else
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

    // Arrays and Vecs
    ArrayLiteral {
        element_type: Option<TypeAnnotation>, // Array<Int>[...] or Array[...]
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
    Expr(Expr),
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
        fields: Vec<Pattern>,
    },
    Or(Vec<Pattern>),
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
