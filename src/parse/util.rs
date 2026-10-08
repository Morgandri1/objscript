use serde_json::{Map, Value as J};
use crate::diag::{join, Diagnostic};

pub const MODULE_KEYS: &[&str] = &[
    "$schema", "objscript", "name", "version", "description", "command", "imports", "params", "returns", "body", "tests",
];
pub const STMT_KW: [&str; 6] = ["let", "set", "if", "while", "return", "do"];
pub const EXPR_KW: [&str; 8] = ["ref", "call", "rec", "get", "and", "or", "fn", "decode"];

pub type R<T> = Result<T, Diagnostic>;

pub fn kind(v: &J) -> &'static str {
    match v {
        J::Null => "null",
        J::Bool(_) => "boolean",
        J::Number(_) => "number",
        J::String(_) => "string",
        J::Array(_) => "array",
        J::Object(_) => "object",
    }
}

pub fn as_obj<'a>(v: &'a J, path: &str) -> R<&'a Map<String, J>> {
    v.as_object()
        .ok_or_else(|| Diagnostic::new(path, "expected_object", format!("expected an object, found {}", kind(v))))
}

pub fn req<'a>(obj: &'a Map<String, J>, path: &str, key: &str) -> R<&'a J> {
    obj.get(key).ok_or_else(|| missing(path, key))
}

pub fn missing(path: &str, key: &str) -> Diagnostic {
    Diagnostic::new(path, "missing_field", format!("missing required field \"{key}\""))
}

pub fn opt_str(obj: &Map<String, J>, path: &str, key: &str) -> R<Option<String>> {
    match obj.get(key) {
        None => Ok(None),
        Some(J::String(s)) => Ok(Some(s.clone())),
        Some(v) => Err(Diagnostic::new(join(path, key), "expected_string", format!("expected a string, found {}", kind(v)))),
    }
}

pub fn check_keys(obj: &Map<String, J>, path: &str, allowed: &[&str]) -> R<()> {
    for k in obj.keys() {
        if !allowed.contains(&k.as_str()) {
            return Err(Diagnostic::new(join(path, k), "unknown_field", format!("unknown field \"{k}\""))
                .hint(format!("allowed fields here: {}", allowed.join(", "))));
        }
    }
    Ok(())
}

pub fn ident(s: &str, path: &str) -> R<()> {
    let mut chars = s.chars();
    let ok = matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    if ok {
        Ok(())
    } else {
        Err(Diagnostic::new(path, "invalid_identifier", format!("\"{s}\" is not a valid identifier"))
            .hint("use letters, digits and _, not starting with a digit"))
    }
}

pub fn ident_val(v: &J, path: &str) -> R<String> {
    match v {
        J::String(s) => ident(s, path).map(|_| s.clone()),
        other => Err(Diagnostic::new(path, "expected_string", format!("expected a name, found {}", kind(other)))),
    }
}
