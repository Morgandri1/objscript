use crate::types::value::Value;
use std::collections::HashMap;
use std::rc::Rc;
use crate::types::ast::Module;

/// Resource budgets. `None` means unlimited. `max_depth` is always finite:
/// the interpreter recurses on the native stack, so it is the one limit that
/// can't be lifted.
#[derive(Clone, Debug)]
pub struct Limits {
    /// One unit per statement, expression and loop iteration.
    pub fuel: Option<u64>,
    /// Max nested calls (lambdas + modules).
    pub max_depth: usize,
    pub max_host_calls: Option<u32>,
    /// Max items in any list and bytes in any string or bytes value.
    pub max_size: Option<usize>,
}

impl Default for Limits {
    /// Conservative defaults for running untrusted scripts inside a host.
    fn default() -> Self {
        Self { fuel: Some(100_000), max_depth: 64, max_host_calls: Some(16), max_size: Some(64 * 1024) }
    }
}

impl Limits {
    /// Everything lifted except call depth. For trusted scripts.
    pub fn unlimited() -> Self {
        Self { fuel: None, max_depth: 64, max_host_calls: None, max_size: None }
    }
}

#[derive(Debug)]
pub struct Outcome {
    pub value: Value,
    pub fuel_used: u64,
    pub host_calls: u32,
}

pub enum Flow {
    Normal,
    Return(Value),
}

pub struct Frame {
    /// Module whose imports calls in this frame resolve against.
    pub module: Rc<Module>,
    pub scopes: Vec<HashMap<String, Value>>,
}

impl Frame {
    pub fn lookup(&self, name: &str) -> Option<&Value> {
        self.scopes.iter().rev().find_map(|s| s.get(name))
    }
}