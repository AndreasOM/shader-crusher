use std::fmt;

/// Why crushing failed. The output of a failed crush is the unchanged input
/// (passthrough); callers decide whether that is acceptable via `exit_code`.
#[derive(Debug, Clone, PartialEq)]
pub enum CrushError {
	/// The parser rejected the input.
	Parse(String),
	/// The parser accepted a prefix of the input and silently stopped.
	/// `consumed` is the byte offset where parsing stopped, `rest` the
	/// beginning of the unparsed text.
	PartialParse { consumed: usize, rest: String },
	/// The crushed output does not parse again.
	Reparse(String),
	/// The crushed output parses to a different AST than intended.
	AstMismatch(String),
	/// Re-resolving the crushed output binds identifiers differently.
	ScopeMismatch(String),
	/// The renamer gave one name to two symbols that must not share one.
	NameCollision(String),
	/// Valid GLSL the crusher does not handle yet.
	Unsupported(String),
	/// More symbols than the sentinel encoding can address.
	TooManySymbols(usize),
}

impl CrushError {
	/// Process exit code for the CLI: 1 input problem, 2 self-check failure,
	/// 3 unsupported input.
	pub fn exit_code(&self) -> i32 {
		match self {
			CrushError::Parse(_) | CrushError::PartialParse { .. } => 1,
			CrushError::Reparse(_)
			| CrushError::AstMismatch(_)
			| CrushError::ScopeMismatch(_)
			| CrushError::NameCollision(_) => 2,
			CrushError::Unsupported(_) | CrushError::TooManySymbols(_) => 3,
		}
	}
}

impl fmt::Display for CrushError {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		match self {
			CrushError::Parse(info) => write!(f, "parse error: {}", info.trim_end()),
			CrushError::PartialParse { consumed, rest } => write!(
				f,
				"parse error: unparsed input at byte {} (shader would have been truncated): {:?}",
				consumed, rest
			),
			CrushError::Reparse(info) => {
				write!(f, "self-check: output does not parse: {}", info.trim_end())
			},
			CrushError::AstMismatch(info) => {
				write!(f, "self-check: output parses differently: {}", info)
			},
			CrushError::ScopeMismatch(info) => {
				write!(f, "self-check: identifiers resolve differently: {}", info)
			},
			CrushError::NameCollision(info) => {
				write!(f, "self-check: renaming collision: {}", info)
			},
			CrushError::Unsupported(info) => write!(f, "unsupported: {}", info),
			CrushError::TooManySymbols(n) => write!(f, "too many symbols ({})", n),
		}
	}
}

impl std::error::Error for CrushError {}
