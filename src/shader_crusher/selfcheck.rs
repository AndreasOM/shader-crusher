//! Verify the crusher's own output: it must parse again, to exactly the
//! tree the crusher meant to print.

use super::CrushError;
use crate::glsl::parser::parse_translation_unit_with_rest;
use crate::glsl::syntax::{ExternalDeclaration, TranslationUnit};

fn truncate(s: String, max: usize) -> String {
	if s.len() <= max {
		s
	} else {
		let mut end = max;
		while !s.is_char_boundary(end) {
			end -= 1;
		}
		format!("{}…", &s[..end])
	}
}

/// Re-parse `output` and compare it with `expected`.
pub fn reparse_equals(output: &str, expected: &TranslationUnit) -> Result<(), CrushError> {
	let (tu, rest) =
		parse_translation_unit_with_rest(output).map_err(|e| CrushError::Reparse(e.info))?;
	if !rest.trim().is_empty() {
		let rest: String = rest.trim_start().chars().take(60).collect();
		return Err(CrushError::Reparse(format!(
			"unparsed trailing output: {:?}",
			rest
		)));
	}
	if tu == *expected {
		return Ok(());
	}
	let got: &[ExternalDeclaration] = &tu.0 .0;
	let want: &[ExternalDeclaration] = &expected.0 .0;
	for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
		if g != w {
			return Err(CrushError::AstMismatch(format!(
				"external declaration {}: printed {} reads back as {}",
				i,
				truncate(format!("{:?}", w), 400),
				truncate(format!("{:?}", g), 400)
			)));
		}
	}
	Err(CrushError::AstMismatch(format!(
		"{} external declarations printed, {} read back",
		want.len(),
		got.len()
	)))
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::glsl::parser::Parse;

	#[test]
	fn detects_differences() {
		let tu = TranslationUnit::parse("float a=1.;").unwrap();
		assert!(reparse_equals("float a=1.;", &tu).is_ok());
		assert!(matches!(
			reparse_equals("float a=2.;", &tu),
			Err(CrushError::AstMismatch(_))
		));
		assert!(matches!(
			reparse_equals("float a=1.;float b;", &tu),
			Err(CrushError::AstMismatch(_))
		));
		assert!(matches!(
			reparse_equals("float a=;", &tu),
			Err(CrushError::Reparse(_))
		));
		assert!(matches!(
			reparse_equals("float a=1.;@@", &tu),
			Err(CrushError::Reparse(_))
		));
	}
}
