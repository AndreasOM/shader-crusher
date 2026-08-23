//! GLSL parser, AST and visitor — vendored from the `glsl` crate 7.0.0
//! (https://github.com/phaazon/glsl), BSD-3-Clause, Copyright (c) 2018
//! Dimitri Sabadie. See `LICENSE-glsl` at the repository root.
//!
//! Vendored (instead of depended upon) because upstream is unmaintained and
//! shader-crusher needs parser fixes that must also reach crates.io users;
//! a `path` dependency would be rewritten to the registry crate on publish.
//! The vendored files keep upstream formatting (`#[rustfmt::skip]`).

#[cfg(test)]
#[rustfmt::skip]
mod parse_tests;
#[rustfmt::skip]
pub mod parser;
#[rustfmt::skip]
#[allow(mismatched_lifetime_syntaxes)]
mod parsers;
#[rustfmt::skip]
pub mod syntax;
#[rustfmt::skip]
pub mod visitor;
