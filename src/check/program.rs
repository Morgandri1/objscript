use std::rc::Rc;
use std::collections::HashMap;
use crate::types::ast::Module;

/// A fully checked program: an entry script plus the modules it (transitively) imports.
#[derive(Debug)]
pub struct Program {
    pub(crate) entry: Rc<Module>,
    /// Import path (e.g. `@morgan/tools/getWeather@3`) -> checked module.
    pub(crate) modules: HashMap<String, Rc<Module>>,
}

impl Program {
    pub fn entry(&self) -> &Module {
        &self.entry
    }
}