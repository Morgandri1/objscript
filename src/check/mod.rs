//! Static checker. Collects every error it can find (agents fix more per round
//! that way), and normalizes the AST so the interpreter can trust it:
//!   - named args are rewritten to positional, in parameter order
//!   - omitted `option<T>` args are filled with `null`
//!   - int literals used where a float is expected become float literals
//!
//! The only way to get a [`Program`] is through [`check_program`].

use std::collections::HashMap;
use std::rc::Rc;

use crate::types::ast::*;
use crate::diag::{join, Diagnostic};
use crate::host::Host;
use crate::stdlib;
use crate::types::{FnType, Type};

mod util;
use util::*;

mod program;
pub use program::Program;

/// Check an entry script and its library modules. `deps` is keyed by the exact
/// import path, including the version pin. Modules are checked against each
/// other's declared signatures, so order doesn't matter.
pub fn check_program(
    entry: Module,
    deps: HashMap<String, Module>,
    host: &dyn Host,
) -> Result<Program, Vec<Diagnostic>> {
    let sigs: HashMap<String, FnType> = deps.iter().map(|(p, m)| (p.clone(), signature(m))).collect();
    let mut errors = Vec::new();
    let mut modules = HashMap::new();

    for (path, mut m) in deps {
        let errs = check_module(&mut m, &sigs, host);
        errors.extend(errs.into_iter().map(|mut d| {
            d.path = format!("{path}#{}", d.path);
            d
        }));
        modules.insert(path, Rc::new(m));
    }

    let mut entry = entry;
    errors.extend(check_module(&mut entry, &sigs, host));

    if errors.is_empty() {
        Ok(Program { entry: Rc::new(entry), modules })
    } else {
        Err(errors)
    }
}

fn is_file_path(p: &str) -> bool {
    p.starts_with('/') || p.starts_with("./") || p.starts_with("../")
}

fn check_module(m: &mut Module, sigs: &HashMap<String, FnType>, host: &dyn Host) -> Vec<Diagnostic> {
    let mut c = Checker { imports: HashMap::new(), scopes: Vec::new(), returns: Vec::new(), errors: Vec::new() };

    for (alias, path) in &m.imports {
        let at = join("/imports", alias);
        let sig = if path.starts_with("std/") {
            host.caps().iter()
                .find(|c| c.path == path)
                .map(|c| c.signature.clone())
                .ok_or_else(|| {
                    Diagnostic::new(
                        &at, "capability_not_granted", 
                        format!("host capability \"{path}\" is not available")
                    ).hint("check the catalog for granted std/* capabilities")
                })
        } else if path.starts_with('@') || is_file_path(path) {
            sigs.get(path).cloned()
                .ok_or_else(|| { 
                    Diagnostic::new(
                        &at, "unknown_module", 
                        format!("module \"{path}\" is not loaded")
                    ).hint("load the module's file into the interpreter, then import it by name or path")
                })
        } else {
            Err(Diagnostic::new(
                &at, "invalid_import", 
                format!("\"{path}\" is not a valid import")
            ).hint("imports are std/*, @org/pkg/name, or a path to a .json file; std functions need no import"))
        };
        match sig {
            Ok(sig) => {
                c.imports.insert(alias.clone(), sig);
            }
            Err(d) => c.errors.push(d),
        }
    }

    let params: HashMap<String, Binding> =
        m.params.iter().map(|p| (p.name.clone(), Binding { ty: p.ty.clone(), mutable: false })).collect();
    c.scopes.push(params);
    c.returns.push(m.returns.clone());
    c.check_block(&mut m.body);
    if !always_returns(&m.body) && !Type::Null.fits(&m.returns) {
        c.report(Diagnostic::new("/body", "missing_return", format!("body can finish without returning a {}", m.returns))
            .hint("end the body with a {\"return\": ...} statement"));
    }
    c.errors
}

struct Checker {
    /// alias -> signature, for both host capabilities and modules.
    imports: HashMap<String, FnType>,
    scopes: Vec<HashMap<String, Binding>>,
    returns: Vec<Type>,
    errors: Vec<Diagnostic>,
}



impl Checker {
    fn report(&mut self, d: Diagnostic) {
        self.errors.push(d);
    }

    fn lookup(&self, name: &str) -> Option<Binding> {
        self.scopes.iter().rev().find_map(|s| s.get(name).cloned())
    }

    fn names_in_scope(&self) -> String {
        let mut names: Vec<&str> = self.scopes.iter().flat_map(|s| s.keys().map(String::as_str)).collect();
        names.sort_unstable();
        names.dedup();
        if names.is_empty() { "(none)".into() } else { names.join(", ") }
    }

    // ---------- statements ----------

    fn check_block(&mut self, block: &mut Block) {
        self.scopes.push(HashMap::new());
        for s in block.iter_mut() {
            self.check_stmt(s);
        }
        self.scopes.pop();
    }

    fn check_stmt(&mut self, s: &mut Stmt) {
        let at = s.at.clone();
        match &mut s.kind {
            StmtKind::Let { name, mutable, ty, value } => {
                let bound = match ty.as_ref() {
                    Some(t) => {
                        self.check_against(value, t);
                        t.clone()
                    }
                    None => self.check_expr(value, None),
                };
                if bound == Type::Null && ty.is_none() {
                    self.report(Diagnostic::new(join(&at, "value"), "cannot_infer", "cannot infer a type from null")
                        .hint("add a type, e.g. \"type\": {\"option\": \"string\"}"));
                }
                let scope = self.scopes.last_mut().expect("block scope");
                if scope.contains_key(name.as_str()) {
                    let d = Diagnostic::new(join(&at, "let"), "already_defined", format!("\"{name}\" is already defined in this block"))
                        .hint("use {\"set\": ...} to change a mut variable, or pick a new name");
                    self.report(d);
                } else {
                    scope.insert(name.clone(), Binding { ty: bound, mutable: *mutable });
                }
            }
            StmtKind::Set { name, value } => match self.lookup(name) {
                None => {
                    let d = Diagnostic::new(join(&at, "set"), "unknown_variable", format!("\"{name}\" is not defined"))
                        .hint(format!("in scope: {}", self.names_in_scope()));
                    self.report(d);
                    self.check_expr(value, None);
                }
                Some(b) => {
                    if !b.mutable {
                        self.report(Diagnostic::new(join(&at, "set"), "immutable", format!("\"{name}\" is not mutable"))
                            .hint("declare it with \"mut\": true (variables captured by a fn are always read-only)"));
                    }
                    self.check_against(value, &b.ty);
                }
            },
            StmtKind::If { cond, then, els } => {
                self.check_against(cond, &Type::Bool);
                self.check_block(then);
                self.check_block(els);
            }
            StmtKind::While { cond, body } => {
                self.check_against(cond, &Type::Bool);
                self.check_block(body);
            }
            StmtKind::Return(e) => {
                let ret = self.returns.last().cloned().expect("return context");
                self.check_against(e, &ret);
            }
            StmtKind::Do(e) => {
                self.check_expr(e, None);
            }
        }
    }

    // ---------- expressions ----------

    fn check_against(&mut self, e: &mut Expr, expected: &Type) -> Type {
        let actual = self.check_expr(e, Some(expected));
        if !actual.fits(expected) {
            let mut d = Diagnostic::new(&e.at, "type_mismatch", format!("expected {expected}, found {actual}"));
            if *expected == Type::Str {
                d = d.hint("convert with {\"call\": \"to_string\", \"args\": [...]}");
            } else if wants_float(expected) && actual == Type::Int {
                d = d.hint("ints don't convert to floats implicitly; write a float literal like 1.0");
            } else if actual == Type::Json {
                d = d.hint(format!("use {{\"decode\": ..., \"as\": {expected}}} to give json a type"));
            }
            self.report(d);
        }
        actual
    }

    fn check_expr(&mut self, e: &mut Expr, expected: Option<&Type>) -> Type {
        if let ExprKind::Int(i) = e.kind {
            if expected.is_some_and(wants_float) {
                e.kind = ExprKind::Float(i as f64);
            }
        }
        let at = e.at.clone();
        match &mut e.kind {
            ExprKind::Null => Type::Null,
            ExprKind::Bool(_) => Type::Bool,
            ExprKind::Int(_) => Type::Int,
            ExprKind::Float(_) => Type::Float,
            ExprKind::Str(_) => Type::Str,

            ExprKind::List(xs) => {
                let elem_hint = match expected {
                    Some(Type::List(t)) => Some((**t).clone()),
                    Some(Type::Option(o)) => match &**o {
                        Type::List(t) => Some((**t).clone()),
                        _ => None,
                    },
                    _ => None,
                };
                // Without a hint, the first element decides the element type.
                let (elem, skip) = match (elem_hint, xs.first_mut()) {
                    (Some(t), _) => (t, 0),
                    (None, Some(first)) => (self.check_expr(first, None), 1),
                    (None, None) => {
                        self.report(Diagnostic::new(&at, "cannot_infer", "cannot infer the element type of an empty list")
                            .hint("add a type, e.g. \"type\": {\"list\": \"string\"}"));
                        return Type::List(Box::new(Type::Unknown));
                    }
                };
                for x in xs.iter_mut().skip(skip) {
                    self.check_against(x, &elem);
                }
                Type::List(Box::new(elem))
            }

            ExprKind::Rec(fields) => {
                let mut out = std::collections::BTreeMap::new();
                for (k, x) in fields.iter_mut() {
                    let hint = match expected {
                        Some(Type::Rec(ft)) => ft.get(k.as_str()).cloned(),
                        Some(Type::Map(t)) => Some((**t).clone()),
                        _ => None,
                    };
                    let t = match &hint {
                        Some(h) => self.check_expr(x, Some(h)),
                        None => self.check_expr(x, None),
                    };
                    out.insert(k.clone(), t);
                }
                Type::Rec(out)
            }

            ExprKind::Ref(name) => match self.lookup(name) {
                Some(b) => b.ty,
                None => {
                    let d = if self.imports.contains_key(name.as_str()) || stdlib::is_builtin(name) {
                        Diagnostic::new(&at, "not_a_value", format!("\"{name}\" is a function, not a variable"))
                            .hint(format!("call it with {{\"call\": \"{name}\", \"args\": ...}}"))
                    } else {
                        Diagnostic::new(&at, "unknown_variable", format!("\"{name}\" is not defined"))
                            .hint(format!("in scope: {}", self.names_in_scope()))
                    };
                    self.report(d);
                    Type::Unknown
                }
            },

            ExprKind::Call { callee, args } => {
                let callee = callee.clone();
                self.check_call(&callee, args, &at, expected)
            }

            ExprKind::Get { target, at: key } => {
                let t = self.check_expr(target, None);
                match (&t, &*key) {
                    (Type::Unknown, _) => Type::Unknown,
                    (Type::Rec(fields), Key::Field(k)) => match fields.get(k.as_str()) {
                        Some(ft) => ft.clone(),
                        None => {
                            let known: Vec<&str> = fields.keys().map(String::as_str).collect();
                            self.report(Diagnostic::new(join(&at, "at"), "unknown_field", format!("{t} has no field \"{k}\""))
                                .hint(format!("fields: {}", known.join(", "))));
                            Type::Unknown
                        }
                    },
                    // Missing map keys and out-of-range indexes are runtime errors.
                    (Type::Map(v), Key::Field(_)) => (**v).clone(),
                    (Type::List(v), Key::Index(_)) => (**v).clone(),
                    (Type::Json, _) => {
                        self.report(Diagnostic::new(&at, "untyped_json", "cannot read from untyped json")
                            .hint("decode it first: {\"decode\": ..., \"as\": {\"rec\": {...}}}"));
                        Type::Unknown
                    }
                    (Type::Option(_), _) => {
                        self.report(Diagnostic::new(&at, "maybe_null", format!("{t} may be null"))
                            .hint("use {\"call\": \"unwrap_or\", \"args\": [value, default]} first"));
                        Type::Unknown
                    }
                    _ => {
                        self.report(Diagnostic::new(&at, "invalid_get", format!("cannot index {t} with this key")));
                        Type::Unknown
                    }
                }
            }

            ExprKind::And(xs) | ExprKind::Or(xs) => {
                for x in xs.iter_mut() {
                    self.check_against(x, &Type::Bool);
                }
                Type::Bool
            }

            ExprKind::Fn(def) => {
                let def = Rc::make_mut(def);
                // Closures capture by value, so captured variables are read-only inside.
                let frozen: HashMap<String, Binding> = self
                    .scopes
                    .iter()
                    .flat_map(|s| s.iter())
                    .map(|(k, b)| (k.clone(), Binding { ty: b.ty.clone(), mutable: false }))
                    .collect();
                let params: HashMap<String, Binding> =
                    def.params.iter().map(|(n, t)| (n.clone(), Binding { ty: t.clone(), mutable: false })).collect();
                let saved = std::mem::replace(&mut self.scopes, vec![frozen, params]);
                self.returns.push(def.returns.clone());
                self.check_block(&mut def.body);
                if !always_returns(&def.body) && !Type::Null.fits(&def.returns) {
                    self.report(Diagnostic::new(join(&join(&at, "fn"), "body"), "missing_return", format!("fn can finish without returning a {}", def.returns)));
                }
                self.returns.pop();
                self.scopes = saved;
                Type::Fn(FnType { params: def.params.clone(), ret: Box::new(def.returns.clone()) })
            }

            ExprKind::Decode { value, ty } => {
                if let Type::Fn(_) = self.check_expr(value, None) {
                    self.report(Diagnostic::new(join(&at, "decode"), "invalid_decode", "cannot decode a function"));
                }
                ty.clone()
            }
        }
    }

    // ---------- calls ----------

    fn check_call(&mut self, callee: &str, args: &mut Args, at: &str, expected: Option<&Type>) -> Type {
        // Resolution order (mirrored by the interpreter): locals, imports, built-ins.
        if let Some(b) = self.lookup(callee) {
            return match b.ty {
                Type::Fn(ft) => {
                    self.check_args(args, &ft.params, at);
                    *ft.ret
                }
                Type::Unknown => Type::Unknown,
                other => {
                    self.report(Diagnostic::new(join(at, "call"), "not_callable", format!("\"{callee}\" is a {other}, not a function")));
                    Type::Unknown
                }
            };
        }
        if let Some(ft) = self.imports.get(callee).cloned() {
            self.check_args(args, &ft.params, at);
            return *ft.ret;
        }
        if stdlib::is_builtin(callee) {
            return match args {
                Args::Positional(xs) => self.check_builtin(callee, xs, at, expected),
                Args::Named(_) => {
                    self.report(Diagnostic::new(join(at, "args"), "positional_only", format!("built-in \"{callee}\" takes positional args"))
                        .hint("pass an array: \"args\": [a, b]"));
                    Type::Unknown
                }
            };
        }
        let builtins: Vec<&str> = stdlib::BUILTINS.iter().map(|(n, _, _)| *n).collect();
        self.report(Diagnostic::new(join(at, "call"), "unknown_function", format!("\"{callee}\" is not a function in scope"))
            .hint(format!("import it, or use a built-in: {}", builtins.join(", "))));
        // Still check the arguments so nested errors surface in the same round.
        match args {
            Args::Positional(xs) => xs.iter_mut().for_each(|x| {
                self.check_expr(x, None);
            }),
            Args::Named(named) => named.iter_mut().for_each(|(_, x)| {
                self.check_expr(x, None);
            }),
        }
        Type::Unknown
    }

    fn check_args(&mut self, args: &mut Args, params: &[(String, Type)], at: &str) {
        let ap = join(at, "args");
        match &mut *args {
            Args::Positional(xs) => {
                if xs.len() != params.len() {
                    self.report(Diagnostic::new(&ap, "arity", format!("expected {} args, got {}", params.len(), xs.len()))
                        .hint(format!("parameters: {}", param_list(params))));
                }
                for (x, (_, t)) in xs.iter_mut().zip(params) {
                    self.check_against(x, t);
                }
            }
            Args::Named(named) => {
                let mut slots: Vec<Option<Expr>> = params.iter().map(|_| None).collect();
                for (k, x) in named.drain(..) {
                    match params.iter().position(|(p, _)| *p == k) {
                        Some(i) => slots[i] = Some(x),
                        None => self.report(Diagnostic::new(&x.at, "unknown_arg", format!("unexpected argument \"{k}\""))
                            .hint(format!("parameters: {}", param_list(params)))),
                    }
                }
                let mut out = Vec::with_capacity(params.len());
                for ((p, t), slot) in params.iter().zip(slots) {
                    let mut x = match slot {
                        Some(x) => x,
                        None if Type::Null.fits(t) => Expr { kind: ExprKind::Null, at: join(&ap, p) },
                        None => {
                            self.report(Diagnostic::new(&ap, "missing_arg", format!("missing argument \"{p}\" of type {t}")));
                            Expr { kind: ExprKind::Null, at: join(&ap, p) }
                        }
                    };
                    self.check_against(&mut x, t);
                    out.push(x);
                }
                *args = Args::Positional(out);
            }
        }
    }

    fn check_builtin(&mut self, name: &str, xs: &mut [Expr], at: &str, expected: Option<&Type>) -> Type {
        let want = match name {
            "not" | "to_string" | "len" => Some(1),
            "concat" => None,
            _ => Some(2),
        };
        if let Some(n) = want {
            if xs.len() != n {
                let sig = stdlib::BUILTINS.iter().find(|(b, _, _)| *b == name).map(|(_, s, _)| *s).unwrap_or("");
                self.report(Diagnostic::new(join(at, "args"), "arity", format!("{name} takes {n} args, got {}", xs.len()))
                    .hint(format!("{name}{sig}")));
                return Type::Unknown;
            }
        }
        let float_hint = expected.is_some_and(wants_float) || xs.iter().any(|x| matches!(x.kind, ExprKind::Float(_)));
        let float_ty = Type::Float;
        let num_hint = float_hint.then_some(&float_ty);

        match name {
            "add" | "sub" | "mul" | "div" | "mod" => {
                let ta = self.check_expr(&mut xs[0], num_hint);
                if !ta.is_numeric() {
                    self.report(Diagnostic::new(&xs[0].at, "type_mismatch", format!("{name} needs int or float, found {ta}")));
                    return Type::Unknown;
                }
                self.check_against(&mut xs[1], &ta);
                ta
            }
            "lt" | "lte" | "gt" | "gte" => {
                let ta = self.check_expr(&mut xs[0], num_hint);
                if !ta.is_numeric() && ta != Type::Str {
                    self.report(Diagnostic::new(&xs[0].at, "type_mismatch", format!("{name} needs int, float or string, found {ta}")));
                } else {
                    self.check_against(&mut xs[1], &ta);
                }
                Type::Bool
            }
            "eq" | "neq" => {
                let ta = self.check_expr(&mut xs[0], num_hint);
                let tb = self.check_expr(&mut xs[1], Some(&ta));
                let ok = tb.fits(&ta) || ta.fits(&tb) || (ta.is_numeric() && tb.is_numeric());
                if !ok {
                    self.report(Diagnostic::new(join(at, "args"), "type_mismatch", format!("cannot compare {ta} with {tb}")));
                }
                if matches!(ta, Type::Fn(_)) {
                    self.report(Diagnostic::new(&xs[0].at, "type_mismatch", "functions cannot be compared"));
                }
                Type::Bool
            }
            "not" => {
                self.check_against(&mut xs[0], &Type::Bool);
                Type::Bool
            }
            "concat" | "to_string" => {
                for x in xs.iter_mut() {
                    if let Type::Fn(_) = self.check_expr(x, None) {
                        self.report(Diagnostic::new(&x.at, "type_mismatch", "functions cannot be converted to strings"));
                    }
                }
                Type::Str
            }
            "len" => {
                let t = self.check_expr(&mut xs[0], None);
                if !matches!(t, Type::Str | Type::List(_) | Type::Unknown) {
                    self.report(Diagnostic::new(&xs[0].at, "type_mismatch", format!("len needs a string or list, found {t}")));
                }
                Type::Int
            }
            "push" => {
                let tl = self.check_expr(&mut xs[0], expected);
                match tl {
                    Type::List(elem) => {
                        self.check_against(&mut xs[1], &elem);
                        Type::List(elem)
                    }
                    Type::Unknown => Type::Unknown,
                    other => {
                        self.report(Diagnostic::new(&xs[0].at, "type_mismatch", format!("push needs a list, found {other}")));
                        Type::Unknown
                    }
                }
            }
            "unwrap_or" => {
                let hint = expected.map(|t| Type::Option(Box::new(t.clone())));
                let t = self.check_expr(&mut xs[0], hint.as_ref());
                let inner = match t {
                    Type::Option(inner) => *inner,
                    Type::Null => match expected {
                        Some(e) => e.clone(),
                        None => Type::Unknown,
                    },
                    other => other,
                };
                self.check_against(&mut xs[1], &inner);
                inner
            }
            _ => unreachable!("is_builtin checked"),
        }
    }
}
