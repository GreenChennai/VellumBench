//! Host-independent CSS animation parsing and evaluation shared by sessions and Kiln.
//!
//! The document CSS remains authoritative. This crate projects `@keyframes` and
//! `animation` declarations into typed values without depending on a UI or renderer.

mod core;
pub mod preset;
pub mod timeline;

pub use core::*;
