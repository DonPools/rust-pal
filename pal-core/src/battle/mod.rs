//! Deterministic, platform-independent Classic battle state and combat rules.

mod command;
mod construction;
mod effects;
mod helpers;
mod resolution;
mod scripts;
mod types;

#[cfg(test)]
mod tests;

pub use types::*;
