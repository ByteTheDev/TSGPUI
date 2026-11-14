//! Rendrix CSS engine.
//!
//! Pipeline: [`parser`] (text → rules) → [`cascade`] (rules + element → computed style, via the
//! [`selector::SelectorElement`] trait so any tree can be styled) → consumers read
//! [`style::ComputedStyle`]. [`animation`] holds timing, interpolation and transform math.

pub mod animation;
pub mod cascade;
pub mod color;
pub mod parser;
pub mod properties;
pub mod selector;
pub mod style;
pub mod ua;
pub mod values;

pub use cascade::{compute_style, Origin, RuleSet};
pub use color::Color;
pub use parser::{parse_declarations, Declaration, MediaContext, Stylesheet};
pub use properties::{apply_declaration, StyleContext};
pub use selector::{ElementState, PseudoElement, Selector, SelectorElement};
pub use style::*;
pub use values::{Dim, LengthContext};
