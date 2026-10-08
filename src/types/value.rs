use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use crate::types::ast::{FnDef, Module};
use crate::types::Type;

#[derive(Clone, Debug)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Rc<str>),
    Bytes(Rc<[u8]>),
    List(Rc<Vec<Value>>),
    Rec(Rc<BTreeMap<String, Value>>),
    Fn(Rc<Closure>),
}

#[derive(Debug)]
pub struct Closure {
    pub def: Rc<FnDef>,
    /// Captured by value at creation time.
    pub env: HashMap<String, Value>,
    /// Defining module, so calls inside the body resolve against its imports.
    pub module: Rc<Module>,
}

impl Value {
    pub fn str(s: impl AsRef<str>) -> Self {
        Value::Str(Rc::from(s.as_ref()))
    }

    pub fn from_json(v: &serde_json::Value) -> Self {
        use serde_json::Value as J;
        match v {
            J::Null => Value::Null,
            J::Bool(b) => Value::Bool(*b),
            J::Number(n) => match n.as_i64() {
                Some(i) => Value::Int(i),
                None => Value::Float(n.as_f64().unwrap_or(f64::NAN)),
            },
            J::String(s) => Value::str(s),
            J::Array(xs) => Value::List(Rc::new(xs.iter().map(Value::from_json).collect())),
            J::Object(m) => Value::Rec(Rc::new(
                m.iter().map(|(k, v)| (k.clone(), Value::from_json(v))).collect(),
            )),
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::Value as J;
        match self {
            Value::Null => J::Null,
            Value::Bool(b) => J::Bool(*b),
            Value::Int(i) => J::from(*i),
            Value::Float(f) => serde_json::Number::from_f64(*f).map(J::Number).unwrap_or(J::Null),
            Value::Str(s) => J::String(s.to_string()),
            Value::Bytes(b) => J::Array(b.iter().map(|x| J::from(*x)).collect()),
            Value::List(xs) => J::Array(xs.iter().map(Value::to_json).collect()),
            Value::Rec(m) => J::Object(m.iter().map(|(k, v)| (k.clone(), v.to_json())).collect()),
            Value::Fn(_) => J::String("<fn>".into()),
        }
    }

    /// Structural equality. Functions are never equal.
    pub fn equals(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Int(a), Value::Float(b)) | (Value::Float(b), Value::Int(a)) => (*a as f64) == *b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Bytes(a), Value::Bytes(b)) => a == b,
            (Value::List(a), Value::List(b)) => {
                a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.equals(y))
            }
            (Value::Rec(a), Value::Rec(b)) => {
                a.len() == b.len() && a.iter().all(|(k, v)| b.get(k).is_some_and(|w| v.equals(w)))
            }
            _ => false,
        }
    }

    /// Display form used by `to_string` and `concat`.
    pub fn render(&self) -> String {
        match self {
            Value::Str(s) => s.to_string(),
            Value::Null => "null".into(),
            other => other.to_json().to_string(),
        }
    }
}

/// Runtime check for `decode`: does `v` have shape `ty`? Returns the value
/// converted to the target type (ints widen to floats where a float is expected).
pub fn conform(v: &Value, ty: &Type, path: &str) -> Result<Value, String> {
    let bad = || Err(format!("at {path}: expected {ty}, found {}", v.to_json()));
    match (ty, v) {
        (Type::Json | Type::Unknown, _) => Ok(v.clone()),
        (Type::Null, Value::Null) => Ok(Value::Null),
        (Type::Bool, Value::Bool(_)) => Ok(v.clone()),
        (Type::Int, Value::Int(_)) => Ok(v.clone()),
        (Type::Float, Value::Float(_)) => Ok(v.clone()),
        (Type::Float, Value::Int(i)) => Ok(Value::Float(*i as f64)),
        (Type::Str, Value::Str(_)) => Ok(v.clone()),
        (Type::Bytes, Value::Bytes(_)) => Ok(v.clone()),
        (Type::Option(_), Value::Null) => Ok(Value::Null),
        (Type::Option(inner), _) => conform(v, inner, path),
        (Type::List(t), Value::List(xs)) => {
            let out: Result<Vec<_>, _> = xs
                .iter()
                .enumerate()
                .map(|(i, x)| conform(x, t, &format!("{path}/{i}")))
                .collect();
            Ok(Value::List(Rc::new(out?)))
        }
        (Type::Map(t), Value::Rec(m)) => {
            let mut out = BTreeMap::new();
            for (k, x) in m.iter() {
                out.insert(k.clone(), conform(x, t, &format!("{path}/{k}"))?);
            }
            Ok(Value::Rec(Rc::new(out)))
        }
        (Type::Rec(fields), Value::Rec(m)) => {
            let mut out = BTreeMap::new();
            for (k, t) in fields {
                let x = m.get(k).unwrap_or(&Value::Null);
                out.insert(k.clone(), conform(x, t, &format!("{path}/{k}"))?);
            }
            Ok(Value::Rec(Rc::new(out)))
        }
        _ => bad(),
    }
}
