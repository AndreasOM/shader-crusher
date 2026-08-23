/// How a new name is chosen for a renamable symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scoring {
	/// Next unused name in frequency order.
	Frequency,
	/// Shader-Minifier style: maximise the frequency of the character
	/// bigrams the new name forms with its distinct neighbours.
	Bigram,
	/// Like `Bigram`, but each neighbour is weighted by how often it occurs.
	/// Measured best after compression on intro-sized shaders; the default.
	BigramCount,
}

impl Scoring {
	pub fn parse(s: &str) -> Option<Scoring> {
		match s {
			"freq" | "frequency" => Some(Scoring::Frequency),
			"bigram" => Some(Scoring::Bigram),
			"count" | "bigram-count" => Some(Scoring::BigramCount),
			_ => None,
		}
	}
}

pub use super::simplify::Flags as Rewrites;

#[derive(Debug, Clone, PartialEq)]
pub struct Options {
	/// Identifiers that are never renamed (in addition to keywords, builtins
	/// and everything protected by `#pragma SHADER_CRUSHER_OFF`).
	pub blocklist: Vec<String>,
	/// Per-identifier diagnostics on stderr.
	pub verbose:   bool,
	/// Rename identifiers at all.
	pub rename:    bool,
	/// Apply AST-level rewrites (`(void)` → `()`, declaration merging, ...).
	pub simplify:  bool,
	/// Which rewrites `simplify` applies.
	pub rewrites:  Rewrites,
	/// Let a local reuse the name of an outer symbol that is not referenced
	/// inside its scope (spec-legal in every GLSL version). Off = locals
	/// never reuse names of globals, functions or types.
	pub shadowing: bool,
	pub scoring:   Scoring,
	/// Re-parse the output and verify it is the intended AST with the
	/// intended identifier binding.
	pub selfcheck: bool,
}

impl Default for Options {
	fn default() -> Self {
		Options {
			blocklist: Vec::new(),
			verbose:   false,
			rename:    true,
			simplify:  true,
			rewrites:  Rewrites::default(),
			shadowing: true,
			scoring:   Scoring::BigramCount,
			selfcheck: true,
		}
	}
}
