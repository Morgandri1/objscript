use crate::types::value::Value;
use std::collections::HashMap;
use std::rc::Rc;
use crate::types::ast::Module;

#[derive(Clone, Debug)]
pub struct Limits {
    /// One unit per statement, expression and loop iteration.
    pub fuel: u64,
    /// Max nested calls (lambdas + modules).
    pub max_depth: usize,
    pub max_host_calls: u32,
    /// Max items in any list and bytes in any string.
    pub max_size: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self { fuel: 100_000, max_depth: 64, max_host_calls: 16, max_size: 64 * 1024 }
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