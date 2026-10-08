//! Pure built-in functions. Always in scope, positional args only.
//! Type rules live in `check.rs`; this file is evaluation plus catalog text.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::rc::Rc;
use serde_json::Value as J;

use crate::{Diagnostic, Host, Program, check_program};
use crate::parse::parse_module;
use crate::types::value::Value;

/// `{"rec": {...}}` of strings -> pairs; `null` (omitted) -> nothing.
pub fn string_pairs(v: &Value) -> Vec<(String, String)> {
    match v {
        Value::Rec(m) => m.iter().map(|(k, v)| (k.clone(), v.render())).collect(),
        _ => Vec::new(),
    }
}

pub fn load(file: &str) -> Result<J, String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("{file}: invalid JSON: {e}"))
}

pub fn print(v: &J) {
    println!("{}", serde_json::to_string_pretty(v).expect("serializable"));
}

/// (name, signature, description). Feed this to agents as part of the catalog.
pub const BUILTINS: &[(&str, &str, &str)] = &[
    ("add", "(a: N, b: N) -> N", "Add two ints or two floats."),
    ("sub", "(a: N, b: N) -> N", "Subtract b from a."),
    ("mul", "(a: N, b: N) -> N", "Multiply."),
    ("div", "(a: N, b: N) -> N", "Divide. Int division truncates; dividing an int by 0 is an error."),
    ("mod", "(a: N, b: N) -> N", "Euclidean remainder."),
    ("eq", "(a: T, b: T) -> bool", "Structural equality."),
    ("neq", "(a: T, b: T) -> bool", "Structural inequality."),
    ("lt", "(a: N|string, b: same) -> bool", "Less than."),
    ("lte", "(a: N|string, b: same) -> bool", "Less than or equal."),
    ("gt", "(a: N|string, b: same) -> bool", "Greater than."),
    ("gte", "(a: N|string, b: same) -> bool", "Greater than or equal."),
    ("not", "(a: bool) -> bool", "Logical not."),
    ("concat", "(...parts: any) -> string", "Join the display form of every argument into one string."),
    ("to_string", "(a: any) -> string", "Display form of a value."),
    ("len", "(a: string|list<T>) -> int", "Characters in a string or items in a list."),
    ("push", "(xs: list<T>, x: T) -> list<T>", "New list with x appended."),
    ("unwrap_or", "(x: option<T>, default: T) -> T", "x if it isn't null, otherwise default."),
];

pub fn is_builtin(name: &str) -> bool {
    BUILTINS.iter().any(|(n, _, _)| *n == name)
}

fn overflow() -> String {
    "integer overflow".into()
}

pub fn call(name: &str, args: &[Value]) -> Result<Value, String> {
    use Value::*;
    match (name, args) {
        ("add", [Int(a), Int(b)]) => a.checked_add(*b).map(Int).ok_or_else(overflow),
        ("add", [Float(a), Float(b)]) => Ok(Float(a + b)),
        ("sub", [Int(a), Int(b)]) => a.checked_sub(*b).map(Int).ok_or_else(overflow),
        ("sub", [Float(a), Float(b)]) => Ok(Float(a - b)),
        ("mul", [Int(a), Int(b)]) => a.checked_mul(*b).map(Int).ok_or_else(overflow),
        ("mul", [Float(a), Float(b)]) => Ok(Float(a * b)),
        ("div" | "mod", [Int(_), Int(0)]) => Err("division by zero".into()),
        ("div", [Int(a), Int(b)]) => a.checked_div(*b).map(Int).ok_or_else(overflow),
        ("div", [Float(a), Float(b)]) => Ok(Float(a / b)),
        ("mod", [Int(a), Int(b)]) => a.checked_rem_euclid(*b).map(Int).ok_or_else(overflow),
        ("mod", [Float(a), Float(b)]) => Ok(Float(a.rem_euclid(*b))),
        ("eq", [a, b]) => Ok(Bool(a.equals(b))),
        ("neq", [a, b]) => Ok(Bool(!a.equals(b))),
        ("lt" | "lte" | "gt" | "gte", [a, b]) => {
            let ord = compare(a, b)?;
            Ok(Bool(match name {
                "lt" => ord.is_lt(),
                "lte" => ord.is_le(),
                "gt" => ord.is_gt(),
                _ => ord.is_ge(),
            }))
        }
        ("not", [Bool(b)]) => Ok(Bool(!b)),
        ("concat", parts) => Ok(Value::str(parts.iter().map(Value::render).collect::<String>())),
        ("to_string", [x]) => Ok(Value::str(x.render())),
        ("len", [Str(s)]) => Ok(Int(s.chars().count() as i64)),
        ("len", [List(xs)]) => Ok(Int(xs.len() as i64)),
        ("push", [List(xs), x]) => {
            let mut out = (**xs).clone();
            out.push(x.clone());
            Ok(List(Rc::new(out)))
        }
        ("unwrap_or", [Null, default]) => Ok(default.clone()),
        ("unwrap_or", [x, _]) => Ok(x.clone()),
        _ => Err(format!("internal error: bad arguments to built-in {name}")),
    }
}

fn compare(a: &Value, b: &Value) -> Result<Ordering, String> {
    match (a, b) {
        (Value::Int(a), Value::Int(b)) => Ok(a.cmp(b)),
        (Value::Float(a), Value::Float(b)) => a.partial_cmp(b).ok_or_else(|| "cannot compare NaN".into()),
        (Value::Str(a), Value::Str(b)) => Ok(a.cmp(b)),
        _ => Err("internal error: incomparable values".into()),
    }
}

pub fn compile(src: &J, deps: &[(String, String)], host: &dyn Host) -> Result<Program, Vec<Diagnostic>> {
    let entry = parse_module(src).map_err(|d| vec![d])?;
    let mut modules = HashMap::new();
    for (path, file) in deps {
        let json = load(file).map_err(|m| vec![Diagnostic::new(path.as_str(), "load_error", m)])?;
        let m = parse_module(&json).map_err(|mut d| {
            d.path = format!("{path}#{}", d.path);
            vec![d]
        })?;
        modules.insert(path.clone(), m);
    }
    check_program(entry, modules, host)
}
