//! WASM bindings. Everything crosses the boundary as JSON strings.
//! Async host calls use replay: `step` runs until it hits a host call with no
//! recorded result, then returns it as `suspend`. JS performs the call, appends
//! the result to `replay`, and calls `step` again. Deterministic because scripts
//! can only touch the world through host calls.

use std::collections::HashMap;
use objscript::parse::{parse_module, parse_type};
use objscript::types::FnType;
use objscript::capability::Capability;
use objscript::{Diagnostic, Host, Limits, Program, Value, check_program, run};
use serde_json::{json, Value as J};
use wasm_bindgen::prelude::*;

const SUSPEND: &str = "__objscript_suspend__";

#[wasm_bindgen]
pub struct Compiled {
    program: Program,
    caps: Vec<Capability>,
}

/// Parse + check a script. Throws an Error whose message is `{"ok":false,"errors":[...]}`.
/// `deps`: `{ "@org/pkg/name@1": <module json>, ... }`
/// `capabilities`: `{ "org/area/name": { "params": {...}, "returns": T, "description"?: "..." }, ... }`.
/// These are the only capabilities the script may import; param order is call order.
#[wasm_bindgen]
pub fn compile(script: &str, deps: &str, capabilities: &str) -> Result<Compiled, JsError> {
    let caps = parse_caps(&parse_json(capabilities, "capabilities")?).map_err(|d| err_json(vec![d.to_json()]))?;
    let entry = parse_module(&parse_json(script, "script")?).map_err(|d| err_json(vec![d.to_json()]))?;
    let mut modules = HashMap::new();
    if let J::Object(deps) = parse_json(deps, "deps")? {
        for (path, src) in deps {
            let m = parse_module(&src).map_err(|mut d| {
                d.path = format!("{path}#{}", d.path);
                err_json(vec![d.to_json()])
            })?;
            modules.insert(path, m);
        }
    }

    let program = {
        let host = ReplayHost::new(Vec::new(), caps.clone());
        check_program(entry, modules, &host).map_err(|ds| err_json(ds.iter().map(Diagnostic::to_json).collect()))?
    };
    Ok(Compiled { program, caps })
}

#[wasm_bindgen]
impl Compiled {
    /// Returns `{"type":"done", ...result}` or `{"type":"suspend","path":..,"args":{..}}`.
    /// `replay`: `[{ "path": ..., "ok": value } | { "path": ..., "err": message }, ...]`
    pub fn step(&self, args: &str, limits: &str, replay: &str) -> Result<String, JsError> {
        let args = parse_json(args, "args")?;
        let limits = parse_limits(&parse_json(limits, "limits")?);
        let replay = match parse_json(replay, "replay")? {
            J::Array(xs) => xs,
            _ => Vec::new(),
        };

        let mut host = ReplayHost::new(replay, self.caps.clone());
        let out = match run(&self.program, &mut host, &args, limits) {
            Ok(o) => json!({
                "type": "done", "ok": true, "value": o.value.to_json(),
                "fuel_used": o.fuel_used, "host_calls": o.host_calls
            }),
            Err(_) if host.pending.is_some() => {
                let (path, args) = host.pending.take().expect("checked");
                json!({ "type": "suspend", "path": path, "args": args })
            }
            Err(e) => json!({ "type": "done", "ok": false, "error": e.to_json() }),
        };
        Ok(out.to_string())
    }
}

struct ReplayHost {
    replay: Vec<J>,
    next: usize,
    pending: Option<(String, J)>,
    caps: Vec<Capability>,
}

impl ReplayHost {
    fn new(replay: Vec<J>, caps: Vec<Capability>) -> Self {
        Self { replay, next: 0, pending: None, caps }
    }
}

fn parse_caps(v: &J) -> Result<Vec<Capability>, Diagnostic> {
    let J::Object(map) = v else {
        return Err(Diagnostic::new("", "invalid_capabilities", "capabilities must be an object keyed by path"));
    };
    let mut caps = Vec::new();
    for (path, spec) in map {
        let at = format!("capabilities/{path}");
        let mut params = Vec::new();
        if let Some(J::Object(ps)) = spec.get("params") {
            for (name, ty) in ps {
                params.push((name.clone(), parse_type(ty, &format!("{at}/params/{name}"))?));
            }
        }
        let ret = parse_type(spec.get("returns").unwrap_or(&J::Null), &format!("{at}/returns"))?;
        let description = spec.get("description").and_then(J::as_str).unwrap_or_default();
        caps.push(Capability::new(path.clone(), description, FnType { params, ret: Box::new(ret) }));
    }
    Ok(caps)
}

impl Host for ReplayHost {
    fn call(&mut self, path: &str, args: Vec<(String, Value)>) -> Result<Value, String> {
        if let Some(r) = self.replay.get(self.next) {
            self.next += 1;
            if r.get("path").and_then(J::as_str) != Some(path) {
                return Err(format!("replay mismatch: expected a call to {path}"));
            }
            return match r.get("err") {
                Some(e) => Err(e.as_str().unwrap_or("host error").to_string()),
                None => Ok(Value::from_json(r.get("ok").unwrap_or(&J::Null))),
            };
        }
        let args = J::Object(args.into_iter().map(|(k, v)| (k, v.to_json())).collect());
        self.pending = Some((path.to_string(), args));
        Err(SUSPEND.into())
    }

    fn intercept(&mut self, _path: &str, _args: &[(String, Value)]) -> Option<Result<Value, String>> {
        None
    }
    
    fn caps(&self) -> Vec<Capability> {
        self.caps.clone()
    }
}

fn parse_limits(v: &J) -> Limits {
    let mut l = Limits::default();
    // Absent = default; `null` = unlimited; a number = that limit.
    let get = |k: &str| v.get(k).map(J::as_u64);
    if let Some(x) = get("fuel") { l.fuel = x; }
    if let Some(Some(x)) = get("max_depth") { l.max_depth = x as usize; }
    if let Some(x) = get("max_host_calls") { l.max_host_calls = x.map(|x| x as u32); }
    if let Some(x) = get("max_size") { l.max_size = x.map(|x| x as usize); }
    l
}

fn parse_json(s: &str, what: &str) -> Result<J, JsError> {
    serde_json::from_str(s).map_err(|e| err_one("invalid_json", format!("{what}: {e}")))
}

fn err_json(errors: Vec<J>) -> JsError {
    JsError::new(&json!({ "ok": false, "errors": errors }).to_string())
}

fn err_one(code: &'static str, msg: impl Into<String>) -> JsError {
    err_json(vec![Diagnostic::new("", code, msg).to_json()])
}

#[wasm_bindgen]
pub fn builtins() -> String {
    let list: Vec<J> = objscript::stdlib::BUILTINS
        .iter()
        .map(|(name, sig, desc)| json!({ "name": name, "signature": sig, "description": desc }))
        .collect();
    J::Array(list).to_string()
}