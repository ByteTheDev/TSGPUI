//! The boundary between Rendrix and a JavaScript engine.
//!
//! Everything above this file (host operations, Web APIs, the event loop) is engine-agnostic.
//! A new engine needs to implement [`JsEngine`] and route its single native entry point
//! (`__native(op, ...args)`) to a [`Host`]. The Web API surface itself is JavaScript (`prelude/`).

use std::fmt;

/// Plain data exchanged across the boundary.
#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    Undefined,
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    List(Vec<Val>),
}

impl Val {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Val::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Val::Num(n) => Some(*n),
            Val::Bool(b) => Some(*b as u8 as f64),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> bool {
        match self {
            Val::Bool(b) => *b,
            Val::Num(n) => *n != 0.0,
            Val::Str(s) => !s.is_empty(),
            Val::List(_) => true,
            Val::Undefined | Val::Null => false,
        }
    }
    pub fn from_opt_id(id: Option<u32>) -> Val {
        id.map(|i| Val::Num(i as f64)).unwrap_or(Val::Null)
    }
}

impl From<&str> for Val {
    fn from(s: &str) -> Val {
        Val::Str(s.to_string())
    }
}
impl From<String> for Val {
    fn from(s: String) -> Val {
        Val::Str(s)
    }
}
impl From<f64> for Val {
    fn from(n: f64) -> Val {
        Val::Num(n)
    }
}
impl From<bool> for Val {
    fn from(b: bool) -> Val {
        Val::Bool(b)
    }
}

#[derive(Debug, Clone)]
pub struct JsError {
    pub message: String,
    pub stack: Option<String>,
}

impl fmt::Display for JsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(s) = &self.stack {
            write!(f, "\n{s}")?;
        }
        Ok(())
    }
}
impl std::error::Error for JsError {}

/// Native operations callable from JavaScript through `__native(op, ...args)`.
pub trait Host {
    fn call(&mut self, op: &str, args: &[Val]) -> Result<Val, String>;
}

pub trait JsEngine {
    fn name(&self) -> &'static str;
    /// Make an ES module available under `name` (the module map key).
    fn add_module(&mut self, name: &str, source: String);
    /// Run classic script source.
    fn eval_script(&mut self, name: &str, source: &str) -> Result<(), JsError>;
    /// Evaluate the module registered under `name` and everything it imports.
    fn eval_module(&mut self, name: &str) -> Result<(), JsError>;
    /// Call a global function with plain-data arguments.
    fn call_global(&mut self, func: &str, args: Vec<Val>) -> Result<Val, JsError>;
    /// Drain the microtask (promise job) queue.
    fn run_jobs(&mut self) -> Result<(), JsError>;
    fn memory_usage(&self) -> usize;
    fn collect_garbage(&mut self);
    /// Unhandled promise rejections observed since the last call (message text).
    fn take_rejections(&mut self) -> Vec<String> {
        Vec::new()
    }
}
