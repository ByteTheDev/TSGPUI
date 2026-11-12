//! The tsgpu renderer: our own GPU renderer for the compiled document IR.
//! See RENDERER_TSGPU.md for the architecture, module ownership and milestones.

pub mod types;
pub mod surface;
pub mod paint;
pub mod layout;
pub mod text;
pub mod backend;
pub mod input;
