//! AST rewrites that never change meaning. They run before identifier
//! resolution so the self-check compares the printed output against the
//! rewritten tree.

use crate::glsl::syntax::*;
use crate::glsl::visitor::{HostMut, Visit, VisitorMut};

/// Which rewrites to apply; every one is individually switchable so its
/// effect on compressed size can be measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flags {
	/// `void f(void)` → `void f()`.
	pub void_params:    bool,
	/// An empty `{}` statement → `;`.
	pub empty_compound: bool,
}

impl Default for Flags {
	fn default() -> Self {
		Flags {
			void_params:    true,
			empty_compound: true,
		}
	}
}

struct Simplifier {
	flags: Flags,
}

fn is_void_param(p: &FunctionParameterDeclaration) -> bool {
	matches!(
		p,
		FunctionParameterDeclaration::Unnamed(
			None,
			TypeSpecifier {
				ty:              TypeSpecifierNonArray::Void,
				array_specifier: None,
			}
		)
	)
}

impl VisitorMut for Simplifier {
	fn visit_function_prototype(&mut self, p: &mut FunctionPrototype) -> Visit {
		if self.flags.void_params && p.parameters.len() == 1 && is_void_param(&p.parameters[0]) {
			p.parameters.clear();
		}
		Visit::Children
	}

	fn visit_statement(&mut self, st: &mut Statement) -> Visit {
		if self.flags.empty_compound {
			if let Statement::Compound(c) = st {
				if c.statement_list.is_empty() {
					*st = Statement::Simple(Box::new(SimpleStatement::Expression(None)));
				}
			}
		}
		Visit::Children
	}
}

pub fn run(tu: &mut TranslationUnit, flags: &Flags) {
	tu.visit_mut(&mut Simplifier { flags: *flags });
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::glsl::parser::Parse;
	use crate::shader_crusher::printer::print;

	fn simplified(src: &str) -> String {
		let mut tu = TranslationUnit::parse(src).expect("parse");
		run(&mut tu, &Flags::default());
		let out = print(&tu);
		assert_eq!(TranslationUnit::parse(&out).expect("reparse"), tu, "{out}");
		out
	}

	#[test]
	fn void_parameter_lists_become_empty() {
		assert_eq!(simplified("void main(void){}"), "void main(){}");
		assert_eq!(simplified("float f(void);"), "float f();");
		assert_eq!(simplified("float f(float a);"), "float f(float a);");
		assert_eq!(simplified("void main(){f(void);}"), "void main(){f();}");
	}

	#[test]
	fn empty_compound_statements_become_semicolons() {
		assert_eq!(
			simplified("void main(){if(a){}else{}}"),
			"void main(){if(a);else;}"
		);
		assert_eq!(
			simplified("void main(){for(;;){}{}}"),
			"void main(){for(;;);;}"
		);
		assert_eq!(simplified("void main(){}"), "void main(){}");
		assert_eq!(simplified("void main(){{x;}}"), "void main(){{x;}}");
	}
}
