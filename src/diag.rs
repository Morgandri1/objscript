use serde_json::json;

/// A structured error meant to be read by agents as well as humans.
#[derive(Clone, Debug)]
pub struct Diagnostic {
    /// JSON Pointer to the offending node, e.g. `/body/0/value/args/zipCode`.
    pub path: String,
    /// Stable machine-readable code, e.g. `type_mismatch`.
    pub code: &'static str,
    pub message: String,
    pub hint: Option<String>,
}

impl Diagnostic {
    pub fn new(path: impl Into<String>, code: &'static str, message: impl Into<String>) -> Self {
        Self { path: path.into(), code, message: message.into(), hint: None }
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn to_json(&self) -> serde_json::Value {
        let mut v = json!({ "path": self.path, "code": self.code, "message": self.message });
        if let Some(h) = &self.hint {
            v["hint"] = json!(h);
        }
        v
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} [{}] {}", self.path, self.code, self.message)?;
        if let Some(h) = &self.hint {
            write!(f, " (hint: {h})")?;
        }
        Ok(())
    }
}

/// Append a key or index to a JSON Pointer, escaping per RFC 6901.
pub fn join(path: &str, seg: impl std::fmt::Display) -> String {
    let seg = seg.to_string().replace('~', "~0").replace('/', "~1");
    format!("{path}/{seg}")
}
