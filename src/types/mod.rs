use std::collections::BTreeMap;
use std::fmt;

pub mod ast;
pub mod value;

#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    Null,
    Bool,
    Int,
    Float,
    Str,
    Bytes,
    /// Untyped data. Must be `decode`d before fields can be read.
    Json,
    List(Box<Type>),
    Map(Box<Type>),
    Rec(BTreeMap<String, Type>),
    Option(Box<Type>),
    Fn(FnType),
    /// Checker-internal: produced after an error to avoid cascading diagnostics.
    /// A program containing it never passes the checker.
    Unknown,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FnType {
    pub params: Vec<(String, Type)>,
    pub ret: Box<Type>,
}

pub fn str_map() -> Type {
    Type::option(Type::map(Type::Str))
}

impl Type {
    pub fn option(t: Type) -> Type { Type::Option(Box::new(t)) }
    pub fn map(t: Type) -> Type { Type::Map(Box::new(t)) }
    pub fn list(t: Type) -> Type { Type::List(Box::new(t)) }
    pub fn rec<const N: usize>(fields: [(&str, Type); N]) -> Type {
        Type::Rec(fields.into_iter().map(|(k, t)| (k.to_string(), t)).collect())
    }
    
    /// Can a value of type `self` be used where `expected` is required?
    pub fn fits(&self, expected: &Type) -> bool {
        use Type::*;
        match (self, expected) {
            (Unknown, _) | (_, Unknown) => true,
            (a, b) if a == b => true,
            // Anything except functions can be treated as untyped json.
            (Fn(_), Json) => false,
            (_, Json) => true,
            (Null, Option(_)) => true,
            (Option(a), Option(b)) => a.fits(b),
            (a, Option(b)) => a.fits(b),
            (List(a), List(b)) => a.fits(b),
            (Map(a), Map(b)) => a.fits(b),
            // A record whose fields all fit T can be used as map<T>.
            (Rec(fields), Map(t)) => fields.values().all(|f| f.fits(t)),
            // Width subtyping: extra fields are allowed.
            (Rec(a), Rec(b)) => b.iter().all(|(k, bt)| a.get(k).is_some_and(|at| at.fits(bt))),
            _ => false,
        }
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self, Type::Int | Type::Float | Type::Unknown)
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Null => write!(f, "null"),
            Type::Bool => write!(f, "bool"),
            Type::Int => write!(f, "int"),
            Type::Float => write!(f, "float"),
            Type::Str => write!(f, "string"),
            Type::Bytes => write!(f, "bytes"),
            Type::Json => write!(f, "json"),
            Type::List(t) => write!(f, "list<{t}>"),
            Type::Map(t) => write!(f, "map<{t}>"),
            Type::Option(t) => write!(f, "option<{t}>"),
            Type::Rec(fields) => {
                write!(f, "{{")?;
                for (i, (k, t)) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{k}: {t}")?;
                }
                write!(f, "}}")
            }
            Type::Fn(ft) => {
                write!(f, "fn(")?;
                for (i, (k, t)) in ft.params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{k}: {t}")?;
                }
                write!(f, ") -> {}", ft.ret)
            }
            Type::Unknown => write!(f, "<unknown>"),
        }
    }
}
