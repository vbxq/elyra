use std::sync::Arc;

use aelys_syntax::Source;
use aelys_syntax::Span;
use aelys_syntax::{BinaryOp, Decorator, MemberSeparator, NeedsStmt, UnaryOp};

use crate::types::InferType;
use crate::types::{EnumVariantDef, TypeTable};

#[derive(Debug, Clone)]
pub struct TypedProgram {
    pub stmts: Vec<TypedStmt>,
    pub source: Arc<Source>,
    pub type_table: TypeTable,
}

#[derive(Debug, Clone)]
pub struct TypedStmt {
    pub kind: TypedStmtKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum TypedStmtKind {
    Expression(TypedExpr),

    Let {
        name: String,
        mutable: bool,
        initializer: TypedExpr,
        var_type: InferType,
        is_pub: bool,
    },

    Block(Vec<TypedStmt>),

    If {
        condition: TypedExpr,
        then_branch: Box<TypedStmt>,
        else_branch: Option<Box<TypedStmt>>,
    },

    While {
        condition: TypedExpr,
        body: Box<TypedStmt>,
    },

    For {
        iterator: String,
        start: TypedExpr,
        end: TypedExpr,
        inclusive: bool,
        step: Box<Option<TypedExpr>>,
        body: Box<TypedStmt>,
    },

    ForEach {
        iterator: String,
        iterable: TypedExpr,
        elem_type: InferType,
        read_only: bool,
        body: Box<TypedStmt>,
    },

    Return(Option<TypedExpr>),

    Break,
    Continue,

    Function(TypedFunction),

    ImplDecl {
        target: String,
        trait_name: Option<String>,
        type_params: Vec<String>,
        target_type: InferType,
        trait_args: Vec<InferType>,
        methods: Vec<TypedFunction>,
    },

    TraitDecl {
        name: String,
        type_params: Vec<String>,
    },

    Needs(NeedsStmt),

    StructDecl {
        name: String,
        type_params: Vec<String>,
        fields: Vec<(String, InferType)>,
    },

    EnumDecl {
        name: String,
        type_params: Vec<String>,
        variants: Vec<EnumVariantDef>,
    },
}

#[derive(Debug, Clone)]
pub struct TypedFunction {
    pub name: String,
    pub type_params: Vec<String>,
    pub params: Vec<TypedParam>,
    pub return_type: InferType,
    pub body: Vec<TypedStmt>,
    pub decorators: Vec<Decorator>,
    pub is_pub: bool,
    pub span: Span,
    pub captures: Vec<(String, InferType)>,
}

#[derive(Debug, Clone)]
pub struct TypedParam {
    pub name: String,
    pub mutable: bool,
    pub ty: InferType,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct TypedExpr {
    pub kind: TypedExprKind,
    pub ty: InferType,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum TypedFmtStringPart {
    Literal(String),
    Expr(Box<TypedExpr>),
    Placeholder,
}

#[derive(Debug, Clone)]
pub enum TypedExprKind {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    FmtString(Vec<TypedFmtStringPart>),
    Unit,
    Null,

    Identifier(String),

    Binary {
        left: Box<TypedExpr>,
        op: BinaryOp,
        right: Box<TypedExpr>,
    },

    Unary {
        op: UnaryOp,
        operand: Box<TypedExpr>,
    },

    And {
        left: Box<TypedExpr>,
        right: Box<TypedExpr>,
    },

    Or {
        left: Box<TypedExpr>,
        right: Box<TypedExpr>,
    },

    Call {
        callee: Box<TypedExpr>,
        args: Vec<TypedExpr>,
    },

    Assign {
        name: String,
        value: Box<TypedExpr>,
    },

    Grouping(Box<TypedExpr>),

    If {
        condition: Box<TypedExpr>,
        then_branch: Box<TypedExpr>,
        else_branch: Box<TypedExpr>,
    },

    Try {
        operand: Box<TypedExpr>,
        conversion: Option<String>,
    },

    Match {
        scrutinee: Box<TypedExpr>,
        arms: Vec<TypedMatchArm>,
    },

    Lambda(Box<TypedExpr>),

    LambdaInner {
        params: Vec<TypedParam>,
        return_type: InferType,
        body: Vec<TypedStmt>, // Changed to support multi-statement bodies
        captures: Vec<(String, InferType)>, // NEW: captured variables
    },

    Member {
        object: Box<TypedExpr>,
        member: String,
        separator: MemberSeparator,
    },

    StructField {
        object: Box<TypedExpr>,
        member: String,
        offset: u16,
        schema_index: u16,
    },

    StructMethod {
        object: Box<TypedExpr>,
        symbol: String,
        method: String,
        separator: MemberSeparator,
    },

    MemberAssign {
        object: Box<TypedExpr>,
        member: String,
        offset: u16,
        schema_index: u16,
        value: Box<TypedExpr>,
    },

    ArrayLiteral {
        element_type: Option<crate::types::ResolvedType>,
        elements: Vec<TypedExpr>,
        repeat: Option<Box<TypedExpr>>,
    },

    ArraySized {
        element_type: Option<crate::types::ResolvedType>,
        size: Box<TypedExpr>,
    },

    VecLiteral {
        element_type: Option<crate::types::ResolvedType>,
        elements: Vec<TypedExpr>,
        repeat: Option<Box<TypedExpr>>,
    },

    Index {
        object: Box<TypedExpr>,
        index: Box<TypedExpr>,
    },

    IndexAssign {
        object: Box<TypedExpr>,
        index: Box<TypedExpr>,
        value: Box<TypedExpr>,
    },

    Range {
        start: Option<Box<TypedExpr>>,
        end: Option<Box<TypedExpr>>,
        inclusive: bool,
    },

    Slice {
        object: Box<TypedExpr>,
        range: Box<TypedExpr>,
    },

    StructLiteral {
        name: String,
        schema_index: u16,
        fields: Vec<(String, Box<TypedExpr>)>,
        field_offsets: Vec<u16>,
    },

    EnumConstruct {
        enum_name: String,
        variant: String,
        schema_index: u16,
        variant_index: u16,
        fields: Vec<(Option<String>, Box<TypedExpr>)>,
    },

    Cast {
        expr: Box<TypedExpr>,
        target: InferType,
    },

    /// the monomorphizer replaces this node with the integer literal. it must
    AssociatedConst {
        param: String,
        trait_name: String,
        item: String,
    },
}

#[derive(Debug, Clone)]
pub struct TypedMatchArm {
    pub pattern: TypedPattern,
    pub guard: Option<TypedExpr>,
    pub body: TypedMatchArmBody,
    pub explicit_dynamic: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum TypedMatchArmBody {
    Expr(TypedExpr),
    Block(Vec<TypedStmt>),
}

#[derive(Debug, Clone)]
pub struct TypedPattern {
    pub kind: TypedPatternKind,
    pub ty: InferType,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum TypedPatternKind {
    Wildcard,
    Binding(String),
    Int(i64),
    String(String),
    Bool(bool),
    Variant {
        path: Vec<String>,
        enum_schema_index: Option<u16>,
        enum_variant_index: Option<u16>,
        fields: Vec<TypedPattern>,
        field_offsets: Vec<u16>,
    },
    Struct {
        name: String,
        schema_index: u16,
        fields: Vec<(String, TypedPattern, u16)>,
        has_rest: bool,
    },
    Or(Vec<TypedPattern>),
}

impl TypedExpr {
    pub fn new(kind: TypedExprKind, ty: InferType, span: Span) -> Self {
        Self { kind, ty, span }
    }

    pub fn has_concrete_type(&self) -> bool {
        !matches!(self.ty, InferType::Var(_) | InferType::Dynamic)
    }
}

impl TypedStmt {
    pub fn new(kind: TypedStmtKind, span: Span) -> Self {
        Self { kind, span }
    }
}
