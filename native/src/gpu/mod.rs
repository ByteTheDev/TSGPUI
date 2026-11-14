//! The rendrix renderer: our own GPU renderer for the compiled document IR.
//! See RENDERER_RENDRIX.md for the architecture, module ownership and milestones.

pub mod types;
pub mod surface;
pub mod paint;
pub mod layout;
pub mod text;
pub mod backend;
pub mod input;
