//! Rendrix runtime: loads an application bundle, owns the engine and JavaScript runtime, and drives
//! them from a native window (or headlessly).

pub mod bench;
pub mod bundle;
pub mod headless;
pub mod session;
pub mod window;

pub use bundle::AppBundle;
pub use session::Session;
