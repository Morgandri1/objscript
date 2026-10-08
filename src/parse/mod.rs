//! JSON -> AST. Hand-written (instead of `#[serde(untagged)]`) so every error
//! carries a JSON Pointer and a hint an agent can act on. Fails on the first error;
//! the type checker is where multiple errors are collected.

use std::collections::BTreeMap;
use std::rc::Rc;
use serde_json::{Map, Value as J};
use crate::types::ast::*;
use crate::diag::{join, Diagnostic};
use crate::types::{FnType, Type};

mod util;
use util::*;

pub fn parse_module(src: &J) -> R<Module> {
    let obj = as_obj(src, "")?;
    check_keys(obj, "", MODULE_KEYS)?;
    match obj.get("objscript") {
        Some(J::String(v)) if v == "0.2" => {}
        Some(_) => {
            return Err(Diagnostic::new("/objscript", "unsupported_version", "only version \"0.2\" is supported"))
        }
        None => return Err(missing("", "objscript")),
    }
    let name = opt_str(obj, "", "name")?;
    let description = opt_str(obj, "", "description")?;

    let mut imports = Vec::new();
    if let Some(v) = obj.get("imports") {
        for (alias, path) in as_obj(v, "/imports")? {
            let ap = join("/imports", alias);
            ident(alias, &ap)?;
            let path = path
                .as_str()
                .ok_or_else(|| Diagnostic::new(&ap, "expected_string", "import path must be a string"))?;
            imports.push((alias.clone(), path.to_string()));
        }
    }

    let mut params = Vec::new();
    if let Some(v) = obj.get("params") {
        for (name, spec) in as_obj(v, "/params")? {
            let pp = join("/params", name);
            ident(name, &pp)?;
            params.push(parse_param(name, spec, &pp)?);
        }
    }

    let returns = parse_type(req(obj, "", "returns")?, "/returns")?;
    let body = parse_block(req(obj, "", "body")?, "/body")?;
    Ok(Module { name, description, imports, params, returns, body })
}

fn parse_param(name: &str, v: &J, path: &str) -> R<Param> {
    if let J::Object(o) = v {
        if o.contains_key("type") {
            check_keys(o, path, &["type", "description"])?;
            let ty = parse_type(&o["type"], &join(path, "type"))?;
            let description = opt_str(o, path, "description")?;
            return Ok(Param { name: name.into(), ty, description });
        }
    }
    Ok(Param { name: name.into(), ty: parse_type(v, path)?, description: None })
}

pub fn parse_type(v: &J, path: &str) -> R<Type> {
    match v {
        J::String(s) => match s.as_str() {
            "null" => Ok(Type::Null),
            "bool" => Ok(Type::Bool),
            "int" => Ok(Type::Int),
            "float" => Ok(Type::Float),
            "string" => Ok(Type::Str),
            "bytes" => Ok(Type::Bytes),
            "json" => Ok(Type::Json),
            other => Err(Diagnostic::new(path, "unknown_type", format!("unknown type \"{other}\""))
                .hint("primitive types are: null, bool, int, float, string, bytes, json")),
        },
        J::Object(m) if m.len() == 1 => {
            let (k, inner) = m.iter().next().expect("len == 1");
            let ip = join(path, k);
            match k.as_str() {
                "list" => Ok(Type::List(Box::new(parse_type(inner, &ip)?))),
                "map" => Ok(Type::Map(Box::new(parse_type(inner, &ip)?))),
                "option" => Ok(Type::Option(Box::new(parse_type(inner, &ip)?))),
                "rec" => {
                    let mut fields = BTreeMap::new();
                    for (name, t) in as_obj(inner, &ip)? {
                        fields.insert(name.clone(), parse_type(t, &join(&ip, name))?);
                    }
                    Ok(Type::Rec(fields))
                }
                "fn" => {
                    let o = as_obj(inner, &ip)?;
                    check_keys(o, &ip, &["params", "returns"])?;
                    let params = parse_typed_params(req(o, &ip, "params")?, &join(&ip, "params"))?;
                    let ret = parse_type(req(o, &ip, "returns")?, &join(&ip, "returns"))?;
                    Ok(Type::Fn(FnType { params, ret: Box::new(ret) }))
                }
                other => Err(Diagnostic::new(path, "unknown_type", format!("unknown type constructor \"{other}\""))
                    .hint("compound types are: list, map, option, rec, fn")),
            }
        }
        _ => Err(Diagnostic::new(
            path,
            "invalid_type",
            "a type must be a primitive name like \"int\" or a single-key object like {\"list\": \"int\"}",
        )),
    }
}

fn parse_typed_params(v: &J, path: &str) -> R<Vec<(String, Type)>> {
    let mut out = Vec::new();
    for (name, t) in as_obj(v, path)? {
        let p = join(path, name);
        ident(name, &p)?;
        out.push((name.clone(), parse_type(t, &p)?));
    }
    Ok(out)
}

fn parse_block(v: &J, path: &str) -> R<Block> {
    match v {
        J::Array(xs) => xs.iter().enumerate().map(|(i, x)| parse_stmt(x, &join(path, i))).collect(),
        other => Err(Diagnostic::new(path, "expected_block", format!("expected an array of statements, found {}", kind(other)))),
    }
}

fn parse_stmt(v: &J, path: &str) -> R<Stmt> {
    let o = as_obj(v, path)
        .map_err(|d| d.hint("a statement is an object like {\"let\": ...}, {\"if\": ...} or {\"return\": ...}"))?;
    let kw = STMT_KW.into_iter().find(|k| o.contains_key(*k)).ok_or_else(|| {
        Diagnostic::new(path, "unknown_statement", "statement must have one of: let, set, if, while, return, do")
            .hint("to call a function for its side effects, wrap it: {\"do\": {\"call\": ...}}")
    })?;

    let kind = match kw {
        "let" => {
            check_keys(o, path, &["let", "mut", "type", "value"])?;
            let name = ident_val(&o["let"], &join(path, "let"))?;
            let mutable = match o.get("mut") {
                None => false,
                Some(J::Bool(b)) => *b,
                Some(_) => return Err(Diagnostic::new(join(path, "mut"), "expected_bool", "\"mut\" must be true or false")),
            };
            let ty = o.get("type").map(|t| parse_type(t, &join(path, "type"))).transpose()?;
            let value = parse_expr(req(o, path, "value")?, &join(path, "value"))?;
            StmtKind::Let { name, mutable, ty, value }
        }
        "set" => {
            check_keys(o, path, &["set", "value"])?;
            let name = ident_val(&o["set"], &join(path, "set"))?;
            let value = parse_expr(req(o, path, "value")?, &join(path, "value"))?;
            StmtKind::Set { name, value }
        }
        "if" => {
            check_keys(o, path, &["if", "then", "else"])?;
            let cond = parse_expr(&o["if"], &join(path, "if"))?;
            let then = parse_block(req(o, path, "then")?, &join(path, "then"))?;
            let els = match o.get("else") {
                Some(b) => parse_block(b, &join(path, "else"))?,
                None => Vec::new(),
            };
            StmtKind::If { cond, then, els }
        }
        "while" => {
            check_keys(o, path, &["while", "do"])?;
            let cond = parse_expr(&o["while"], &join(path, "while"))?;
            let body = parse_block(req(o, path, "do")?, &join(path, "do"))?;
            StmtKind::While { cond, body }
        }
        "return" => {
            check_keys(o, path, &["return"])?;
            StmtKind::Return(parse_expr(&o["return"], &join(path, "return"))?)
        }
        _ => {
            check_keys(o, path, &["do"])?;
            StmtKind::Do(parse_expr(&o["do"], &join(path, "do"))?)
        }
    };
    Ok(Stmt { kind, at: path.to_string() })
}

pub fn parse_expr(v: &J, path: &str) -> R<Expr> {
    let kind = match v {
        J::Null => ExprKind::Null,
        J::Bool(b) => ExprKind::Bool(*b),
        J::Number(n) => match n.as_i64() {
            Some(i) => ExprKind::Int(i),
            None => ExprKind::Float(n.as_f64().unwrap_or(f64::NAN)),
        },
        J::String(s) => ExprKind::Str(s.clone()),
        J::Array(xs) => ExprKind::List(parse_exprs(xs, path)?),
        J::Object(o) => parse_node(o, path)?,
    };
    Ok(Expr { kind, at: path.to_string() })
}

fn parse_exprs(xs: &[J], path: &str) -> R<Vec<Expr>> {
    xs.iter().enumerate().map(|(i, x)| parse_expr(x, &join(path, i))).collect()
}

fn parse_node(o: &Map<String, J>, path: &str) -> R<ExprKind> {
    let kw = EXPR_KW.into_iter().find(|k| o.contains_key(*k)).ok_or_else(|| {
        let keys: Vec<&str> = o.keys().map(String::as_str).collect();
        Diagnostic::new(path, "unknown_expression", format!("object with keys [{}] is not an expression", keys.join(", ")))
            .hint("expression objects are: ref, call, rec, get, and, or, fn, decode. Write record literals as {\"rec\": {...}}")
    })?;

    Ok(match kw {
        "ref" => {
            check_keys(o, path, &["ref"])?;
            ExprKind::Ref(ident_val(&o["ref"], &join(path, "ref"))?)
        }
        "call" => {
            check_keys(o, path, &["call", "args"])?;
            let callee = ident_val(&o["call"], &join(path, "call"))?;
            let ap = join(path, "args");
            let args = match o.get("args") {
                None => Args::Positional(Vec::new()),
                Some(J::Array(xs)) => Args::Positional(parse_exprs(xs, &ap)?),
                Some(J::Object(m)) => {
                    let mut named = Vec::new();
                    for (k, x) in m {
                        let p = join(&ap, k);
                        ident(k, &p)?;
                        named.push((k.clone(), parse_expr(x, &p)?));
                    }
                    Args::Named(named)
                }
                Some(other) => {
                    return Err(Diagnostic::new(ap, "invalid_args", format!("\"args\" must be an array or object, found {}", kind(other))))
                }
            };
            ExprKind::Call { callee, args }
        }
        "rec" => {
            check_keys(o, path, &["rec"])?;
            let rp = join(path, "rec");
            let mut fields = Vec::new();
            for (k, x) in as_obj(&o["rec"], &rp)? {
                fields.push((k.clone(), parse_expr(x, &join(&rp, k))?));
            }
            ExprKind::Rec(fields)
        }
        "get" => {
            check_keys(o, path, &["get", "at"])?;
            let target = Box::new(parse_expr(&o["get"], &join(path, "get"))?);
            let at = match req(o, path, "at")? {
                J::String(s) => Key::Field(s.clone()),
                J::Number(n) if n.as_i64().is_some() => Key::Index(n.as_i64().expect("checked")),
                other => {
                    return Err(Diagnostic::new(
                        join(path, "at"),
                        "invalid_key",
                        format!("\"at\" must be a field name or integer index, found {}", kind(other)),
                    ))
                }
            };
            ExprKind::Get { target, at }
        }
        "and" | "or" => {
            check_keys(o, path, &[kw])?;
            let lp = join(path, kw);
            let xs = match &o[kw] {
                J::Array(xs) if xs.len() >= 2 => parse_exprs(xs, &lp)?,
                _ => {
                    return Err(Diagnostic::new(lp, "invalid_operands", format!("\"{kw}\" takes an array of at least 2 expressions")))
                }
            };
            if kw == "and" { ExprKind::And(xs) } else { ExprKind::Or(xs) }
        }
        "fn" => {
            check_keys(o, path, &["fn"])?;
            let fp = join(path, "fn");
            let f = as_obj(&o["fn"], &fp)?;
            check_keys(f, &fp, &["params", "returns", "body"])?;
            let params = parse_typed_params(req(f, &fp, "params")?, &join(&fp, "params"))?;
            let returns = parse_type(req(f, &fp, "returns")?, &join(&fp, "returns"))?;
            let body = parse_block(req(f, &fp, "body")?, &join(&fp, "body"))?;
            ExprKind::Fn(Rc::new(FnDef { params, returns, body }))
        }
        _ => {
            check_keys(o, path, &["decode", "as"])?;
            let value = Box::new(parse_expr(&o["decode"], &join(path, "decode"))?);
            let ty = parse_type(req(o, path, "as")?, &join(path, "as"))?;
            ExprKind::Decode { value, ty }
        }
    })
}

/// Parse function signature
pub fn parse_sig(params: &[(&str, Type)], ret: Type) -> FnType {
    FnType { 
        params: params
            .iter()
            .map(|(n, t)| (n.to_string(), t.clone()))
            .collect(), 
        ret: Box::new(ret) 
    }
}