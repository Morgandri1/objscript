use serde_json::{json, Value as J};

use crate::capability::Capability;
use crate::types::value::Value;

#[cfg(feature = "cli")]
pub mod cli;

/// Capabilities granted by the embedding application (the `std/*` imports).
/// A script can only touch the outside world through this trait.
pub trait Host {    
    /// Perform a host call. `args` are named and in signature order.
    fn call(&mut self, path: &str, args: Vec<(String, Value)>) -> Result<Value, String>;

    /// Optionally intercept any import call (host or module) before it runs.
    /// Used by test runners for mocks; real hosts shouldn't need.
    fn intercept(&mut self, _path: &str, _args: &[(String, Value)]) -> Option<Result<Value, String>> {
        None
    }

    /// Output capabilities offered by host
    fn caps(&self) -> Vec<Capability>;
}

pub fn catalog(host: &dyn Host) -> J {
    let builtins: Vec<J> = crate::stdlib::BUILTINS
        .iter()
        .map(|(name, sig, desc)| json!({ "name": name, "signature": sig, "description": desc }))
        .collect();
    let caps: Vec<J> = host.caps()
        .iter()
        .filter_map(|c| {
            let s = &c.signature;
            let params: serde_json::Map<String, J> = s.params.iter().map(|(n, t)| (n.clone(), json!(t.to_string()))).collect();
            Some(json!({ "path": &c.path, "params": params, "returns": s.ret.to_string(), "description": &c.description }))
        })
        .collect();
    json!({ "objscript": "0.2", "builtins": builtins, "host": caps })
}
