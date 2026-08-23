//! Verify the crusher's own output: it must parse again, to exactly the
//! tree the crusher meant to print, and every identifier in it must bind
//! the way the original did.

use super::protect::Protection;
use super::scope::{self, SymbolTable};
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

fn reparse(output: &str) -> Result<TranslationUnit, CrushError> {
	let (tu, rest) =
		parse_translation_unit_with_rest(output).map_err(|e| CrushError::Reparse(e.info))?;
	if !rest.trim().is_empty() {
		let rest: String = rest.trim_start().chars().take(60).collect();
		return Err(CrushError::Reparse(format!(
			"unparsed trailing output: {:?}",
			rest
		)));
	}
	Ok(tu)
}

fn ast_equals(tu: &TranslationUnit, expected: &TranslationUnit) -> Result<(), CrushError> {
	if tu == expected {
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

/// Re-parse `output` and compare it with `expected`.
#[cfg(test)]
pub fn reparse_equals(output: &str, expected: &TranslationUnit) -> Result<(), CrushError> {
	ast_equals(&reparse(output)?, expected)
}

/// Resolve `tu` afresh and check that its binding structure is the one in
/// `table` (same symbols, kinds, scopes and occurrence sequence) and that
/// every symbol carries the name the renamer chose.
pub fn scope_isomorphic(tu: &TranslationUnit, table: &SymbolTable) -> Result<(), CrushError> {
	let mut tu = tu.clone();
	let t2 = scope::resolve(&mut tu, &Protection::default())?;
	if t2.symbols.len() != table.symbols.len() {
		return Err(CrushError::ScopeMismatch(format!(
			"{} symbols before, {} after",
			table.symbols.len(),
			t2.symbols.len()
		)));
	}
	for (i, (a, b)) in t2.symbols.iter().zip(table.symbols.iter()).enumerate() {
		let want = b.new_name.as_deref().unwrap_or(&b.name);
		if a.kind != b.kind || a.scope != b.scope || a.name != want {
			return Err(CrushError::ScopeMismatch(format!(
				"symbol {}: {:?} {} (scope {}) became {:?} {} (scope {})",
				i, b.kind, want, b.scope, a.kind, a.name, a.scope
			)));
		}
	}
	if t2.occ != table.occ {
		let i = t2
			.occ
			.iter()
			.zip(table.occ.iter())
			.position(|(a, b)| a != b)
			.unwrap_or(t2.occ.len().min(table.occ.len()));
		let describe = |t: &SymbolTable, i: usize| match t.occ.get(i) {
			Some(o) => format!(
				"{} {}",
				if o.is_decl {
					"declaration of"
				} else {
					"use of"
				},
				t.symbols[o.sym as usize].name
			),
			None => "end".to_string(),
		};
		return Err(CrushError::ScopeMismatch(format!(
			"occurrence {}: expected {}, found {}",
			i,
			describe(table, i),
			describe(&t2, i)
		)));
	}
	Ok(())
}

/// The full self-check: re-parse, compare the tree, compare the binding.
pub fn run(
	output: &str,
	expected: &TranslationUnit,
	table: &SymbolTable,
) -> Result<(), CrushError> {
	let tu = reparse(output)?;
	ast_equals(&tu, expected)?;
	scope_isomorphic(&tu, table)
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

	#[test]
	fn detects_rebinding() {
		// renaming the local to `a` would rebind the later use of the global
		let mut tu =
			TranslationUnit::parse("float a; void main() { float b = 1.; b = a; }").unwrap();
		let mut table = scope::resolve(&mut tu, &Protection::default()).unwrap();
		table.symbols[2].new_name = Some("a".to_string());
		let bad = TranslationUnit::parse("float a; void main() { float a = 1.; a = a; }").unwrap();
		assert!(matches!(
			scope_isomorphic(&bad, &table),
			Err(CrushError::ScopeMismatch(_))
		));
		table.symbols[2].new_name = Some("c".to_string());
		let good = TranslationUnit::parse("float a; void main() { float c = 1.; c = a; }").unwrap();
		assert!(scope_isomorphic(&good, &table).is_ok());
		// a name of a different kind or scope is a mismatch too
		let other =
			TranslationUnit::parse("float a; float c; void main() { c = 1.; c = a; }").unwrap();
		assert!(matches!(
			scope_isomorphic(&other, &table),
			Err(CrushError::ScopeMismatch(_))
		));
	}
}
