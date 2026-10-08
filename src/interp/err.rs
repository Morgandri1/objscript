use serde_json::json;

#[derive(Clone, Debug)]
pub struct RuntimeError {
    pub path: String,
    pub code: &'static str,
    pub message: String,
}

impl RuntimeError {
    pub fn new(path: impl Into<String>, code: &'static str, message: impl Into<String>) -> Self {
        Self { path: path.into(), code, message: message.into() }
    }

    pub fn internal(path: &str, what: &str) -> Self {
        Self::new(path, "internal", format!("checker invariant violated: {what}"))
    }

    pub fn to_json(&self) -> serde_json::Value {
        json!({ "path": self.path, "code": self.code, "message": self.message })
    }
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} [{}] {}", self.path, self.code, self.message)
    }
}