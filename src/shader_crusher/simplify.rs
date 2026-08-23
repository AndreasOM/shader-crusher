//! AST rewrites that never change meaning. They run before identifier
//! resolution so the self-check compares the printed output against the
//! rewritten tree.

use super::lexer;
use crate::glsl::syntax::*;
use crate::glsl::visitor::{HostMut, Visit, VisitorMut};

/// Which rewrites to apply; every one is individually switchable so its
/// effect on compressed size can be measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flags {
	/// `void f(void)` → `void f()`.
	pub void_params:     bool,
	/// An empty `{}` statement → `;`.
	pub empty_compound:  bool,
	/// Drop the default `in` qualifier of function parameters.
	pub in_params:       bool,
	/// `{s}` → `s` for a branch or loop body, and splice declaration-free
	/// blocks into the enclosing block.
	pub unwrap_blocks:   bool,
	/// `float a;float b;` → `float a,b;`.
	pub merge_decls:     bool,
	/// `x=x+y` → `x+=y` when `x` is a side-effect-free lvalue.
	pub compound_assign: bool,
	/// `+e` → `e`.
	pub unary_plus:      bool,
	/// Minimal whitespace inside `#define` bodies and `#if` conditions.
	pub squeeze_macros:  bool,
}

impl Default for Flags {
	fn default() -> Self {
		Flags {
			void_params:     true,
			empty_compound:  true,
			in_params:       true,
			unwrap_blocks:   true,
			merge_decls:     true,
			compound_assign: true,
			unary_plus:      true,
			squeeze_macros:  true,
		}
	}
}

impl Flags {
	/// The rewrite names accepted by `disable` / the CLI.
	pub const NAMES: [&'static str; 8] = [
		"void-params",
		"empty-compound",
		"in-params",
		"unwrap-blocks",
		"merge-decls",
		"compound-assign",
		"unary-plus",
		"squeeze-macros",
	];

	/// Switch one rewrite off by name; `false` if the name is unknown.
	pub fn disable(&mut self, name: &str) -> bool {
		match name {
			"void-params" => self.void_params = false,
			"empty-compound" => self.empty_compound = false,
			"in-params" => self.in_params = false,
			"unwrap-blocks" => self.unwrap_blocks = false,
			"merge-decls" => self.merge_decls = false,
			"compound-assign" => self.compound_assign = false,
			"unary-plus" => self.unary_plus = false,
			"squeeze-macros" => self.squeeze_macros = false,
			_ => return false,
		}
		true
	}
}

// --- expression / prototype / directive rewrites (generic visitor) --------

struct ExprSimplifier {
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

fn drop_in_qualifier(q: &mut Option<TypeQualifier>) {
	if let Some(tq) = q {
		tq.qualifiers
			.0
			.retain(|s| !matches!(s, TypeQualifierSpec::Storage(StorageQualifier::In)));
		if tq.qualifiers.0.is_empty() {
			*q = None;
		}
	}
}

/// An lvalue whose evaluation has no side effects and can be repeated.
fn pure_lvalue(e: &Expr) -> bool {
	match e {
		Expr::Variable(_) => true,
		Expr::Dot(inner, _) => pure_lvalue(inner),
		Expr::Bracket(inner, spec) => {
			pure_lvalue(inner)
				&& spec.dimensions.0.iter().all(|d| match d {
					ArraySpecifierDimension::ExplicitlySized(i) => {
						matches!(
							**i,
							Expr::IntConst(_) | Expr::UIntConst(_) | Expr::Variable(_)
						)
					},
					ArraySpecifierDimension::Unsized => false,
				})
		},
		_ => false,
	}
}

/// No calls, assignments or increments anywhere inside.
fn side_effect_free(e: &Expr) -> bool {
	match e {
		Expr::Variable(_)
		| Expr::IntConst(_)
		| Expr::UIntConst(_)
		| Expr::BoolConst(_)
		| Expr::FloatConst(_)
		| Expr::DoubleConst(_) => true,
		Expr::Unary(UnaryOp::Inc, _) | Expr::Unary(UnaryOp::Dec, _) => false,
		Expr::Unary(_, e) => side_effect_free(e),
		Expr::Binary(_, l, r) | Expr::Comma(l, r) => side_effect_free(l) && side_effect_free(r),
		Expr::Ternary(a, b, c) => side_effect_free(a) && side_effect_free(b) && side_effect_free(c),
		Expr::Bracket(e, spec) => {
			side_effect_free(e)
				&& spec.dimensions.0.iter().all(|d| match d {
					ArraySpecifierDimension::ExplicitlySized(i) => side_effect_free(i),
					ArraySpecifierDimension::Unsized => true,
				})
		},
		Expr::Dot(e, _) => side_effect_free(e),
		Expr::Assignment(..) | Expr::FunCall(..) | Expr::PostInc(_) | Expr::PostDec(_) => false,
	}
}

fn compound_op(op: &BinaryOp) -> Option<AssignmentOp> {
	Some(match op {
		BinaryOp::Add => AssignmentOp::Add,
		BinaryOp::Sub => AssignmentOp::Sub,
		BinaryOp::Mult => AssignmentOp::Mult,
		BinaryOp::Div => AssignmentOp::Div,
		BinaryOp::Mod => AssignmentOp::Mod,
		BinaryOp::LShift => AssignmentOp::LShift,
		BinaryOp::RShift => AssignmentOp::RShift,
		BinaryOp::BitAnd => AssignmentOp::And,
		BinaryOp::BitXor => AssignmentOp::Xor,
		BinaryOp::BitOr => AssignmentOp::Or,
		_ => return None,
	})
}

/// `vec3 a;vec3 b;` → `vec3 a,b;` inside a struct or block.
fn merge_fields(fields: &mut Vec<StructFieldSpecifier>) {
	let mut out: Vec<StructFieldSpecifier> = Vec::with_capacity(fields.len());
	for f in fields.drain(..) {
		match out.last_mut() {
			Some(last)
				if last.qualifier == f.qualifier
					&& last.ty == f.ty
					&& !matches!(f.ty.ty, TypeSpecifierNonArray::Struct(_)) =>
			{
				last.identifiers.0.extend(f.identifiers.0);
			},
			_ => out.push(f),
		}
	}
	*fields = out;
}

impl VisitorMut for ExprSimplifier {
	fn visit_struct_specifier(&mut self, s: &mut StructSpecifier) -> Visit {
		if self.flags.merge_decls {
			merge_fields(&mut s.fields.0);
		}
		Visit::Children
	}

	fn visit_block(&mut self, b: &mut Block) -> Visit {
		if self.flags.merge_decls {
			merge_fields(&mut b.fields);
		}
		Visit::Children
	}

	fn visit_function_prototype(&mut self, p: &mut FunctionPrototype) -> Visit {
		if self.flags.void_params && p.parameters.len() == 1 && is_void_param(&p.parameters[0]) {
			p.parameters.clear();
		}
		if self.flags.in_params {
			for param in &mut p.parameters {
				match param {
					FunctionParameterDeclaration::Named(q, _) => drop_in_qualifier(q),
					FunctionParameterDeclaration::Unnamed(q, _) => drop_in_qualifier(q),
				}
			}
		}
		Visit::Children
	}

	fn visit_expr(&mut self, e: &mut Expr) -> Visit {
		if self.flags.unary_plus {
			while let Expr::Unary(UnaryOp::Add, inner) = e {
				let inner = std::mem::replace(&mut **inner, Expr::IntConst(0));
				*e = inner;
			}
		}
		if self.flags.compound_assign {
			if let Expr::Assignment(l, AssignmentOp::Equal, r) = e {
				if let Expr::Binary(op, l2, y) = &mut **r {
					if l == l2 && pure_lvalue(l) && side_effect_free(y) {
						if let Some(aop) = compound_op(op) {
							let y = std::mem::replace(&mut **y, Expr::IntConst(0));
							let l = std::mem::replace(&mut **l, Expr::IntConst(0));
							*e = Expr::Assignment(Box::new(l), aop, Box::new(y));
						}
					}
				}
			}
		}
		Visit::Children
	}

	fn visit_preprocessor_define(&mut self, pd: &mut PreprocessorDefine) -> Visit {
		if self.flags.squeeze_macros {
			match pd {
				PreprocessorDefine::ObjectLike { value, .. } => *value = lexer::squeeze(value),
				PreprocessorDefine::FunctionLike { value, .. } => *value = lexer::squeeze(value),
			}
		}
		Visit::Parent
	}

	fn visit_preprocessor_if(&mut self, pi: &mut PreprocessorIf) -> Visit {
		if self.flags.squeeze_macros {
			pi.condition = lexer::squeeze(&pi.condition);
		}
		Visit::Parent
	}

	fn visit_preprocessor_elif(&mut self, pe: &mut PreprocessorElIf) -> Visit {
		if self.flags.squeeze_macros {
			pe.condition = lexer::squeeze(&pe.condition);
		}
		Visit::Parent
	}
}

// --- statement-level rewrites (bottom-up) ----------------------------------

struct StmtSimplifier {
	flags: Flags,
}

/// A statement that declares a name (or is a case label) must stay inside
/// its braces: unwrapping it would move it into the enclosing scope. A
/// nameless declaration such as `x;` or `invariant a;` declares nothing.
fn is_decl(st: &Statement) -> bool {
	match st {
		Statement::Simple(s) => match &**s {
			SimpleStatement::CaseLabel(_) => true,
			SimpleStatement::Declaration(Declaration::InitDeclaratorList(l)) => {
				l.head.name.is_some()
					|| !l.tail.is_empty()
					|| matches!(l.head.ty.ty.ty, TypeSpecifierNonArray::Struct(_))
			},
			SimpleStatement::Declaration(Declaration::Global(..)) => false,
			SimpleStatement::Declaration(_) => true,
			_ => false,
		},
		Statement::Compound(_) => false,
	}
}

/// Whether a following `else` would attach to an `if` inside `st`.
fn ends_with_open_if(st: &Statement) -> bool {
	match st {
		Statement::Compound(_) => false,
		Statement::Simple(s) => match &**s {
			SimpleStatement::Selection(sel) => match &sel.rest {
				SelectionRestStatement::Statement(_) => true,
				SelectionRestStatement::Else(_, e) => ends_with_open_if(e),
			},
			SimpleStatement::Iteration(IterationStatement::While(_, b))
			| SimpleStatement::Iteration(IterationStatement::For(_, _, b)) => ends_with_open_if(b),
			_ => false,
		},
	}
}

fn init_list_mut(st: &mut Statement) -> Option<&mut InitDeclaratorList> {
	match st {
		Statement::Simple(s) => match &mut **s {
			SimpleStatement::Declaration(Declaration::InitDeclaratorList(l)) => Some(l),
			_ => None,
		},
		_ => None,
	}
}

fn mergeable(a: &InitDeclaratorList, b: &InitDeclaratorList) -> bool {
	a.head.ty == b.head.ty
		&& a.head.name.is_some()
		&& b.head.name.is_some()
		&& !matches!(b.head.ty.ty.ty, TypeSpecifierNonArray::Struct(_))
}

/// Append `b`'s declarators to `a` (`float a;float b;` → `float a,b;`).
fn merge_into(a: &mut InitDeclaratorList, mut b: InitDeclaratorList) {
	a.tail.push(SingleDeclarationNoType {
		ident:       ArrayedIdentifier {
			ident:      b.head.name.take().expect("named"),
			array_spec: b.head.array_specifier.take(),
		},
		initializer: b.head.initializer.take(),
	});
	a.tail.append(&mut b.tail);
}

impl StmtSimplifier {
	/// A branch or loop body: `{}` → `;`, `{s}` → `s`.
	fn unwrap(&self, st: &mut Statement, guard_else: bool) {
		loop {
			let inner = match st {
				Statement::Compound(c) if c.statement_list.is_empty() => {
					if self.flags.empty_compound {
						*st = Statement::Simple(Box::new(SimpleStatement::Expression(None)));
					}
					return;
				},
				Statement::Compound(c)
					if self.flags.unwrap_blocks
						&& c.statement_list.len() == 1
						&& !is_decl(&c.statement_list[0])
						&& !(guard_else && ends_with_open_if(&c.statement_list[0])) =>
				{
					c.statement_list.pop().expect("one statement")
				},
				_ => return,
			};
			*st = inner;
		}
	}

	/// A statement list (block or switch body): simplify each statement,
	/// splice declaration-free inner blocks, merge adjacent declarations.
	fn list(&self, stmts: &mut Vec<Statement>) {
		for s in stmts.iter_mut() {
			self.stmt(s);
		}
		let mut out: Vec<Statement> = Vec::with_capacity(stmts.len());
		for s in stmts.drain(..) {
			match s {
				Statement::Compound(c)
					if self.flags.unwrap_blocks && !c.statement_list.iter().any(is_decl) =>
				{
					out.extend(c.statement_list);
				},
				s => out.push(s),
			}
		}
		if self.flags.merge_decls {
			let mut merged: Vec<Statement> = Vec::with_capacity(out.len());
			for mut s in out {
				let take = match (
					merged.last_mut().and_then(init_list_mut),
					init_list_mut(&mut s),
				) {
					(Some(a), Some(b)) if mergeable(a, b) => true,
					_ => false,
				};
				if take {
					let a = merged.last_mut().and_then(init_list_mut).expect("checked");
					let b = match s {
						Statement::Simple(s) => match *s {
							SimpleStatement::Declaration(Declaration::InitDeclaratorList(l)) => l,
							_ => unreachable!(),
						},
						_ => unreachable!(),
					};
					merge_into(a, b);
				} else {
					merged.push(s);
				}
			}
			out = merged;
		}
		*stmts = out;
	}

	fn stmt(&self, st: &mut Statement) {
		match st {
			Statement::Compound(c) => self.list(&mut c.statement_list),
			Statement::Simple(s) => match &mut **s {
				SimpleStatement::Selection(sel) => match &mut sel.rest {
					SelectionRestStatement::Statement(t) => {
						self.stmt(t);
						self.unwrap(t, false);
					},
					SelectionRestStatement::Else(t, e) => {
						self.stmt(t);
						self.unwrap(t, true);
						self.stmt(e);
						self.unwrap(e, false);
					},
				},
				SimpleStatement::Switch(sw) => self.list(&mut sw.body),
				SimpleStatement::Iteration(IterationStatement::While(_, b))
				| SimpleStatement::Iteration(IterationStatement::For(_, _, b))
				| SimpleStatement::Iteration(IterationStatement::DoWhile(b, _)) => {
					self.stmt(b);
					self.unwrap(b, false);
				},
				_ => {},
			},
		}
	}

	fn top_level(&self, eds: &mut Vec<ExternalDeclaration>) {
		if !self.flags.merge_decls {
			return;
		}
		let mut out: Vec<ExternalDeclaration> = Vec::with_capacity(eds.len());
		for ed in eds.drain(..) {
			let take = match (out.last(), &ed) {
				(
					Some(ExternalDeclaration::Declaration(Declaration::InitDeclaratorList(a))),
					ExternalDeclaration::Declaration(Declaration::InitDeclaratorList(b)),
				) => mergeable(a, b),
				_ => false,
			};
			if take {
				let b = match ed {
					ExternalDeclaration::Declaration(Declaration::InitDeclaratorList(l)) => l,
					_ => unreachable!(),
				};
				match out.last_mut() {
					Some(ExternalDeclaration::Declaration(Declaration::InitDeclaratorList(a))) => {
						merge_into(a, b)
					},
					_ => unreachable!(),
				}
			} else {
				out.push(ed);
			}
		}
		*eds = out;
	}
}

pub fn run(tu: &mut TranslationUnit, flags: &Flags) {
	tu.visit_mut(&mut ExprSimplifier { flags: *flags });
	let s = StmtSimplifier { flags: *flags };
	for ed in &mut tu.0 .0 {
		if let ExternalDeclaration::FunctionDefinition(fd) = ed {
			s.list(&mut fd.statement.statement_list);
		}
	}
	s.top_level(&mut tu.0 .0);
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

	fn body(src: &str) -> String {
		let out = simplified(&format!("void main(){{{src}}}"));
		out.strip_prefix("void main(){")
			.unwrap()
			.strip_suffix('}')
			.unwrap()
			.to_string()
	}

	#[test]
	fn void_parameter_lists_become_empty() {
		assert_eq!(simplified("void main(void){}"), "void main(){}");
		assert_eq!(simplified("float f(void);"), "float f();");
		assert_eq!(simplified("float f(float a);"), "float f(float a);");
		assert_eq!(simplified("void main(){f(void);}"), "void main(){f();}");
	}

	#[test]
	fn in_qualifier_is_dropped_from_parameters() {
		assert_eq!(
			simplified(
				"float f(in float a, inout float b, const in float c, out float d, in vec2);"
			),
			"float f(float a,inout float b,const float c,out float d,vec2);"
		);
		assert_eq!(simplified("in vec4 p;"), "in vec4 p;");
	}

	#[test]
	fn blocks_are_unwrapped_when_safe() {
		assert_eq!(body("if(a){x;}else{y;}"), "if(a)x;else y;");
		assert_eq!(body("if(a){}else{}"), "if(a);else;");
		assert_eq!(body("if(a){x;}else{if(b)y;}"), "if(a)x;else if(b)y;");
		assert_eq!(body("if(a){if(b)x;}else y;"), "if(a){if(b)x;}else y;");
		assert_eq!(
			body("if(a){while(c)if(b)x;}else y;"),
			"if(a){while(c)if(b)x;}else y;"
		);
		assert_eq!(
			body("if(a){if(b)x;else if(c)y;}else z;"),
			"if(a){if(b)x;else if(c)y;}else z;"
		);
		assert_eq!(
			body("if(a){if(b)x;else y;}else z;"),
			"if(a)if(b)x;else y;else z;"
		);
		assert_eq!(
			body("if(a){do if(b)x;while(c);}else y;"),
			"if(a)do if(b)x;while(c);else y;"
		);
		assert_eq!(body("if(a){float k;}"), "if(a){float k;}");
		assert_eq!(body("while(c){x;}"), "while(c)x;");
		assert_eq!(body("for(;;){{x;}}"), "for(;;)x;");
		assert_eq!(body("do{x;}while(c);"), "do x;while(c);");
		assert_eq!(body("for(;;){}"), "for(;;);");
		assert_eq!(body("{{x;}}"), "x;");
		assert_eq!(body("a();{b();}c();"), "a();b();c();");
		assert_eq!(body("a();{}c();"), "a();c();");
		assert_eq!(body("{int k;}int k;"), "{int k;}int k;");
		assert_eq!(
			body("switch(a){case 1:{x;break;}default:{int k;}}"),
			"switch(a){case 1:x;break;default:{int k;}}"
		);
	}

	#[test]
	fn adjacent_declarations_merge() {
		assert_eq!(body("float a;float b;"), "float a,b;");
		assert_eq!(
			body("float a=1.;float b=a;float c[2];"),
			"float a=1.,b=a,c[2];"
		);
		assert_eq!(body("float a;int b;"), "float a;int b;");
		assert_eq!(body("float a;x;float b;"), "float a;x;float b;");
		assert_eq!(body("float a;{float b;}"), "float a;{float b;}");
		assert_eq!(
			simplified("uniform float u;uniform float v;"),
			"uniform float u,v;"
		);
		assert_eq!(
			simplified("uniform float u;varying float v;"),
			"uniform float u;varying float v;"
		);
		assert_eq!(
			simplified("layout(location=0)in vec4 a;layout(location=1)in vec4 b;"),
			"layout(location=0)in vec4 a;layout(location=1)in vec4 b;"
		);
		assert_eq!(
			simplified("struct S{int a;}s;struct T{int b;}t;"),
			"struct S{int a;}s;struct T{int b;}t;"
		);
		assert_eq!(simplified("S a;S b,c;"), "S a,b,c;");
		assert_eq!(
			simplified("struct S{vec3 a;vec3 b;float c;float d[2];};uniform B{vec4 x;vec4 y;}b;"),
			"struct S{vec3 a,b;float c,d[2];};uniform B{vec4 x,y;}b;"
		);
		assert_eq!(
			simplified("struct S{struct T{int a;}t;struct T2{int b;}u;};"),
			"struct S{struct T{int a;}t;struct T2{int b;}u;};"
		);
		assert_eq!(
			simplified("float a;\n#define X\nfloat b;"),
			"float a;\n#define X\nfloat b;"
		);
		assert_eq!(
			simplified("invariant gl_Position;invariant gl_PointSize;"),
			"invariant gl_Position;invariant gl_PointSize;"
		);
	}

	#[test]
	fn compound_assignment_for_pure_lvalues() {
		assert_eq!(body("x=x+y;"), "x+=y;");
		assert_eq!(body("v=v*m;"), "v*=m;");
		assert_eq!(body("x=y+x;"), "x=y+x;");
		assert_eq!(body("x=x+f();"), "x=x+f();");
		assert_eq!(body("x=x+y+z;"), "x=x+y+z;");
		assert_eq!(body("x=x-(y+z);"), "x-=y+z;");
		assert_eq!(body("a[i]=a[i]+1;"), "a[i]+=1;");
		assert_eq!(body("a[i++]=a[i++]+1;"), "a[i++]=a[i++]+1;");
		assert_eq!(body("v.x=v.x*2.;"), "v.x*=2.;");
		assert_eq!(body("x=x<<2;"), "x<<=2;");
		assert_eq!(body("x=x&&y;"), "x=x&&y;");
		assert_eq!(body("f()=f()+1;"), "f()=f()+1;");
	}

	#[test]
	fn unary_plus_is_dropped() {
		assert_eq!(body("x=+a;"), "x=a;");
		assert_eq!(body("x=a+ +b;"), "x=a+b;");
		assert_eq!(body("x=+ +a;"), "x=a;");
		assert_eq!(body("x=+(a+b);"), "x=a+b;");
		assert_eq!(body("x=-+a;"), "x=-a;");
		assert_eq!(body("x=x+ +y;"), "x+=y;");
	}

	#[test]
	fn macros_and_conditions_are_squeezed() {
		assert_eq!(
			simplified("#define SQ(v) ( (v) * (v) )\nvoid main(){}"),
			"#define SQ(v) ((v)*(v))\nvoid main(){}"
		);
		assert_eq!(
			simplified("#define test1 int sum = 1; \\\n   sum = test;\nvoid main(){}"),
			"#define test1 int sum=1;sum=test;\nvoid main(){}"
		);
		assert_eq!(
			simplified("#if defined ( X ) && Y > 1\n#elif Z - -1\n#endif\nvoid main(){}"),
			"#if defined(X)&&Y>1\n#elif Z- -1\n#endif\nvoid main(){}"
		);
		assert_eq!(
			simplified("#define X\nvoid main(){}"),
			"#define X\nvoid main(){}"
		);
		assert_eq!(
			simplified("#pragma optionNV( fastmath )\nvoid main(){}"),
			"#pragma optionNV( fastmath )\nvoid main(){}"
		);
	}
}
