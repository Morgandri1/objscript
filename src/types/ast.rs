use std::rc::Rc;

use crate::types::Type;

/// JSON Pointer to the source node, used in every diagnostic.
pub type Path = String;

#[derive(Clone, Debug)]
pub struct Module {
    pub name: Option<String>,
    pub description: Option<String>,
    /// alias -> import path, in source order.
    pub imports: Vec<(String, String)>,
    pub params: Vec<Param>,
    pub returns: Type,
    pub body: Block,
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: String,
    pub ty: Type,
    pub description: Option<String>,
}

pub type Block = Vec<Stmt>;

#[derive(Clone, Debug)]
pub struct Stmt {
    pub kind: StmtKind,
    pub at: Path,
}

#[derive(Clone, Debug)]
pub enum StmtKind {
    Let { name: String, mutable: bool, ty: Option<Type>, value: Expr },
    Set { name: String, value: Expr },
    If { cond: Expr, then: Block, els: Block },
    While { cond: Expr, body: Block },
    Return(Expr),
    Do(Expr),
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub at: Path,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<Expr>),
    Rec(Vec<(String, Expr)>),
    Ref(String),
    Call { callee: String, args: Args },
    Get { target: Box<Expr>, at: Key },
    And(Vec<Expr>),
    Or(Vec<Expr>),
    Fn(Rc<FnDef>),
    Decode { value: Box<Expr>, ty: Type },
}

#[derive(Clone, Debug)]
pub enum Args {
    Positional(Vec<Expr>),
    /// Rewritten to `Positional` (in parameter order) by the checker.
    Named(Vec<(String, Expr)>),
}

#[derive(Clone, Debug)]
pub enum Key {
    Field(String),
    Index(i64),
}

#[derive(Clone, Debug)]
pub struct FnDef {
    pub params: Vec<(String, Type)>,
    pub returns: Type,
    pub body: Block,
}
