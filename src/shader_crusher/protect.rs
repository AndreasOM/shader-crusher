//! Names that must not be renamed because of things the AST walker cannot
//! see: preprocessor text, `#pragma SHADER_CRUSHER_OFF` regions, the
//! caller's blocklist, interface blocks and built-in structure members.

use std::collections::HashSet;

use super::builtins::is_builtin_member;
use super::lexer;
use super::CrushError;
use crate::glsl::syntax::*;
use crate::glsl::visitor::{Host, Visit, Visitor};

#[derive(Debug, Default, Clone)]
pub struct Protection {
	/// Identifiers (variables, functions, types) pinned file-wide.
	pub names:       HashSet<String>,
	/// Names pinned when they appear as a `.member` selector.
	pub field_names: HashSet<String>,
}

/// Collects every identifier and type name below a node.
#[derive(Default)]
struct Collect {
	names: HashSet<String>,
}

impl Visitor for Collect {
	fn visit_identifier(&mut self, i: &Identifier) -> Visit {
		self.names.insert(i.0.clone());
		Visit::Children
	}
	fn visit_type_name(&mut self, t: &TypeName) -> Visit {
		self.names.insert(t.0.clone());
		Visit::Children
	}
}

/// Collects `.member` selectors that name a member of a built-in structure.
#[derive(Default)]
struct BuiltinMembers {
	names: HashSet<String>,
}

impl Visitor for BuiltinMembers {
	fn visit_expr(&mut self, e: &Expr) -> Visit {
		if let Expr::Dot(_, Identifier(f)) = e {
			if is_builtin_member(f) {
				self.names.insert(f.clone());
			}
		}
		Visit::Children
	}
}

fn pragma_command(ed: &ExternalDeclaration) -> Option<&str> {
	match ed {
		ExternalDeclaration::Preprocessor(Preprocessor::Pragma(p)) => Some(p.command.trim()),
		_ => None,
	}
}

/// Compute the protection sets for `tu` and remove the crusher's own
/// `#pragma SHADER_CRUSHER_OFF/ON` directives from it.
///
/// - every identifier inside an OFF region (until the next ON) is pinned;
/// - macro names, the identifiers in macro bodies and in `#if`/`#elif`
///   conditions, `#ifdef`/`#ifndef`/`#undef` names are pinned (macro
///   parameters are not: they are local to the `#define` line and the
///   crusher never touches that line);
/// - interface block names and members are pinned (API-visible);
/// - built-in structure members that appear as `.member` are pinned as
///   field names.
pub fn run(tu: &mut TranslationUnit, blocklist: &[String]) -> Result<Protection, CrushError> {
	let mut p = Protection::default();
	p.names.insert("main".to_string());
	p.names.extend(blocklist.iter().cloned());

	let mut crushing = true;
	let mut keep = Vec::new();
	for ed in std::mem::take(&mut tu.0 .0) {
		match pragma_command(&ed) {
			Some("SHADER_CRUSHER_OFF") => {
				crushing = false;
				continue;
			},
			Some("SHADER_CRUSHER_ON") => {
				crushing = true;
				continue;
			},
			_ => {},
		}
		if !crushing {
			let mut c = Collect::default();
			ed.visit(&mut c);
			p.field_names.extend(c.names.iter().cloned());
			p.names.extend(c.names);
		}
		match &ed {
			ExternalDeclaration::Preprocessor(pp) => match pp {
				Preprocessor::Define(PreprocessorDefine::ObjectLike { ident, value }) => {
					p.names.insert(ident.0.clone());
					for id in lexer::identifiers(value) {
						p.names.insert(id.to_string());
						p.field_names.insert(id.to_string());
					}
				},
				Preprocessor::Define(PreprocessorDefine::FunctionLike { ident, args, value }) => {
					p.names.insert(ident.0.clone());
					for id in lexer::identifiers(value) {
						if args.iter().any(|a| a.0 == id) {
							continue;
						}
						p.names.insert(id.to_string());
						p.field_names.insert(id.to_string());
					}
				},
				Preprocessor::Undef(u) => {
					p.names.insert(u.name.0.clone());
				},
				Preprocessor::IfDef(d) => {
					p.names.insert(d.ident.0.clone());
				},
				Preprocessor::IfNDef(d) => {
					p.names.insert(d.ident.0.clone());
				},
				Preprocessor::If(i) => {
					p.names.extend(
						lexer::identifiers(&i.condition)
							.iter()
							.map(|s| s.to_string()),
					);
				},
				Preprocessor::ElIf(i) => {
					p.names.extend(
						lexer::identifiers(&i.condition)
							.iter()
							.map(|s| s.to_string()),
					);
				},
				_ => {},
			},
			ExternalDeclaration::Declaration(Declaration::Block(b)) => {
				p.names.insert(b.name.0.clone());
				for f in &b.fields {
					for id in &f.identifiers.0 {
						p.names.insert(id.ident.0.clone());
						p.field_names.insert(id.ident.0.clone());
					}
				}
			},
			_ => {},
		}
		keep.push(ed);
	}
	if keep.is_empty() {
		return Err(CrushError::Unsupported(
			"nothing left after removing the SHADER_CRUSHER pragmas".to_string(),
		));
	}
	tu.0 .0 = keep;

	let mut m = BuiltinMembers::default();
	tu.visit(&mut m);
	p.field_names.extend(m.names);
	Ok(p)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::glsl::parser::Parse;

	fn protect(src: &str) -> (Protection, TranslationUnit) {
		let mut tu = TranslationUnit::parse(src).expect("parse");
		let p = run(&mut tu, &[]).expect("protect");
		(p, tu)
	}

	#[test]
	fn pragmas_are_removed_and_off_regions_pinned() {
		let (p, tu) = protect(
			"#pragma SHADER_CRUSHER_OFF\nuniform float keep_me;\n#pragma SHADER_CRUSHER_ON\nuniform float crush_me;\n#pragma optionNV(fastmath)\nvoid main(){}\n",
		);
		assert!(p.names.contains("keep_me"));
		assert!(!p.names.contains("crush_me"));
		assert!(p.names.contains("main"));
		let pragmas: Vec<_> = tu.0 .0.iter().filter_map(pragma_command).collect();
		assert_eq!(pragmas, ["optionNV(fastmath)"]);
	}

	#[test]
	fn macro_names_bodies_and_conditions_are_pinned_but_not_params() {
		let (p, _) = protect(
			"#define SQ(v) ((v)*(v))\n#define HALF (scale_factor*0.5)\n#ifdef FOO\n#undef BAR\n#endif\n#if defined(BAZ) && QUX > 1\n#elif QUUX\n#endif\nuniform float scale_factor;\nvoid main(){}\n",
		);
		for n in [
			"SQ",
			"HALF",
			"scale_factor",
			"FOO",
			"BAR",
			"BAZ",
			"QUX",
			"QUUX",
			"defined",
		] {
			assert!(p.names.contains(n), "{n}");
		}
		assert!(!p.names.contains("v"), "macro parameter must not be pinned");
		assert!(p.field_names.contains("scale_factor"));
	}

	#[test]
	fn blocks_and_builtin_members_are_pinned() {
		let (p, _) = protect(
			"uniform Light { vec4 light_pos; } light_src;\nstruct S { float intensity; };\nvoid main(){ S s; gl_FragColor = gl_LightSource[0].position + vec4(s.intensity) + gl_FrontMaterial.diffuse; }\n",
		);
		assert!(p.names.contains("Light"));
		assert!(p.names.contains("light_pos"));
		assert!(
			!p.names.contains("light_src"),
			"block instance names are shader-private"
		);
		assert!(p.field_names.contains("position"));
		assert!(p.field_names.contains("diffuse"));
		assert!(!p.field_names.contains("intensity"));
	}

	#[test]
	fn only_pragmas_is_unsupported() {
		let mut tu = TranslationUnit::parse("#pragma SHADER_CRUSHER_OFF\n").expect("parse");
		assert!(matches!(run(&mut tu, &[]), Err(CrushError::Unsupported(_))));
	}
}
