mod builtins;
mod error;
mod lexer;
mod options;
mod preprocess;
mod protect;
mod shadercrusher;

pub use error::CrushError;
pub use options::{Options, Scoring};
pub use shadercrusher::{crush_str, ShaderCrusher, Stats};
