mod error;
mod options;
mod preprocess;
mod shadercrusher;

pub use error::CrushError;
pub use options::{Options, Scoring};
pub use shadercrusher::{crush_str, ShaderCrusher, Stats};
