//! Elysium's kernel modules, exposed as a library so headless consumers —
//! `kernel/main.rs`'s binary, `cargo test`, and `benches/` — can all link
//! against the same code instead of the binary being the only entry point.

pub mod esm_resolver;
pub mod framebuffer;
pub mod image;
pub mod input;
pub mod runtime;
pub mod timers;
pub mod transform;
pub mod window;
