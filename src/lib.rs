//! ObjScript proof of concept.
//!
//! Pipeline: JSON -> [`parse`] -> AST -> [`check`] -> [`check::Program`] -> [`interp`].
//! `Program` can only be built by the checker, so the interpreter never sees
//! an ill-typed script.

pub mod check;
pub mod diag;
pub mod host;
pub mod interp;
pub mod parse;
pub mod stdlib;
pub mod types;
#[cfg(test)]
pub mod test;
pub mod capability;

pub use check::{check_program, Program};
pub use diag::Diagnostic;
pub use host::Host;
pub use interp::{run, Limits, Outcome, RuntimeError};
pub use types::value::Value;
pub use stdlib::{ string_pairs, load, print, compile };