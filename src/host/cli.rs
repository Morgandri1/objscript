use serde_json::{json, Value as J};

#[cfg(feature = "cli")]
use crate::capability::Capability;
use crate::string_pairs;
use crate::types::Type;
#[cfg(feature = "cli")]
use crate::types::str_map;
use crate::types::value::Value;
use crate::parse::parse_sig;
use super::Host;

/// Takes scope as cli flags for validation and local use
#[cfg(feature = "cli")]
pub struct CliHost;

#[cfg(feature = "cli")]
impl Host for CliHost {
    fn call(&mut self, path: &str, args: Vec<(String, Value)>) -> Result<Value, String> {
        match path {
            "host/secret" => {
                let name = args[0].1.render();
                std::env::var(&name).map(Value::str).map_err(|_| format!("secret {name} is not set"))
            }
            "host/http/fetch" => {
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

    fn caps(&self) -> Vec<Capability> {
        vec![
            Capability::new(
                "host/secret", 
                "Read a named secret from the env.", 
                parse_sig(&[("name", Type::Str)], Type::Str)
            ),
            Capability::new(
                "host/http/fetch",
                "HTTP call",
                parse_sig(
                    &[("url", Type::Str), ("headers", str_map()), ("query", str_map())],
                    Type::Rec([("status".to_string(), Type::Int), ("body".to_string(), Type::Json)].into_iter().collect()),
                )
            )
        ]
    }
}