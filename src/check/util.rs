use crate::types::ast::{Block, StmtKind, Module};
use crate::types::{FnType, Type};

pub fn always_returns(block: &Block) -> bool {
    block.iter().any(|s| match &s.kind {
        StmtKind::Return(_) => true,
        StmtKind::If { then, els, .. } => always_returns(then) && always_returns(els),
        _ => false,
    })
}

pub fn wants_float(t: &Type) -> bool {
    match t {
        Type::Float => true,
        Type::Option(inner) => wants_float(inner),
        _ => false,
    }
}

pub fn param_list(params: &[(String, Type)]) -> String {
    params.iter().map(|(n, t)| format!("{n}: {t}")).collect::<Vec<_>>().join(", ")
}

pub fn signature(m: &Module) -> FnType {
    FnType {
        params: m.params.iter().map(|p| (p.name.clone(), p.ty.clone())).collect(),
        ret: Box::new(m.returns.clone()),
    }
}

#[derive(Clone)]
pub struct Binding {
    pub ty: Type,
    pub mutable: bool,
}