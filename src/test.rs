use std::collections::HashMap;

use crate::capability::Capability;
use crate::parse::parse_sig;
use crate::string_pairs;
use crate::types::{Type, str_map};
use crate::{
    Host, 
    Value,
    host::cli::CliHost
};
use serde_json::{Value as J, json};

/// Answers calls from mocks and records every call made through the entry script's imports.
struct TestHost<'a> {
    /// import path -> alias in the entry script
    aliases: &'a HashMap<String, String>,
    mocks: HashMap<String, J>,
    calls: Vec<(String, J)>,
}

impl Host for TestHost<'_> {
    fn call(&mut self, path: &str, args: Vec<(String, Value)>) -> Result<Value, String> {
        match path {
            "std/secret" => {
                let name = args[0].1.render();
                std::env::var(&name).map(Value::str).map_err(|_| format!("secret {name} is not set"))
            }
            "std/http/fetch" => {
                let [(_, url), (_, headers), (_, query)]: [(String, Value); 3] =
                    args.try_into().map_err(|_| "fetch: expected url, headers, query".to_string())?;
                let mut req = ureq::get(&url.render());
                for (k, v) in string_pairs(&headers) {
                    req = req.set(&k, &v);
                }
                for (k, v) in string_pairs(&query) {
                    req = req.query(&k, &v);
                }
                // Non-2xx statuses go back to the script instead of erroring.
                let (status, text) = match req.call() {
                    Ok(r) => (r.status(), r.into_string().map_err(|e| format!("fetch: {e}"))?),
                    Err(ureq::Error::Status(code, r)) => (code, r.into_string().unwrap_or_default()),
                    Err(e) => return Err(format!("fetch failed: {e}")),
                };
                let body = serde_json::from_str::<J>(&text).unwrap_or(J::String(text));
                Ok(Value::from_json(&json!({ "status": status, "body": body })))
            }
            _ => Err(format!("{path} is not implemented")),
        }
    }
    
    fn intercept(&mut self, path: &str, args: &[(String, Value)]) -> Option<Result<Value, String>> {
        let alias = self.aliases.get(path)?.clone();
        let args_json: serde_json::Map<String, J> = args.iter().map(|(k, v)| (k.clone(), v.to_json())).collect();
        self.calls.push((alias.clone(), J::Object(args_json)));
        match self.mocks.get(&alias) {
            Some(v) => Some(Ok(Value::from_json(v))),
            // host calls expectedly returning null succeed
            None if path.starts_with("std/") => Some(match CliHost.caps()
                .iter().find(|c| c.path == path)
                .map(|c| c.signature.clone()) 
            {
                Some(s) if *s.ret == Type::Null => Ok(Value::Null),
                _ => Err(format!("{alias} ({path}) has no mock in this test")),
            }),
            None => None,
        }
    }

    fn caps(&self) -> Vec<Capability> {
        vec![
            Capability::new(
                "std/secret", 
                "Read a named secret from the env.", 
                parse_sig(&[("name", Type::Str)], Type::Str)
            ),
            Capability::new(
                "std/http/fetch",
                "HTTP call",
                parse_sig(
                    &[("url", Type::Str), ("headers", str_map()), ("query", str_map())],
                    Type::Rec([("status".to_string(), Type::Int), ("body".to_string(), Type::Json)].into_iter().collect()),
                )
            )
        ]
    }
}

// fn main(file: &str) -> Result<ExitCode, String> {
//     let mut deps = Vec::new();
//     let src = load(file)?;
//     let program = match compile(&src, &deps) {
//         Ok(p) => p,
//         Err(diags) => {
//             print(&json!({ "ok": false, "errors": diags.iter().map(Diagnostic::to_json).collect::<Vec<_>>() }));
//             return Ok(ExitCode::FAILURE);
//         }
//     };
// }

// #[test]
// fn test_http() {
//     let r = main("../examples/fetch.json").unwrap();
//     if r != ExitCode::SUCCESS {
//         panic!("go fuck yourself")
//     }
// }