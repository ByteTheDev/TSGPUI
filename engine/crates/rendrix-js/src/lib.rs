//! Rendrix JavaScript runtime: an engine-neutral host interface, a QuickJS backend, the DOM and
//! Web API prelude, and the event-loop glue the platform layer drives.

pub mod engine_trait;
pub mod host;
pub mod quickjs;
pub mod runtime;

pub use engine_trait::{Host, JsEngine, JsError, Val};
pub use host::{DomHost, Permissions, WindowCommand};
pub use runtime::{JsRuntime, TickInfo};

#[cfg(test)]
mod tests;
