//! Syntax tree produced by the parser.

use crate::source::Span;

#[derive(Clone, Debug)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Default)]
pub struct Program {
    pub items: Vec<Item>,
}

#[derive(Debug)]
pub enum Item {
    Struct(StructDecl),
    Enum(EnumDecl),
    Interface(InterfaceDecl),
    Fn(FnDecl),
    View(ViewDecl),
    App(AppDecl),
}

impl Item {
    pub fn name(&self) -> &Ident {
        match self {
            Item::Struct(d) => &d.name,
            Item::Enum(d) => &d.name,
            Item::Interface(d) => &d.name,
            Item::Fn(d) => &d.name,
            Item::View(d) => &d.name,
            Item::App(d) => &d.name,
        }
    }
}

#[derive(Debug)]
pub struct StructDecl {
    pub name: Ident,
    /// Interfaces it conforms to (`struct Circle: Shape`).
    pub conforms: Vec<Ident>,
    pub fields: Vec<FieldDecl>,
    pub methods: Vec<FnDecl>,
}

#[derive(Debug)]
pub struct FieldDecl {
    pub name: Ident,
    pub ty: TypeExpr,
    pub default: Option<Expr>,
}

#[derive(Debug)]
pub struct EnumDecl {
    pub name: Ident,
    pub conforms: Vec<Ident>,
    pub cases: Vec<CaseDecl>,
    pub methods: Vec<FnDecl>,
}

/// `interface Shape { fn area() -> Float }`: methods a type must have.
#[derive(Debug)]
pub struct InterfaceDecl {
    pub name: Ident,
    pub methods: Vec<MethodReq>,
}

/// A method an interface requires: its signature, without a body.
#[derive(Debug)]
pub struct MethodReq {
    pub name: Ident,
    pub params: Vec<Param>,
    pub ret: Option<TypeExpr>,
    pub span: Span,
}

#[derive(Debug)]
pub struct CaseDecl {
    pub name: Ident,
    pub fields: Vec<Param>,
}

#[derive(Debug)]
pub struct FnDecl {
    pub name: Ident,
    pub params: Vec<Param>,
    pub ret: Option<TypeExpr>,
    pub body: Block,
}

#[derive(Debug)]
pub struct Param {
    /// Declared as `_ name: T`: the argument is passed without a label.
    pub unlabeled: bool,
    pub name: Ident,
    /// Declared as `name: bind T`.
    pub bind: bool,
    pub ty: TypeExpr,
    pub default: Option<Expr>,
    pub span: Span,
}

#[derive(Debug)]
pub struct ViewDecl {
    pub name: Ident,
    pub params: Vec<Param>,
    pub body: Block,
}

#[derive(Debug)]
pub struct AppDecl {
    pub name: Ident,
    pub args: Vec<Arg>,
    pub args_span: Span,
    pub body: Block,
}

#[derive(Debug, Clone)]
pub enum TypeExpr {
    Named(Ident),
    List(Box<TypeExpr>, Span),
    Dict(Box<TypeExpr>, Box<TypeExpr>, Span),
    Optional(Box<TypeExpr>, Span),
    Fn(Vec<TypeExpr>, Option<Box<TypeExpr>>, Span),
}

impl TypeExpr {
    pub fn span(&self) -> Span {
        match self {
            TypeExpr::Named(id) => id.span,
            TypeExpr::List(_, s) | TypeExpr::Dict(_, _, s) | TypeExpr::Optional(_, s) | TypeExpr::Fn(_, _, s) => *s,
        }
    }
}

#[derive(Debug)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug)]
pub enum Stmt {
    Let(LocalDecl),
    Var(LocalDecl),
    State(LocalDecl),
    Fn(FnDecl),
    Assign { target: Expr, op: AssignOp, value: Expr, span: Span },
    Return { value: Option<Expr>, span: Span },
    For { var: Ident, iter: Expr, body: Block, span: Span },
    /// `while cond { … }`, also `while let x = … { … }`.
    While { cond: Cond, body: Block, span: Span },
    Break(Span),
    Continue(Span),
    Expr(Expr),
}

#[derive(Debug)]
pub struct LocalDecl {
    pub name: Ident,
    pub ty: Option<TypeExpr>,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AssignOp {
    Set,
    Add,
    Sub,
    Mul,
    Div,
}

pub type ExprId = u32;

#[derive(Debug)]
pub struct Expr {
    pub id: ExprId,
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug)]
pub enum ExprKind {
    Int(i64),
    Float(f64),
    Bool(bool),
    Nil,
    Str(Vec<StrPart>),
    Name(String),
    /// `.name` with the type taken from context: `.bold`, `.active`.
    DotName(String),
    List(Vec<Expr>),
    Dict(Vec<(Expr, Expr)>),
    Unary(UnaryOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Call { callee: Box<Expr>, args: Vec<Arg>, trailing: Option<Box<Expr>> },
    Member { object: Box<Expr>, name: Ident, optional: bool },
    Index(Box<Expr>, Box<Expr>),
    Closure { params: Vec<Ident>, body: Block },
    If { cond: Cond, then: Block, otherwise: Option<Else> },
    Match { subject: Box<Expr>, arms: Vec<Arm> },
    /// A `{ … }` block used as a value, e.g. a match arm `-> { … }`.
    Block(Block),
    /// Stand-in for an expression that failed to parse.
    Error,
}

impl Program {
    /// Expressions covering byte `offset` of `file`, innermost first.
    pub fn exprs_at<'p>(&'p self, file: u32, offset: u32) -> Vec<&'p Expr> {
        let mut found: Vec<&'p Expr> = Vec::new();
        let mut visit = |e: &'p Expr| {
            if e.span.file == file && e.span.lo <= offset && offset < e.span.hi.max(e.span.lo + 1) {
                found.push(e);
            }
        };
        for item in &self.items {
            item_exprs(item, &mut visit);
        }
        found.sort_by_key(|e| e.span.hi - e.span.lo);
        found
    }
}

fn item_exprs<'a>(item: &'a Item, f: &mut dyn FnMut(&'a Expr)) {
    let block = |b: &'a Block, f: &mut dyn FnMut(&'a Expr)| block_exprs(b, f);
    match item {
        Item::Struct(d) => {
            for field in &d.fields {
                if let Some(e) = &field.default {
                    expr_tree(e, f);
                }
            }
            for m in &d.methods {
                block(&m.body, f);
            }
        }
        Item::Enum(d) => d.methods.iter().for_each(|m| block(&m.body, f)),
        Item::Interface(_) => {}
        Item::Fn(d) => block(&d.body, f),
        Item::View(d) => block(&d.body, f),
        Item::App(d) => {
            for a in &d.args {
                expr_tree(&a.value, f);
            }
            block(&d.body, f);
        }
    }
}

/// Calls `f` on every expression in `b`, nested ones included.
pub fn block_exprs<'a>(b: &'a Block, f: &mut dyn FnMut(&'a Expr)) {
    for s in &b.stmts {
        match s {
            Stmt::Let(d) | Stmt::Var(d) | Stmt::State(d) => expr_tree(&d.value, f),
            Stmt::Fn(d) => block_exprs(&d.body, f),
            Stmt::Assign { target, value, .. } => {
                expr_tree(target, f);
                expr_tree(value, f);
            }
            Stmt::Return { value, .. } => {
                if let Some(v) = value {
                    expr_tree(v, f);
                }
            }
            Stmt::For { iter, body, .. } => {
                expr_tree(iter, f);
                block_exprs(body, f);
            }
            Stmt::While { cond, body, .. } => {
                match cond {
                    Cond::Expr(c) | Cond::Let(_, _, c) => expr_tree(c, f),
                }
                block_exprs(body, f);
            }
            Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::Expr(e) => expr_tree(e, f),
        }
    }
}

/// Calls `f` on `e` and every expression inside it.
fn expr_tree<'a>(e: &'a Expr, f: &mut dyn FnMut(&'a Expr)) {
    f(e);
    match &e.kind {
        ExprKind::Str(parts) => {
            for p in parts {
                if let StrPart::Expr(x) = p {
                    expr_tree(x, f);
                }
            }
        }
        ExprKind::List(items) => items.iter().for_each(|x| expr_tree(x, f)),
        ExprKind::Dict(pairs) => {
            for (k, v) in pairs {
                expr_tree(k, f);
                expr_tree(v, f);
            }
        }
        ExprKind::Unary(_, x) => expr_tree(x, f),
        ExprKind::Binary(_, l, r) | ExprKind::Index(l, r) => {
            expr_tree(l, f);
            expr_tree(r, f);
        }
        ExprKind::Call { callee, args, trailing } => {
            expr_tree(callee, f);
            for a in args {
                expr_tree(&a.value, f);
            }
            if let Some(t) = trailing {
                expr_tree(t, f);
            }
        }
        ExprKind::Member { object, .. } => expr_tree(object, f),
        ExprKind::Closure { body, .. } | ExprKind::Block(body) => block_exprs(body, f),
        ExprKind::If { cond, then, otherwise } => {
            match cond {
                Cond::Expr(c) | Cond::Let(_, _, c) => expr_tree(c, f),
            }
            block_exprs(then, f);
            match otherwise {
                Some(Else::Block(b)) => block_exprs(b, f),
                Some(Else::If(x)) => expr_tree(x, f),
                None => {}
            }
        }
        ExprKind::Match { subject, arms } => {
            expr_tree(subject, f);
            for a in arms {
                expr_tree(&a.body, f);
            }
        }
        _ => {}
    }
}

impl Expr {
    /// Is this part of an optional chain (`a?.b`, `a?.b.c`, `a?.f().g`)?
    /// Accesses after a `?.` apply to the unwrapped value, and the whole
    /// chain is `nil` if any `?.` finds `nil`.
    pub fn in_optional_chain(&self) -> bool {
        match &self.kind {
            ExprKind::Member { object, optional, .. } => *optional || object.in_optional_chain(),
            ExprKind::Call { callee, .. } => callee.in_optional_chain(),
            _ => false,
        }
    }
}

#[derive(Debug)]
pub enum StrPart {
    Text(String),
    Expr(Expr),
}

#[derive(Debug)]
pub struct Arg {
    pub label: Option<Ident>,
    pub value: Expr,
}

#[derive(Debug)]
pub enum Cond {
    Expr(Box<Expr>),
    /// `if let name = value`, or `if let name: Type = value`
    Let(Ident, Option<TypeExpr>, Box<Expr>),
}

#[derive(Debug)]
pub enum Else {
    Block(Block),
    If(Box<Expr>),
}

#[derive(Debug)]
pub struct Arm {
    pub pattern: Pattern,
    pub body: Expr,
    pub span: Span,
}

#[derive(Debug)]
pub enum Pattern {
    /// `.name` or `.name(a, b)`
    Case { name: Ident, bindings: Vec<Ident>, span: Span },
    Wildcard(Span),
    Literal(Expr),
    /// An arm that failed to parse; silences exhaustiveness errors.
    Error(Span),
}

impl Pattern {
    pub fn span(&self) -> Span {
        match self {
            Pattern::Case { span, .. } | Pattern::Wildcard(span) | Pattern::Error(span) => *span,
            Pattern::Literal(e) => e.span,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Coalesce,
    Range,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

impl BinOp {
    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::Or => "||",
            BinOp::And => "&&",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::Coalesce => "??",
            BinOp::Range => "..",
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Rem => "%",
        }
    }
}
