use super::Capability;
use crate::types::{FnType, Type, str_map};

pub fn http_fetch() -> Capability {
    Capability {
        path: "host/http/fetch",
        description: "HTTP fetch. Returns the status and the body (parsed as JSON when possible, otherwise a string).",
        signature: FnType {
            params: vec![("url".into(), Type::Str), ("headers".into(), str_map()), ("query".into(), str_map())],
            ret: Box::new(Type::Rec(
                [("status".to_string(), Type::Int), ("body".to_string(), Type::Json)].into_iter().collect(),
            )),
        },
    }
}