pub mod glsl;
mod shader_crusher;

pub use crate::shader_crusher::{crush_str, CrushError, Options, Scoring, ShaderCrusher, Stats};
