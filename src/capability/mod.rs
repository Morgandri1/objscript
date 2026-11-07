use std::collections::HashMap;

use crate::types::{FnType};

pub mod http;

/// A capability's contract, independent of how any host implements it.
#[derive(Clone, Debug)]
pub struct Capability {
    pub path: String,
    pub description: String,
    pub signature: FnType,
}

impl Capability {
    pub fn new(
        path: impl Into<String>,
        description: impl Into<String>,
        signature: FnType
    ) -> Self {
        Self { path: path.into(), description: description.into(), signature }
    }
}

impl PartialEq for Capability {
    /// checks if path & sig are equal
    /// ignores description because it 
    /// may be ommitted or zero
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path && 
        self.signature == other.signature
    }
}

/// path -> signature, for hosts that only need to answer `signature()`.
pub fn signatures(caps: &[Capability]) -> HashMap<String, FnType> {
    caps.iter().map(|c| (c.path.clone(), c.signature.clone())).collect()
}