//! Rendrix graphics: the display list, text shaping, and the wgpu renderer.

pub mod context;
pub mod display;
pub mod renderer;
pub mod text;

pub use context::GpuContext;
pub use display::*;
pub use renderer::Renderer;
pub use text::TextSystem;
