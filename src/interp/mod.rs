//! Tree-walking interpreter over a checked [`Program`].
//!
//! This proof of concept calls the host synchronously. The production design
//! compiles to bytecode so a run can pause on host calls (async I/O, WASM);
//! the semantics here are the reference for that.

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use crate::types::ast::*;
use crate::check::Program;
use crate::host::Host;
use crate::stdlib;
use crate::types::Type;
use crate::types::value::{conform, Closure, Value};

pub mod err;
pub use err::RuntimeError;

pub mod types;
pub use types::{Limits, Outcome};
use types::{Flow, Frame};

/// Run the entry script. `args` is external input (e.g. Discord options), so
/// it is validated against the declared params at runtime.
pub fn run(program: &Program, host: &mut dyn Host, args: &serde_json::Value, limits: Limits) -> Result<Outcome, RuntimeError> {
    let entry = program.entry.clone();
    let obj = args
        .as_object()
        .ok_or_else(|| RuntimeError::new("/args", "invalid_args", "args must be a JSON object"))?;
    if let Some(k) = obj.keys().find(|k| !entry.params.iter().any(|p| &p.name == *k)) {
        return Err(RuntimeError::new("/args", "invalid_args", format!("unexpected argument \"{k}\"")));
    }
    let mut scope = HashMap::new();
    for p in &entry.params {
        let raw = obj.get(&p.name).map(Value::from_json).unwrap_or(Value::Null);
        let v = conform(&raw, &p.ty, &format!("/args/{}", p.name))
            .map_err(|m| RuntimeError::new("/args", "invalid_args", m))?;
        scope.insert(p.name.clone(), v);
    }

    let mut it = Interp { program, host, fuel: limits.fuel, host_calls: 0, depth: 0, limits };
    let value = it.call_module(entry, scope, "")?;
    Ok(Outcome { value, fuel_used: it.limits.fuel - it.fuel, host_calls: it.host_calls })
}

struct Interp<'a> {
    program: &'a Program,
    host: &'a mut dyn Host,
    limits: Limits,
    fuel: u64,
    host_calls: u32,
    depth: usize,
}

impl Interp<'_> {
    fn tick(&mut self, at: &str) -> Result<(), RuntimeError> {
        if self.fuel == 0 {
            return Err(RuntimeError::new(at, "out_of_fuel", format!("script exceeded its fuel budget of {}", self.limits.fuel)));
        }
        self.fuel -= 1;
        Ok(())
    }

    fn check_size(&self, v: &Value, at: &str) -> Result<(), RuntimeError> {
        let size = match v {
            Value::List(xs) => xs.len(),
            Value::Str(s) => s.len(),
            Value::Bytes(b) => b.len(),
            _ => 0,
        };
        if size > self.limits.max_size {
            return Err(RuntimeError::new(at, "too_large", format!("value of size {size} exceeds the limit of {}", self.limits.max_size)));
        }
        Ok(())
    }

    fn call_module(&mut self, module: Rc<Module>, params: HashMap<String, Value>, at: &str) -> Result<Value, RuntimeError> {
        self.depth += 1;
        if self.depth > self.limits.max_depth {
            return Err(RuntimeError::new(at, "stack_overflow", format!("call depth exceeded {}", self.limits.max_depth)));
        }
        let mut frame = Frame { module: module.clone(), scopes: vec![params] };
        let flow = self.exec_block(&mut frame, &module.body)?;
        self.depth -= 1;
        Ok(match flow {
            Flow::Return(v) => v,
            Flow::Normal => Value::Null,
        })
    }

    fn call_closure(&mut self, c: &Closure, args: Vec<Value>, at: &str) -> Result<Value, RuntimeError> {
        self.depth += 1;
        if self.depth > self.limits.max_depth {
            return Err(RuntimeError::new(at, "stack_overflow", format!("call depth exceeded {}", self.limits.max_depth)));
        }
        let params: HashMap<String, Value> = c.def.params.iter().map(|(n, _)| n.clone()).zip(args).collect();
        let mut frame = Frame { module: c.module.clone(), scopes: vec![c.env.clone(), params] };
        let flow = self.exec_block(&mut frame, &c.def.body)?;
        self.depth -= 1;
        Ok(match flow {
            Flow::Return(v) => v,
            Flow::Normal => Value::Null,
        })
    }

    fn exec_block(&mut self, f: &mut Frame, block: &[Stmt]) -> Result<Flow, RuntimeError> {
        f.scopes.push(HashMap::new());
        for s in block {
            if let Flow::Return(v) = self.exec_stmt(f, s)? {
                f.scopes.pop();
                return Ok(Flow::Return(v));
            }
        }
        f.scopes.pop();
        Ok(Flow::Normal)
    }

    fn exec_stmt(&mut self, f: &mut Frame, s: &Stmt) -> Result<Flow, RuntimeError> {
        self.tick(&s.at)?;
        match &s.kind {
            StmtKind::Let { name, value, .. } => {
                let v = self.eval(f, value)?;
                f.scopes.last_mut().expect("block scope").insert(name.clone(), v);
            }
            StmtKind::Set { name, value } => {
                let v = self.eval(f, value)?;
                match f.scopes.iter_mut().rev().find_map(|sc| sc.get_mut(name.as_str())) {
                    Some(slot) => *slot = v,
                    None => return Err(RuntimeError::internal(&s.at, "set of unknown variable")),
                }
            }
            StmtKind::If { cond, then, els } => {
                let branch = if self.eval_bool(f, cond)? { then } else { els };
                return self.exec_block(f, branch);
            }
            StmtKind::While { cond, body } => {
                while self.eval_bool(f, cond)? {
                    if let Flow::Return(v) = self.exec_block(f, body)? {
                        return Ok(Flow::Return(v));
                    }
                    self.tick(&s.at)?;
                }
            }
            StmtKind::Return(e) => return Ok(Flow::Return(self.eval(f, e)?)),
            StmtKind::Do(e) => {
                self.eval(f, e)?;
            }
        }
        Ok(Flow::Normal)
    }

    fn eval_bool(&mut self, f: &mut Frame, e: &Expr) -> Result<bool, RuntimeError> {
        match self.eval(f, e)? {
            Value::Bool(b) => Ok(b),
            _ => Err(RuntimeError::internal(&e.at, "condition is not a bool")),
        }
    }

    fn eval(&mut self, f: &mut Frame, e: &Expr) -> Result<Value, RuntimeError> {
        self.tick(&e.at)?;
        let v = match &e.kind {
            ExprKind::Null => Value::Null,
            ExprKind::Bool(b) => Value::Bool(*b),
            ExprKind::Int(i) => Value::Int(*i),
            ExprKind::Float(x) => Value::Float(*x),
            ExprKind::Str(s) => Value::str(s),
            ExprKind::List(xs) => {
                let mut out = Vec::with_capacity(xs.len());
                for x in xs {
                    out.push(self.eval(f, x)?);
                }
                Value::List(Rc::new(out))
            }
            ExprKind::Rec(fields) => {
                let mut out = BTreeMap::new();
                for (k, x) in fields {
                    out.insert(k.clone(), self.eval(f, x)?);
                }
                Value::Rec(Rc::new(out))
            }
            ExprKind::Ref(name) => f.lookup(name).cloned().ok_or_else(|| RuntimeError::internal(&e.at, "unbound variable"))?,
            ExprKind::Call { callee, args } => self.call(f, callee, args, &e.at)?,
            ExprKind::Get { target, at } => {
                let t = self.eval(f, target)?;
                match (&t, at) {
                    (Value::Rec(m), Key::Field(k)) => m
                        .get(k)
                        .cloned()
                        .ok_or_else(|| RuntimeError::new(&e.at, "missing_key", format!("no key \"{k}\"")))?,
                    (Value::List(xs), Key::Index(i)) => {
                        // Negative indexes count from the end.
                        let idx = if *i < 0 { xs.len() as i64 + i } else { *i };
                        usize::try_from(idx).ok().and_then(|n| xs.get(n)).cloned().ok_or_else(|| {
                            RuntimeError::new(&e.at, "index_out_of_bounds", format!("index {i} is out of bounds for a list of length {}", xs.len()))
                        })?
                    }
                    _ => return Err(RuntimeError::internal(&e.at, "invalid get")),
                }
            }
            ExprKind::And(xs) => {
                for x in xs {
                    if !self.eval_bool(f, x)? {
                        return Ok(Value::Bool(false));
                    }
                }
                Value::Bool(true)
            }
            ExprKind::Or(xs) => {
                for x in xs {
                    if self.eval_bool(f, x)? {
                        return Ok(Value::Bool(true));
                    }
                }
                Value::Bool(false)
            }
            ExprKind::Fn(def) => {
                let env = f.scopes.iter().flat_map(|s| s.iter()).map(|(k, v)| (k.clone(), v.clone())).collect();
                Value::Fn(Rc::new(Closure { def: def.clone(), env, module: f.module.clone() }))
            }
            ExprKind::Decode { value, ty } => {
                let v = self.eval(f, value)?;
                conform(&v, ty, "").map_err(|m| RuntimeError::new(&e.at, "decode_failed", m))?
            }
        };
        self.check_size(&v, &e.at)?;
        Ok(v)
    }

    // ---------- calls ----------

    fn call(&mut self, f: &mut Frame, callee: &str, args: &Args, at: &str) -> Result<Value, RuntimeError> {
        let Args::Positional(xs) = args else {
            return Err(RuntimeError::internal(at, "named args survived checking"));
        };
        let mut vals = Vec::with_capacity(xs.len());
        for x in xs {
            vals.push(self.eval(f, x)?);
        }

        // Same resolution order as the checker: locals, imports, built-ins.
        if let Some(v) = f.lookup(callee) {
            let Value::Fn(c) = v.clone() else {
                return Err(RuntimeError::internal(at, "call of non-function"));
            };
            return self.call_closure(&c, vals, at);
        }
        if let Some((_, path)) = f.module.imports.iter().find(|(alias, _)| alias == callee) {
            let path = path.clone();
            return self.call_import(&path, vals, at);
        }
        stdlib::call(callee, &vals).map_err(|m| RuntimeError::new(at, "builtin_error", m))
    }

    fn call_import(&mut self, path: &str, vals: Vec<Value>, at: &str) -> Result<Value, RuntimeError> {
        let module = self.program.modules.get(path).cloned();
        let (names, ret_ty): (Vec<String>, Type) = match &module {
            Some(m) => (m.params.iter().map(|p| p.name.clone()).collect(), m.returns.clone()),
            None => {
                let sig = self.host.caps().iter()
                    .find(|c| c.path == path).map(|c| c.signature.clone())
                    .ok_or_else(|| RuntimeError::internal(at, "capability vanished"))?;
                (sig.params.into_iter().map(|(n, _)| n).collect(), *sig.ret)
            }
        };
        let named: Vec<(String, Value)> = names.into_iter().zip(vals).collect();

        // Anything coming back from outside the script is untrusted: re-check its shape.
        let checked = |r: Result<Value, String>| -> Result<Value, RuntimeError> {
            let v = r.map_err(|m| RuntimeError::new(at, "host_error", m))?;
            conform(&v, &ret_ty, "").map_err(|m| RuntimeError::new(at, "host_contract", format!("{path} returned the wrong shape: {m}")))
        };

        if let Some(r) = self.host.intercept(path, &named) {
            return checked(r);
        }
        if let Some(m) = module {
            return self.call_module(m, named.into_iter().collect(), at);
        }
        self.host_calls += 1;
        if self.host_calls > self.limits.max_host_calls {
            return Err(RuntimeError::new(at, "too_many_host_calls", format!("more than {} host calls", self.limits.max_host_calls)));
        }
        checked(self.host.call(path, named))
    }
}
