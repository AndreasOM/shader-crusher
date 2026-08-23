//! Minimal-whitespace GLSL printer.
//!
//! Faithful to the AST *as the vendored parser builds it*: printing and
//! re-parsing yields an equal tree (checked by `selfcheck`). That rules the
//! details below:
//! - parentheses come from operator precedence only; `|`, `^`, `&` (and `,`
//!   and assignment) are right-nested by the parser, everything else is
//!   left-folded, so the child contexts mirror that;
//! - call arguments and initializers are `assignment_expr`, so an embedded
//!   comma expression gets parentheses;
//! - a negative `IntConst` (only produced from a hex/octal literal whose bit
//!   pattern is negative) is printed as hex, since `-1` would parse as a
//!   unary minus;
//! - a space is emitted only where two tokens would otherwise merge
//!   (identifier/number characters, `+ +`, `- -`); directives sit on their
//!   own line.

use crate::glsl::syntax::*;

/// Precedence levels as used by the parser; lower binds tighter. An
/// expression needs parentheses where its level exceeds the context's.
const CTX_EXPR: u8 = 17; // `expr`: comma allowed
const CTX_ASSIGN: u8 = 16; // `assignment_expr`
const CTX_COND: u8 = 15; // `cond_expr`
const CTX_UNARY: u8 = 3; // operand of a unary operator / assignment target
const CTX_POSTFIX: u8 = 2; // base of `.`, `[]`, `()`, `++`, `--`

fn binop_prec(op: &BinaryOp) -> u8 {
	match op {
		BinaryOp::Mult | BinaryOp::Div | BinaryOp::Mod => 4,
		BinaryOp::Add | BinaryOp::Sub => 5,
		BinaryOp::LShift | BinaryOp::RShift => 6,
		BinaryOp::LT | BinaryOp::GT | BinaryOp::LTE | BinaryOp::GTE => 7,
		BinaryOp::Equal | BinaryOp::NonEqual => 8,
		BinaryOp::BitAnd => 9,
		BinaryOp::BitXor => 10,
		BinaryOp::BitOr => 11,
		BinaryOp::And => 12,
		BinaryOp::Xor => 13,
		BinaryOp::Or => 14,
	}
}

/// The parser builds `a|b|c` as `a|(b|c)` for these.
fn right_nested(op: &BinaryOp) -> bool {
	matches!(op, BinaryOp::BitAnd | BinaryOp::BitXor | BinaryOp::BitOr)
}

fn prec(e: &Expr) -> u8 {
	match e {
		Expr::Variable(_) | Expr::IntConst(_) | Expr::UIntConst(_) | Expr::BoolConst(_) => 0,
		Expr::FloatConst(x) => {
			if *x < 0.0 {
				CTX_UNARY
			} else {
				0
			}
		},
		Expr::DoubleConst(x) => {
			if *x < 0.0 {
				CTX_UNARY
			} else {
				0
			}
		},
		Expr::Unary(..) => CTX_UNARY,
		Expr::Binary(op, ..) => binop_prec(op),
		Expr::Ternary(..) => CTX_COND,
		Expr::Assignment(..) => CTX_ASSIGN,
		Expr::Bracket(..)
		| Expr::FunCall(..)
		| Expr::Dot(..)
		| Expr::PostInc(_)
		| Expr::PostDec(_) => CTX_POSTFIX,
		Expr::Comma(..) => CTX_EXPR,
	}
}

fn unary_op_str(op: &UnaryOp) -> &'static str {
	match op {
		UnaryOp::Inc => "++",
		UnaryOp::Dec => "--",
		UnaryOp::Add => "+",
		UnaryOp::Minus => "-",
		UnaryOp::Not => "!",
		UnaryOp::Complement => "~",
	}
}

fn binary_op_str(op: &BinaryOp) -> &'static str {
	match op {
		BinaryOp::Or => "||",
		BinaryOp::Xor => "^^",
		BinaryOp::And => "&&",
		BinaryOp::BitOr => "|",
		BinaryOp::BitXor => "^",
		BinaryOp::BitAnd => "&",
		BinaryOp::Equal => "==",
		BinaryOp::NonEqual => "!=",
		BinaryOp::LT => "<",
		BinaryOp::GT => ">",
		BinaryOp::LTE => "<=",
		BinaryOp::GTE => ">=",
		BinaryOp::LShift => "<<",
		BinaryOp::RShift => ">>",
		BinaryOp::Add => "+",
		BinaryOp::Sub => "-",
		BinaryOp::Mult => "*",
		BinaryOp::Div => "/",
		BinaryOp::Mod => "%",
	}
}

fn assignment_op_str(op: &AssignmentOp) -> &'static str {
	match op {
		AssignmentOp::Equal => "=",
		AssignmentOp::Mult => "*=",
		AssignmentOp::Div => "/=",
		AssignmentOp::Mod => "%=",
		AssignmentOp::Add => "+=",
		AssignmentOp::Sub => "-=",
		AssignmentOp::LShift => "<<=",
		AssignmentOp::RShift => ">>=",
		AssignmentOp::And => "&=",
		AssignmentOp::Xor => "^=",
		AssignmentOp::Or => "|=",
	}
}

fn type_keyword(t: &TypeSpecifierNonArray) -> Option<&'static str> {
	use TypeSpecifierNonArray::*;
	Some(match t {
		Void => "void",
		Bool => "bool",
		Int => "int",
		UInt => "uint",
		Float => "float",
		Double => "double",
		Vec2 => "vec2",
		Vec3 => "vec3",
		Vec4 => "vec4",
		DVec2 => "dvec2",
		DVec3 => "dvec3",
		DVec4 => "dvec4",
		BVec2 => "bvec2",
		BVec3 => "bvec3",
		BVec4 => "bvec4",
		IVec2 => "ivec2",
		IVec3 => "ivec3",
		IVec4 => "ivec4",
		UVec2 => "uvec2",
		UVec3 => "uvec3",
		UVec4 => "uvec4",
		Mat2 => "mat2",
		Mat3 => "mat3",
		Mat4 => "mat4",
		Mat23 => "mat2x3",
		Mat24 => "mat2x4",
		Mat32 => "mat3x2",
		Mat34 => "mat3x4",
		Mat42 => "mat4x2",
		Mat43 => "mat4x3",
		DMat2 => "dmat2",
		DMat3 => "dmat3",
		DMat4 => "dmat4",
		DMat23 => "dmat2x3",
		DMat24 => "dmat2x4",
		DMat32 => "dmat3x2",
		DMat34 => "dmat3x4",
		DMat42 => "dmat4x2",
		DMat43 => "dmat4x3",
		Sampler1D => "sampler1D",
		Image1D => "image1D",
		Sampler2D => "sampler2D",
		Image2D => "image2D",
		Sampler3D => "sampler3D",
		Image3D => "image3D",
		SamplerCube => "samplerCube",
		ImageCube => "imageCube",
		Sampler2DRect => "sampler2DRect",
		Image2DRect => "image2DRect",
		Sampler1DArray => "sampler1DArray",
		Image1DArray => "image1DArray",
		Sampler2DArray => "sampler2DArray",
		Image2DArray => "image2DArray",
		SamplerBuffer => "samplerBuffer",
		ImageBuffer => "imageBuffer",
		Sampler2DMS => "sampler2DMS",
		Image2DMS => "image2DMS",
		Sampler2DMSArray => "sampler2DMSArray",
		Image2DMSArray => "image2DMSArray",
		SamplerCubeArray => "samplerCubeArray",
		ImageCubeArray => "imageCubeArray",
		Sampler1DShadow => "sampler1DShadow",
		Sampler2DShadow => "sampler2DShadow",
		Sampler2DRectShadow => "sampler2DRectShadow",
		Sampler1DArrayShadow => "sampler1DArrayShadow",
		Sampler2DArrayShadow => "sampler2DArrayShadow",
		SamplerCubeShadow => "samplerCubeShadow",
		SamplerCubeArrayShadow => "samplerCubeArrayShadow",
		ISampler1D => "isampler1D",
		IImage1D => "iimage1D",
		ISampler2D => "isampler2D",
		IImage2D => "iimage2D",
		ISampler3D => "isampler3D",
		IImage3D => "iimage3D",
		ISamplerCube => "isamplerCube",
		IImageCube => "iimageCube",
		ISampler2DRect => "isampler2DRect",
		IImage2DRect => "iimage2DRect",
		ISampler1DArray => "isampler1DArray",
		IImage1DArray => "iimage1DArray",
		ISampler2DArray => "isampler2DArray",
		IImage2DArray => "iimage2DArray",
		ISamplerBuffer => "isamplerBuffer",
		IImageBuffer => "iimageBuffer",
		ISampler2DMS => "isampler2DMS",
		IImage2DMS => "iimage2DMS",
		ISampler2DMSArray => "isampler2DMSArray",
		IImage2DMSArray => "iimage2DMSArray",
		ISamplerCubeArray => "isamplerCubeArray",
		IImageCubeArray => "iimageCubeArray",
		AtomicUInt => "atomic_uint",
		USampler1D => "usampler1D",
		UImage1D => "uimage1D",
		USampler2D => "usampler2D",
		UImage2D => "uimage2D",
		USampler3D => "usampler3D",
		UImage3D => "uimage3D",
		USamplerCube => "usamplerCube",
		UImageCube => "uimageCube",
		USampler2DRect => "usampler2DRect",
		UImage2DRect => "uimage2DRect",
		USampler1DArray => "usampler1DArray",
		UImage1DArray => "uimage1DArray",
		USampler2DArray => "usampler2DArray",
		UImage2DArray => "uimage2DArray",
		USamplerBuffer => "usamplerBuffer",
		UImageBuffer => "uimageBuffer",
		USampler2DMS => "usampler2DMS",
		UImage2DMS => "uimage2DMS",
		USampler2DMSArray => "usampler2DMSArray",
		UImage2DMSArray => "uimage2DMSArray",
		USamplerCubeArray => "usamplerCubeArray",
		UImageCubeArray => "uimageCubeArray",
		Struct(_) | TypeName(_) => return None,
	})
}

/// Identifier/number character for the token-join rule. Sentinel characters
/// (private use plane, see `scope`) count as identifier characters so the
/// spacing of the sentinel print equals the spacing of the final print.
fn is_ident_char(c: char) -> bool {
	c.is_ascii_alphanumeric() || c == '_' || (c as u32) >= 0xF0000
}

struct Out {
	buf: String,
}

impl Out {
	/// Append a token, inserting a space only if it would otherwise merge
	/// with the previous one.
	fn push(&mut self, tok: &str) {
		if let (Some(last), Some(first)) = (self.buf.chars().next_back(), tok.chars().next()) {
			let merge = (is_ident_char(last) && is_ident_char(first))
				|| (last == '+' && first == '+')
				|| (last == '-' && first == '-');
			if merge {
				self.buf.push(' ');
			}
		}
		self.buf.push_str(tok);
	}

	/// Append a preprocessor directive on a line of its own.
	fn directive(&mut self, line: &str) {
		if !self.buf.is_empty() && !self.buf.ends_with('\n') {
			self.buf.push('\n');
		}
		self.buf.push_str(line);
		self.buf.push('\n');
	}

	fn expr(&mut self, e: &Expr, ctx: u8) {
		let paren = prec(e) > ctx;
		if paren {
			self.push("(");
		}
		match e {
			Expr::Variable(i) => self.push(&i.0),
			Expr::IntConst(v) => {
				if *v >= 0 {
					self.push(&v.to_string());
				} else {
					// only a hex/octal literal yields a negative value; `-n` would re-parse as unary minus
					self.push(&format!("0x{:X}", *v as u32));
				}
			},
			Expr::UIntConst(v) => self.push(&format!("{}u", v)),
			Expr::BoolConst(b) => self.push(if *b { "true" } else { "false" }),
			Expr::FloatConst(x) => self.push(&format_float(*x)),
			Expr::DoubleConst(x) => self.push(&format_double(*x)),
			Expr::Unary(op, e) => {
				self.push(unary_op_str(op));
				self.expr(e, CTX_UNARY);
			},
			Expr::Binary(op, l, r) => {
				let p = binop_prec(op);
				if right_nested(op) {
					self.expr(l, p - 1);
					self.push(binary_op_str(op));
					self.expr(r, p);
				} else {
					self.expr(l, p);
					self.push(binary_op_str(op));
					self.expr(r, p - 1);
				}
			},
			Expr::Ternary(c, a, b) => {
				self.expr(c, CTX_COND - 1);
				self.push("?");
				self.expr(a, CTX_EXPR);
				self.push(":");
				self.expr(b, CTX_ASSIGN);
			},
			Expr::Assignment(l, op, r) => {
				self.expr(l, CTX_UNARY);
				self.push(assignment_op_str(op));
				self.expr(r, CTX_ASSIGN);
			},
			Expr::Bracket(e, spec) => {
				self.expr(e, CTX_POSTFIX);
				self.array_spec(spec);
			},
			Expr::FunCall(fi, args) => {
				match fi {
					FunIdentifier::Identifier(i) => self.push(&i.0),
					FunIdentifier::Expr(e) => self.expr(e, CTX_POSTFIX),
				}
				self.push("(");
				for (n, a) in args.iter().enumerate() {
					if n > 0 {
						self.push(",");
					}
					self.expr(a, CTX_ASSIGN);
				}
				self.push(")");
			},
			Expr::Dot(e, i) => {
				self.expr(e, CTX_POSTFIX);
				self.push(".");
				self.push(&i.0);
			},
			Expr::PostInc(e) => {
				self.expr(e, CTX_POSTFIX);
				self.push("++");
			},
			Expr::PostDec(e) => {
				self.expr(e, CTX_POSTFIX);
				self.push("--");
			},
			Expr::Comma(a, b) => {
				self.expr(a, CTX_ASSIGN);
				self.push(",");
				self.expr(b, CTX_EXPR);
			},
		}
		if paren {
			self.push(")");
		}
	}

	fn array_spec(&mut self, a: &ArraySpecifier) {
		for d in &a.dimensions.0 {
			self.push("[");
			if let ArraySpecifierDimension::ExplicitlySized(e) = d {
				self.expr(e, CTX_COND);
			}
			self.push("]");
		}
	}

	fn arrayed_identifier(&mut self, a: &ArrayedIdentifier) {
		self.push(&a.ident.0);
		if let Some(spec) = &a.array_spec {
			self.array_spec(spec);
		}
	}

	fn type_specifier_non_array(&mut self, t: &TypeSpecifierNonArray) {
		match t {
			TypeSpecifierNonArray::Struct(s) => self.struct_specifier(s),
			TypeSpecifierNonArray::TypeName(tn) => self.push(&tn.0),
			other => self.push(type_keyword(other).expect("keyword type")),
		}
	}

	fn type_specifier(&mut self, t: &TypeSpecifier) {
		self.type_specifier_non_array(&t.ty);
		if let Some(spec) = &t.array_specifier {
			self.array_spec(spec);
		}
	}

	fn fully_specified_type(&mut self, t: &FullySpecifiedType) {
		if let Some(q) = &t.qualifier {
			self.type_qualifier(q);
		}
		self.type_specifier(&t.ty);
	}

	fn struct_specifier(&mut self, s: &StructSpecifier) {
		self.push("struct");
		if let Some(n) = &s.name {
			self.push(&n.0);
		}
		self.push("{");
		for f in &s.fields.0 {
			self.struct_field(f);
		}
		self.push("}");
	}

	fn struct_field(&mut self, f: &StructFieldSpecifier) {
		if let Some(q) = &f.qualifier {
			self.type_qualifier(q);
		}
		self.type_specifier(&f.ty);
		for (n, id) in f.identifiers.0.iter().enumerate() {
			if n > 0 {
				self.push(",");
			}
			self.arrayed_identifier(id);
		}
		self.push(";");
	}

	fn type_qualifier(&mut self, q: &TypeQualifier) {
		for spec in &q.qualifiers.0 {
			match spec {
				TypeQualifierSpec::Storage(s) => self.storage_qualifier(s),
				TypeQualifierSpec::Layout(l) => self.layout_qualifier(l),
				TypeQualifierSpec::Precision(p) => self.push(precision_str(p)),
				TypeQualifierSpec::Interpolation(i) => self.push(match i {
					InterpolationQualifier::Smooth => "smooth",
					InterpolationQualifier::Flat => "flat",
					InterpolationQualifier::NoPerspective => "noperspective",
				}),
				TypeQualifierSpec::Invariant => self.push("invariant"),
				TypeQualifierSpec::Precise => self.push("precise"),
			}
		}
	}

	fn storage_qualifier(&mut self, s: &StorageQualifier) {
		use StorageQualifier::*;
		let w = match s {
			Const => "const",
			InOut => "inout",
			In => "in",
			Out => "out",
			Centroid => "centroid",
			Patch => "patch",
			Sample => "sample",
			Uniform => "uniform",
			Attribute => "attribute",
			Varying => "varying",
			Buffer => "buffer",
			Shared => "shared",
			Coherent => "coherent",
			Volatile => "volatile",
			Restrict => "restrict",
			ReadOnly => "readonly",
			WriteOnly => "writeonly",
			Subroutine(names) => {
				self.push("subroutine");
				if !names.is_empty() {
					self.push("(");
					for (n, t) in names.iter().enumerate() {
						if n > 0 {
							self.push(",");
						}
						self.push(&t.0);
					}
					self.push(")");
				}
				return;
			},
		};
		self.push(w);
	}

	fn layout_qualifier(&mut self, l: &LayoutQualifier) {
		self.push("layout");
		self.push("(");
		for (n, id) in l.ids.0.iter().enumerate() {
			if n > 0 {
				self.push(",");
			}
			match id {
				LayoutQualifierSpec::Identifier(i, None) => self.push(&i.0),
				LayoutQualifierSpec::Identifier(i, Some(e)) => {
					self.push(&i.0);
					self.push("=");
					self.expr(e, CTX_COND);
				},
				LayoutQualifierSpec::Shared => self.push("shared"),
			}
		}
		self.push(")");
	}

	fn initializer(&mut self, i: &Initializer) {
		match i {
			Initializer::Simple(e) => self.expr(e, CTX_ASSIGN),
			Initializer::List(l) => {
				self.push("{");
				for (n, i) in l.0.iter().enumerate() {
					if n > 0 {
						self.push(",");
					}
					self.initializer(i);
				}
				self.push("}");
			},
		}
	}

	fn single_declaration(&mut self, d: &SingleDeclaration) {
		self.fully_specified_type(&d.ty);
		if let Some(n) = &d.name {
			self.push(&n.0);
		}
		if let Some(spec) = &d.array_specifier {
			self.array_spec(spec);
		}
		if let Some(i) = &d.initializer {
			self.push("=");
			self.initializer(i);
		}
	}

	fn function_prototype(&mut self, p: &FunctionPrototype) {
		self.fully_specified_type(&p.ty);
		self.push(&p.name.0);
		self.push("(");
		for (n, param) in p.parameters.iter().enumerate() {
			if n > 0 {
				self.push(",");
			}
			match param {
				FunctionParameterDeclaration::Named(q, d) => {
					if let Some(q) = q {
						self.type_qualifier(q);
					}
					self.type_specifier(&d.ty);
					self.arrayed_identifier(&d.ident);
				},
				FunctionParameterDeclaration::Unnamed(q, ty) => {
					if let Some(q) = q {
						self.type_qualifier(q);
					}
					self.type_specifier(ty);
				},
			}
		}
		self.push(")");
	}

	fn declaration(&mut self, d: &Declaration) {
		match d {
			Declaration::FunctionPrototype(p) => {
				self.function_prototype(p);
				self.push(";");
			},
			Declaration::InitDeclaratorList(l) => {
				self.single_declaration(&l.head);
				for t in &l.tail {
					self.push(",");
					self.arrayed_identifier(&t.ident);
					if let Some(i) = &t.initializer {
						self.push("=");
						self.initializer(i);
					}
				}
				self.push(";");
			},
			Declaration::Precision(q, ty) => {
				self.push("precision");
				self.push(precision_str(q));
				self.type_specifier(ty);
				self.push(";");
			},
			Declaration::Block(b) => {
				self.type_qualifier(&b.qualifier);
				self.push(&b.name.0);
				self.push("{");
				for f in &b.fields {
					self.struct_field(f);
				}
				self.push("}");
				if let Some(id) = &b.identifier {
					self.arrayed_identifier(id);
				}
				self.push(";");
			},
			Declaration::Global(q, ids) => {
				// the parser wants a comma before every identifier here
				self.type_qualifier(q);
				for id in ids {
					self.push(",");
					self.push(&id.0);
				}
				self.push(";");
			},
		}
	}

	fn condition(&mut self, c: &Condition) {
		match c {
			Condition::Expr(e) => self.expr(e, CTX_EXPR),
			Condition::Assignment(ty, name, init) => {
				self.fully_specified_type(ty);
				self.push(&name.0);
				self.push("=");
				self.initializer(init);
			},
		}
	}

	fn compound(&mut self, c: &CompoundStatement) {
		self.push("{");
		for st in &c.statement_list {
			self.statement(st);
		}
		self.push("}");
	}

	fn statement(&mut self, st: &Statement) {
		match st {
			Statement::Compound(c) => self.compound(c),
			Statement::Simple(s) => self.simple_statement(s),
		}
	}

	fn simple_statement(&mut self, s: &SimpleStatement) {
		match s {
			SimpleStatement::Declaration(d) => self.declaration(d),
			SimpleStatement::Expression(e) => {
				if let Some(e) = e {
					self.expr(e, CTX_EXPR);
				}
				self.push(";");
			},
			SimpleStatement::Selection(sel) => {
				self.push("if");
				self.push("(");
				self.expr(&sel.cond, CTX_EXPR);
				self.push(")");
				match &sel.rest {
					SelectionRestStatement::Statement(st) => self.statement(st),
					SelectionRestStatement::Else(a, b) => {
						self.statement(a);
						self.push("else");
						self.statement(b);
					},
				}
			},
			SimpleStatement::Switch(sw) => {
				self.push("switch");
				self.push("(");
				self.expr(&sw.head, CTX_EXPR);
				self.push(")");
				self.push("{");
				for st in &sw.body {
					self.statement(st);
				}
				self.push("}");
			},
			SimpleStatement::CaseLabel(CaseLabel::Case(e)) => {
				self.push("case");
				self.expr(e, CTX_EXPR);
				self.push(":");
			},
			SimpleStatement::CaseLabel(CaseLabel::Def) => {
				self.push("default");
				self.push(":");
			},
			SimpleStatement::Iteration(IterationStatement::While(c, body)) => {
				self.push("while");
				self.push("(");
				self.condition(c);
				self.push(")");
				self.statement(body);
			},
			SimpleStatement::Iteration(IterationStatement::DoWhile(body, e)) => {
				self.push("do");
				self.statement(body);
				self.push("while");
				self.push("(");
				self.expr(e, CTX_EXPR);
				self.push(")");
				self.push(";");
			},
			SimpleStatement::Iteration(IterationStatement::For(init, rest, body)) => {
				self.push("for");
				self.push("(");
				match init {
					ForInitStatement::Expression(e) => {
						if let Some(e) = e {
							self.expr(e, CTX_EXPR);
						}
						self.push(";");
					},
					ForInitStatement::Declaration(d) => self.declaration(d),
				}
				if let Some(c) = &rest.condition {
					self.condition(c);
				}
				self.push(";");
				if let Some(e) = &rest.post_expr {
					self.expr(e, CTX_EXPR);
				}
				self.push(")");
				self.statement(body);
			},
			SimpleStatement::Jump(j) => match j {
				JumpStatement::Continue => {
					self.push("continue");
					self.push(";");
				},
				JumpStatement::Break => {
					self.push("break");
					self.push(";");
				},
				JumpStatement::Discard => {
					self.push("discard");
					self.push(";");
				},
				JumpStatement::Return(e) => {
					self.push("return");
					if let Some(e) = e {
						self.expr(e, CTX_EXPR);
					}
					self.push(";");
				},
			},
		}
	}

	fn preprocessor(&mut self, pp: &Preprocessor) {
		let line = match pp {
			Preprocessor::Define(PreprocessorDefine::ObjectLike { ident, value }) => {
				if value.is_empty() {
					format!("#define {}", ident.0)
				} else {
					format!("#define {} {}", ident.0, value)
				}
			},
			Preprocessor::Define(PreprocessorDefine::FunctionLike { ident, args, value }) => {
				let args: Vec<&str> = args.iter().map(|a| a.0.as_str()).collect();
				if value.is_empty() {
					format!("#define {}({})", ident.0, args.join(","))
				} else {
					format!("#define {}({}) {}", ident.0, args.join(","), value)
				}
			},
			Preprocessor::Else => "#else".to_string(),
			Preprocessor::ElIf(e) => format!("#elif {}", e.condition),
			Preprocessor::EndIf => "#endif".to_string(),
			Preprocessor::Error(e) => {
				if e.message.is_empty() {
					"#error".to_string()
				} else {
					format!("#error {}", e.message)
				}
			},
			Preprocessor::If(i) => format!("#if {}", i.condition),
			Preprocessor::IfDef(d) => format!("#ifdef {}", d.ident.0),
			Preprocessor::IfNDef(d) => format!("#ifndef {}", d.ident.0),
			Preprocessor::Include(i) => match &i.path {
				Path::Absolute(p) => format!("#include <{}>", p),
				Path::Relative(p) => format!("#include \"{}\"", p),
			},
			Preprocessor::Line(l) => match l.source_string_number {
				Some(n) => format!("#line {} {}", l.line, n),
				None => format!("#line {}", l.line),
			},
			Preprocessor::Pragma(p) => {
				if p.command.is_empty() {
					"#pragma".to_string()
				} else {
					format!("#pragma {}", p.command)
				}
			},
			Preprocessor::Undef(u) => format!("#undef {}", u.name.0),
			Preprocessor::Version(v) => {
				let profile = match v.profile {
					None => "",
					Some(PreprocessorVersionProfile::Core) => " core",
					Some(PreprocessorVersionProfile::Compatibility) => " compatibility",
					Some(PreprocessorVersionProfile::ES) => " es",
				};
				format!("#version {}{}", v.version, profile)
			},
			Preprocessor::Extension(e) => {
				let name = match &e.name {
					PreprocessorExtensionName::All => "all",
					PreprocessorExtensionName::Specific(s) => s.as_str(),
				};
				match &e.behavior {
					None => format!("#extension {}", name),
					Some(b) => {
						let b = match b {
							PreprocessorExtensionBehavior::Require => "require",
							PreprocessorExtensionBehavior::Enable => "enable",
							PreprocessorExtensionBehavior::Warn => "warn",
							PreprocessorExtensionBehavior::Disable => "disable",
						};
						format!("#extension {} : {}", name, b)
					},
				}
			},
		};
		self.directive(&line);
	}
}

fn precision_str(p: &PrecisionQualifier) -> &'static str {
	match p {
		PrecisionQualifier::High => "highp",
		PrecisionQualifier::Medium => "mediump",
		PrecisionQualifier::Low => "lowp",
	}
}

/// Print a whole shader with minimal whitespace.
pub fn print(tu: &TranslationUnit) -> String {
	let mut out = Out { buf: String::new() };
	for ed in &tu.0 .0 {
		match ed {
			ExternalDeclaration::Preprocessor(pp) => out.preprocessor(pp),
			ExternalDeclaration::FunctionDefinition(fd) => {
				out.function_prototype(&fd.prototype);
				out.compound(&fd.statement);
			},
			ExternalDeclaration::Declaration(d) => out.declaration(d),
		}
	}
	// a directive at the very end does not need its newline
	if out.buf.ends_with('\n') {
		out.buf.pop();
	}
	out.buf
}

/// Decimal digits and exponent of a finite positive number from Rust's
/// shortest round-trip scientific rendering: `x = digits × 10^k`.
fn decompose(sci: &str) -> (String, i32) {
	let (mant, exp) = sci.split_once('e').expect("scientific notation");
	let exp: i32 = exp.parse().expect("exponent");
	let digits: String = mant.chars().filter(|c| *c != '.').collect();
	let k = exp - (digits.len() as i32 - 1);
	(digits, k)
}

/// All GLSL spellings of `digits × 10^k` (plain decimal and every
/// mantissa/exponent split), shortest first.
fn float_candidates(digits: &str, k: i32) -> Vec<String> {
	let n = digits.len() as i32;
	let mut cands = Vec::new();
	if k >= 0 {
		cands.push(format!("{}{}.", digits, "0".repeat(k as usize)));
	} else if -k < n {
		let s = (n + k) as usize;
		cands.push(format!("{}.{}", &digits[..s], &digits[s..]));
	} else {
		cands.push(format!(".{}{}", "0".repeat((-k - n) as usize), digits));
	}
	for s in 0..=n {
		let e = k + (n - s);
		let s = s as usize;
		let m = if s == 0 {
			format!(".{}", digits)
		} else if s == digits.len() {
			digits.to_string()
		} else {
			format!("{}.{}", &digits[..s], &digits[s..])
		};
		cands.push(format!("{}e{}", m, e));
	}
	cands.sort_by(|a, b| a.len().cmp(&b.len()).then(a.cmp(b)));
	cands
}

/// The parser's reading of a literal: a leading `.` gets a `0` in front and
/// the rest goes through Rust's float parser.
fn parser_reads(lit: &str) -> String {
	if lit.starts_with('.') {
		format!("0{}", lit)
	} else {
		lit.to_string()
	}
}

/// Shortest GLSL float literal that parses back to exactly `x`.
pub fn format_float(x: f32) -> String {
	if !x.is_finite() {
		return format!("{}", x);
	}
	if x < 0.0 {
		return format!("-{}", format_float(-x));
	}
	if x == 0.0 {
		return "0.".to_string();
	}
	let (digits, k) = decompose(&format!("{:e}", x));
	float_candidates(&digits, k)
		.into_iter()
		.find(|c| parser_reads(c).parse::<f32>().ok() == Some(x))
		.expect("the shortest decimal rendering round-trips")
}

/// Shortest GLSL double literal (`lf` suffix) that parses back to exactly `x`.
pub fn format_double(x: f64) -> String {
	if !x.is_finite() {
		return format!("{}lf", x);
	}
	if x < 0.0 {
		return format!("-{}", format_double(-x));
	}
	if x == 0.0 {
		return "0.lf".to_string();
	}
	let (digits, k) = decompose(&format!("{:e}", x));
	let lit = float_candidates(&digits, k)
		.into_iter()
		.find(|c| parser_reads(c).parse::<f64>().ok() == Some(x))
		.expect("the shortest decimal rendering round-trips");
	format!("{}lf", lit)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::glsl::parser::Parse;

	/// Parse, print, and check that the print parses back to the same AST.
	fn rt(src: &str) -> String {
		let tu = TranslationUnit::parse(src).unwrap_or_else(|e| panic!("{src}: {e}"));
		let out = print(&tu);
		let again = TranslationUnit::parse(&out).unwrap_or_else(|e| panic!("{src} -> {out}: {e}"));
		assert_eq!(again, tu, "{src} -> {out} parses differently");
		out
	}

	fn expr(src: &str) -> String {
		let out = rt(&format!("void main(){{x={};}}", src));
		out.strip_prefix("void main(){x=")
			.unwrap()
			.strip_suffix(";}")
			.unwrap()
			.to_string()
	}

	#[test]
	fn expressions_keep_only_necessary_parens() {
		for (src, want) in [
			("a + b * c", "a+b*c"),
			("(a + b) * c", "(a+b)*c"),
			("a * (b + c)", "a*(b+c)"),
			("a - b - c", "a-b-c"),
			("a - (b - c)", "a-(b-c)"),
			("a / (b * c)", "a/(b*c)"),
			("-(a * b)", "-(a*b)"),
			("-a * b", "-a*b"),
			("(-a).x", "(-a).x"),
			("-a.x", "-a.x"),
			("(-a)++", "(-a)++"),
			("-a++", "-a++"),
			("a - -b", "a- -b"),
			("a + +b", "a+ +b"),
			("a - --b", "a- --b"),
			("a++ + b", "a++ +b"),
			("a-- - b", "a-- -b"),
			("a + ++b", "a+ ++b"),
			("- -a", "- -a"),
			("-(-a)", "- -a"),
			("a ? b : c", "a?b:c"),
			("a ? b : c ? d : e", "a?b:c?d:e"),
			("(a ? b : c) ? d : e", "(a?b:c)?d:e"),
			("a || b ? c : d", "a||b?c:d"),
			("(a = b) ? c : d", "(a=b)?c:d"),
			("a ? b, c : d", "a?b,c:d"),
			("a ? b : c = d", "a?b:c=d"),
			("a = b = c", "a=b=c"),
			("(a = b) = c", "(a=b)=c"),
			("a += b * c", "a+=b*c"),
			("a | b | c", "a|b|c"),
			("(a | b) | c", "(a|b)|c"),
			("a & b & c", "a&b&c"),
			("a ^ b | c", "a^b|c"),
			("a | b ^ c", "a|b^c"),
			("a && b && c", "a&&b&&c"),
			("a && (b && c)", "a&&(b&&c)"),
			("a == b == c", "a==b==c"),
			("a < b == c", "a<b==c"),
			("a << 1 + b", "a<<1+b"),
			("f(a, b)", "f(a,b)"),
			("f((a, b))", "f((a,b))"),
			("f(a = 1)", "f(a=1)"),
			("float[2](1., 2.)", "float[2](1.,2.)"),
			("a.length()", "a.length()"),
			("a[1][2]", "a[1][2]"),
			("a[b ? 1 : 2]", "a[b?1:2]"),
			("a[(b = 1)]", "a[(b=1)]"),
			("(a, b), c", "(a,b),c"),
			("a, b, c", "a,b,c"),
			("v.xyz", "v.xyz"),
			("vec4(1.0, 0.5, 0.0, 1e3)", "vec4(1.,.5,0.,1e3)"),
			("!a", "!a"),
			("!(a && b)", "!(a&&b)"),
			("~a", "~a"),
			("3u + 0x10", "3u+16"),
			("0xFFFFFFFF", "0xFFFFFFFF"),
			("010", "8"),
			("true && false", "true&&false"),
			("1.5lf", "1.5lf"),
		] {
			assert_eq!(expr(src), want, "{src}");
		}
	}

	#[test]
	fn statements_and_declarations() {
		for (src, want) in [
			("void main(){if(a)x;else y;}", "void main(){if(a)x;else y;}"),
			(
				"void main(){if(a){x;}else{y;}}",
				"void main(){if(a){x;}else{y;}}",
			),
			(
				"void main(){if(a)x;else if(b)y;else z;}",
				"void main(){if(a)x;else if(b)y;else z;}",
			),
			(
				"void main(){if(a){if(b)x;}else y;}",
				"void main(){if(a){if(b)x;}else y;}",
			),
			("void main(){do x;while(c);}", "void main(){do x;while(c);}"),
			(
				"void main(){do{x;}while(c);}",
				"void main(){do{x;}while(c);}",
			),
			("void main(){for(;;);}", "void main(){for(;;);}"),
			(
				"void main(){for(int i=0;i<3;i++){x;}}",
				"void main(){for(int i=0;i<3;i++){x;}}",
			),
			(
				"void main(){for(i=0;i<3;++i)x;}",
				"void main(){for(i=0;i<3;++i)x;}",
			),
			("void main(){while(a)x;}", "void main(){while(a)x;}"),
			(
				"void main(){switch(a){case 1:x;break;default:y;}}",
				"void main(){switch(a){case 1:x;break;default:y;}}",
			),
			("void main(){return;}", "void main(){return;}"),
			("void main(){return -1;}", "void main(){return-1;}"),
			("void main(){return 1.;}", "void main(){return 1.;}"),
			("void main(){return .5;}", "void main(){return.5;}"),
			(
				"void main(){discard;continue;break;}",
				"void main(){discard;continue;break;}",
			),
			("void main(){{}}", "void main(){{}}"),
			("void main(){;}", "void main(){;}"),
			("void main(){a;}", "void main(){a;}"),
			("void main(void){}", "void main(void){}"),
			(
				"float f(float a, in vec2 b[2], out float c);",
				"float f(float a,in vec2 b[2],out float c);",
			),
			(
				"float a, b[2] = float[2](1., 2.);",
				"float a,b[2]=float[2](1.,2.);",
			),
			(
				"const float a[] = float[](0., 1.);",
				"const float a[]=float[](0.,1.);",
			),
			(
				"layout(location = 0) in vec4 p;",
				"layout(location=0)in vec4 p;",
			),
			(
				"layout(std140, binding = 1) uniform B { vec4 a; float b[2]; } inst;",
				"layout(std140,binding=1)uniform B{vec4 a;float b[2];}inst;",
			),
			("uniform B { vec4 a; };", "uniform B{vec4 a;};"),
			(
				"struct S { int a; float b, c; };",
				"struct S{int a;float b,c;};",
			),
			(
				"const struct S { int a; } s = S(1);",
				"const struct S{int a;}s=S(1);",
			),
			("struct { int a; } s;", "struct{int a;}s;"),
			("precision mediump float;", "precision mediump float;"),
			("highp vec4 c;", "highp vec4 c;"),
			("invariant gl_Position;", "invariant gl_Position;"),
			("flat centroid in vec3 n;", "flat centroid in vec3 n;"),
			(
				"subroutine(T) float f(float a){return a;}",
				"subroutine(T)float f(float a){return a;}",
			),
			("int a[2] = {1, 2};", "int a[2]={1,2};"),
			(
				"void main(){float x = (a, b);}",
				"void main(){float x=(a,b);}",
			),
			("void main(){int x = x;}", "void main(){int x=x;}"),
			("void main(){float a[2][3];}", "void main(){float a[2][3];}"),
		] {
			assert_eq!(rt(src), want, "{src}");
		}
	}

	#[test]
	fn preprocessor_lines_stay_on_their_own_lines() {
		let src = "#version 300 es\n#extension GL_ARB_x : enable\n#extension all : warn\n#define X\n#define Y 1 + 2\n#define SQ(v) ((v)*(v))\n#define F(a, b)\n#ifdef X\n#undef X\n#elif defined(Y)\n#else\n#endif\n#if 1\n#endif\n#error no way\n#pragma optionNV(fastmath)\n#line 3 4\n#line 5\nfloat a;\n#ifndef Z\nvoid main(){}\n#endif\n";
		let want = "#version 300 es\n#extension GL_ARB_x : enable\n#extension all : warn\n#define X\n#define Y 1 + 2\n#define SQ(v) ((v)*(v))\n#define F(a,b)\n#ifdef X\n#undef X\n#elif defined(Y)\n#else\n#endif\n#if 1\n#endif\n#error no way\n#pragma optionNV(fastmath)\n#line 3 4\n#line 5\nfloat a;\n#ifndef Z\nvoid main(){}\n#endif";
		assert_eq!(rt(src), want);
		assert_eq!(
			rt("#version 110\nvoid main(){}"),
			"#version 110\nvoid main(){}"
		);
		assert_eq!(rt("#version 110"), "#version 110");
		assert_eq!(
			rt("#version 110\n#version 120"),
			"#version 110\n#version 120"
		);
	}

	#[test]
	fn float_literals_are_shortest_roundtrip() {
		for (x, want) in [
			(1.0, "1."),
			(0.5, ".5"),
			(0.0, "0."),
			(-0.0, "0."),
			(1.5, "1.5"),
			(1.1, "1.1"),
			(100.0, "1e2"),
			(1000000.0, "1e6"),
			(1500000.0, "15e5"),
			(0.0000001, "1e-7"),
			(0.001, ".001"),
			(123456.0, "123456."),
			(16777216.0, "16777216."),
			(0.1, ".1"),
			(3.1415927, "3.1415927"),
			(f32::MAX, "34028235e31"),
			(f32::MIN_POSITIVE, "11754944e-45"),
			(1.0e-45, "1e-45"),
		] {
			let got = format_float(x);
			assert_eq!(got, want, "{x}");
			assert_eq!(
				parser_reads(&got).parse::<f32>().unwrap(),
				x,
				"{x} -> {got}"
			);
		}
		assert_eq!(format_float(-2.5), "-2.5");
		assert_eq!(format_double(1.0), "1.lf");
		assert_eq!(format_double(0.1), ".1lf");
		assert_eq!(format_double(1.0e300), "1e300lf");
		// every candidate form is something both Rust and the parser accept
		for lit in ["1.", ".5", "1e3", "1.e3", ".5e1", "15e5", "1.5e-10"] {
			assert!(parser_reads(lit).parse::<f32>().is_ok(), "{lit}");
		}
	}

	#[test]
	fn negative_int_constants_print_as_hex() {
		assert_eq!(expr("-1"), "-1");
		let tu = TranslationUnit::parse("int a=0xFFFFFFFF;").unwrap();
		match &tu.0 .0[0] {
			ExternalDeclaration::Declaration(Declaration::InitDeclaratorList(l)) => {
				assert_eq!(
					l.head.initializer,
					Some(Initializer::Simple(Box::new(Expr::IntConst(-1))))
				);
			},
			other => panic!("{other:?}"),
		}
		assert_eq!(rt("int a=0xFFFFFFFF;"), "int a=0xFFFFFFFF;");
	}
}
