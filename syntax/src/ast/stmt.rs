use super::expr::{Expr, Parameter, TypeAnnotation};
use crate::{ModuleId, Span};

#[derive(Debug, Clone)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
    pub read_only: bool,
    pub definition_module: Option<ModuleId>,
}

impl Stmt {
    pub fn new(kind: StmtKind, span: Span) -> Self {
        Self {
            kind,
            span,
            read_only: false,
            definition_module: None,
        }
    }

    pub fn read_only(kind: StmtKind, span: Span) -> Self {
        Self {
            kind,
            span,
            read_only: true,
            definition_module: None,
        }
    }

    pub fn with_definition_module(mut self, module: ModuleId) -> Self {
        self.definition_module = Some(module);
        self
    }
}

#[derive(Debug, Clone)]
pub enum StmtKind {
    Expression(Expr),

    Let {
        name: String,
        mutable: bool,
        type_annotation: Option<TypeAnnotation>,
        initializer: Expr,
        is_pub: bool,
    },

    Block(Vec<Stmt>),

    If {
        condition: Expr,
        then_branch: Box<Stmt>,
        else_branch: Option<Box<Stmt>>,
    },
    While {
        condition: Expr,
        body: Box<Stmt>,
    },

    For {
        iterator: String,
        start: Expr,
        end: Expr,
        inclusive: bool,
        step: Box<Option<Expr>>, // default: inferred from direction
        body: Box<Stmt>,
    },

    ForEach {
        iterator: String,
        iterable: Expr,
        body: Box<Stmt>,
    },

    Break,
    Continue,
    Return(Option<Expr>),
    Function(Function),
    ImplDecl {
        type_params: Vec<String>,
        trait_path: Option<TypeAnnotation>,
        self_type: TypeAnnotation,
        where_clauses: Vec<WhereClause>,
        methods: Vec<Function>,
        associated_types: Vec<AssociatedTypeDef>,
        associated_consts: Vec<AssociatedConstDef>,
    },
    TraitDecl {
        name: String,
        type_params: Vec<String>,
        super_bounds: Vec<TypeAnnotation>,
        where_clauses: Vec<WhereClause>,
        methods: Vec<TraitMethod>,
        associated_types: Vec<AssociatedTypeDecl>,
        associated_consts: Vec<AssociatedConstDecl>,
        is_pub: bool,
    },
    Needs(NeedsStmt),

    StructDecl {
        name: String,
        type_params: Vec<String>,
        fields: Vec<StructFieldDecl>,
        is_pub: bool,
    },

    EnumDecl {
        name: String,
        type_params: Vec<String>,
        variants: Vec<EnumVariantDecl>,
        is_pub: bool,
    },
}

#[derive(Debug, Clone)]
pub struct StructFieldDecl {
    pub name: String,
    pub type_annotation: TypeAnnotation,
    pub is_pub: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EnumVariantDecl {
    pub name: String,
    pub fields: EnumVariantFields,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum EnumVariantFields {
    Unit,
    Tuple(Vec<TypeAnnotation>),
    Named(Vec<StructFieldDecl>),
}

#[derive(Debug, Clone)]
pub struct TraitMethod {
    pub function: Function,
    pub has_body: bool,
}

#[derive(Debug, Clone)]
pub struct AssociatedTypeDecl {
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct AssociatedConstDecl {
    pub name: String,
    pub type_annotation: TypeAnnotation,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct AssociatedTypeDef {
    pub name: String,
    pub value: TypeAnnotation,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct AssociatedConstDef {
    pub name: String,
    pub type_annotation: TypeAnnotation,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct WhereClause {
    pub type_annotation: TypeAnnotation,
    pub bounds: Vec<TypeAnnotation>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct NeedsStmt {
    pub path: Vec<String>, // ["utils", "helpers"]
    pub kind: ImportKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ImportKind {
    Module { alias: Option<String> }, // needs foo.bar (as alias)?
    Symbols(Vec<String>),             // needs x, y from foo.bar
    Wildcard,                         // needs foo.bar.*
}

#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub type_params: Vec<String>,
    pub where_clauses: Vec<WhereClause>,
    pub params: Vec<Parameter>,
    pub return_type: Option<TypeAnnotation>,
    pub body: Vec<Stmt>,
    pub decorators: Vec<Decorator>,
    pub is_pub: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Decorator {
    pub name: String,
    pub span: Span,
}
