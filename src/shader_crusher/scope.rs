//! Identifier resolution: builds the scope tree and symbol table of a shader
//! and binds every identifier occurrence to its symbol.
//!
//! Binding is done by rewriting the identifier string in the AST to a
//! *sentinel*: a single private-use character encoding the symbol id. Every
//! later stage recognises symbols by `sentinel_id`; `rename::apply` writes
//! the final names back.
//!
//! Scoping follows the strictest reading over all GLSL / GLSL ES versions:
//! - variables, functions and struct type names share one namespace per
//!   scope; a nested scope may hide any outer name;
//! - function parameters and the function body form one scope;
//! - a `for`/`while` header and its body form one scope; `do` bodies, `switch`
//!   bodies, unbraced `if`/`else` branches and `{}` blocks are scopes;
//! - a declared name becomes visible only after its initializer;
//! - struct members live in a namespace per struct; interface block names and
//!   members are API-visible and pinned; swizzles are never identifiers.

use std::collections::{HashMap, HashSet};

use super::builtins::{is_builtin_function, is_keyword, is_reserved, is_swizzle};
use super::protect::Protection;
use super::CrushError;
use crate::glsl::syntax::*;

pub type SymbolId = u32;
pub type ScopeId = u32;
pub type StructId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
	Variable,
	Parameter,
	Function,
	StructType,
	BlockInstance,
	Field(StructId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeRef {
	/// A built-in type (scalar, vector, sampler, ...) or a built-in struct.
	Builtin,
	UserStruct(StructId),
	/// Unknown: a type name that could not be resolved.
	Opaque,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinReason {
	/// Protected by the caller, a pragma region or preprocessor text.
	Protected,
	/// `gl_`/`GL_` prefix or `__`.
	Reserved,
	/// A user function with the name of a built-in function.
	BuiltinShadow,
	BlockMember,
	Subroutine,
	/// Member of a struct that is reachable from a pinned variable.
	ApiStruct,
	/// A field name that is also selected on a value of unknown type.
	FieldNameOpaque,
	/// A field whose name looks like a swizzle.
	SwizzleShaped,
	/// The same name declared with different kinds in one scope.
	KindClash,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Symbol {
	pub name:        String,
	pub kind:        SymbolKind,
	pub scope:       ScopeId,
	pub ty:          TypeRef,
	pub pinned:      Option<PinReason>,
	pub occurrences: u32,
	pub new_name:    Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructDef {
	pub type_symbol: Option<SymbolId>,
	/// In declaration order.
	pub fields:      Vec<SymbolId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
	Global,
	Function,
	Block,
	Loop,
	Switch,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scope {
	pub parent:     Option<ScopeId>,
	pub kind:       ScopeKind,
	/// In declaration order.
	pub symbols:    Vec<SymbolId>,
	pub children:   Vec<ScopeId>,
	/// Symbols used anywhere in this scope's subtree.
	pub referenced: HashSet<SymbolId>,
}

/// One identifier occurrence, in traversal order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Occ {
	pub sym:     SymbolId,
	pub is_decl: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SymbolTable {
	pub symbols:            Vec<Symbol>,
	pub scopes:             Vec<Scope>,
	pub structs:            Vec<StructDef>,
	/// Every binding in traversal order: the fingerprint the self-check
	/// compares after re-parsing the output.
	pub occ:                Vec<Occ>,
	/// Names that must keep their spelling (and must not be generated).
	pub pinned_names:       HashSet<String>,
	/// Field names that must keep their spelling in every struct.
	pub pinned_field_names: HashSet<String>,
}

const SENTINEL_BASE: u32 = 0xF0000;
const SENTINEL_MAX: u32 = 0xFFFFD;

/// The placeholder identifier for symbol `id`.
pub fn sentinel(id: SymbolId) -> String {
	char::from_u32(SENTINEL_BASE + id)
		.expect("sentinel in range")
		.to_string()
}

/// The symbol id encoded by a placeholder identifier, if `s` is one.
pub fn sentinel_id(s: &str) -> Option<SymbolId> {
	let mut it = s.chars();
	let c = it.next()?;
	if it.next().is_some() {
		return None;
	}
	let u = c as u32;
	if (SENTINEL_BASE..=SENTINEL_MAX).contains(&u) {
		Some(u - SENTINEL_BASE)
	} else {
		None
	}
}

impl SymbolTable {
	pub fn new_name_or_original(&self, id: SymbolId) -> &str {
		let s = &self.symbols[id as usize];
		s.new_name.as_deref().unwrap_or(&s.name)
	}

	/// Per-symbol listing for `--verbose`.
	pub fn dump(&self) {
		eprintln!(
			"{: <24} {: <6} {: <14} {: >5} {: >4}  pin",
			"symbol", "new", "kind", "scope", "uses"
		);
		for s in &self.symbols {
			eprintln!(
				"{: <24} {: <6} {: <14} {: >5} {: >4}  {}",
				s.name,
				s.new_name.as_deref().unwrap_or("-"),
				format!("{:?}", s.kind),
				s.scope,
				s.occurrences,
				s.pinned.map(|p| format!("{:?}", p)).unwrap_or_default()
			);
		}
	}
}

/// What a `.member` selector is applied to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Base {
	Struct(StructId),
	/// A built-in type: the selector is a swizzle or a built-in member.
	NotStruct,
	Unknown,
}

struct Resolver<'a> {
	t:           SymbolTable,
	prot:        &'a Protection,
	cur:         ScopeId,
	scope_names: Vec<HashMap<String, SymbolId>>,
	field_names: Vec<HashMap<String, SymbolId>>,
	use_sites:   Vec<(SymbolId, ScopeId)>,
}

type R = Result<(), CrushError>;

impl<'a> Resolver<'a> {
	fn new(prot: &'a Protection) -> Self {
		let mut t = SymbolTable::default();
		t.scopes.push(Scope {
			parent:     None,
			kind:       ScopeKind::Global,
			symbols:    Vec::new(),
			children:   Vec::new(),
			referenced: HashSet::new(),
		});
		t.pinned_names = prot.names.clone();
		t.pinned_field_names = prot.field_names.clone();
		Resolver {
			t,
			prot,
			cur: 0,
			scope_names: vec![HashMap::new()],
			field_names: Vec::new(),
			use_sites: Vec::new(),
		}
	}

	fn push_scope(&mut self, kind: ScopeKind) {
		let id = self.t.scopes.len() as ScopeId;
		self.t.scopes.push(Scope {
			parent: Some(self.cur),
			kind,
			symbols: Vec::new(),
			children: Vec::new(),
			referenced: HashSet::new(),
		});
		self.scope_names.push(HashMap::new());
		self.t.scopes[self.cur as usize].children.push(id);
		self.cur = id;
	}

	fn pop_scope(&mut self) {
		self.cur = self.t.scopes[self.cur as usize]
			.parent
			.expect("not the global scope");
	}

	fn new_struct(&mut self) -> StructId {
		self.t.structs.push(StructDef {
			type_symbol: None,
			fields:      Vec::new(),
		});
		self.field_names.push(HashMap::new());
		(self.t.structs.len() - 1) as StructId
	}

	fn pin(&mut self, id: SymbolId, reason: PinReason) {
		let s = &mut self.t.symbols[id as usize];
		if s.pinned.is_none() {
			s.pinned = Some(reason);
		}
	}

	fn new_symbol(
		&mut self,
		name: &str,
		kind: SymbolKind,
		scope: ScopeId,
		ty: TypeRef,
	) -> Result<SymbolId, CrushError> {
		let id = self.t.symbols.len() as u32;
		if SENTINEL_BASE + id > SENTINEL_MAX {
			return Err(CrushError::TooManySymbols(id as usize));
		}
		self.t.symbols.push(Symbol {
			name: name.to_string(),
			kind,
			scope,
			ty,
			pinned: None,
			occurrences: 0,
			new_name: None,
		});
		Ok(id)
	}

	/// Declare `name` in `scope`; a redeclaration in the same scope (prototype
	/// and definition, overloads, `#if` duplicates) yields the existing symbol.
	fn declare(
		&mut self,
		scope: ScopeId,
		name: &str,
		kind: SymbolKind,
		ty: TypeRef,
	) -> Result<SymbolId, CrushError> {
		if let Some(&id) = self.scope_names[scope as usize].get(name) {
			let s = &mut self.t.symbols[id as usize];
			if s.kind != kind {
				if s.pinned.is_none() {
					s.pinned = Some(PinReason::KindClash);
				}
			} else if kind == SymbolKind::Function && s.ty != ty {
				s.ty = TypeRef::Opaque;
			}
			return Ok(id);
		}
		let id = self.new_symbol(name, kind, scope, ty)?;
		self.scope_names[scope as usize].insert(name.to_string(), id);
		self.t.scopes[scope as usize].symbols.push(id);
		Ok(id)
	}

	fn declare_field(
		&mut self,
		def: StructId,
		name: &str,
		ty: TypeRef,
	) -> Result<SymbolId, CrushError> {
		if let Some(&id) = self.field_names[def as usize].get(name) {
			return Ok(id);
		}
		let id = self.new_symbol(name, SymbolKind::Field(def), self.cur, ty)?;
		self.field_names[def as usize].insert(name.to_string(), id);
		self.t.structs[def as usize].fields.push(id);
		Ok(id)
	}

	fn lookup(&self, name: &str) -> Option<SymbolId> {
		let mut scope = Some(self.cur);
		while let Some(id) = scope {
			if let Some(&s) = self.scope_names[id as usize].get(name) {
				return Some(s);
			}
			scope = self.t.scopes[id as usize].parent;
		}
		None
	}

	fn bind(&mut self, slot: &mut String, sym: SymbolId, is_decl: bool) {
		self.t.symbols[sym as usize].occurrences += 1;
		self.t.occ.push(Occ { sym, is_decl });
		self.use_sites.push((sym, self.cur));
		*slot = sentinel(sym);
	}

	/// A name that is neither declared nor owned by the language: it must
	/// keep its spelling everywhere (a macro may declare it).
	fn pin_name(&mut self, name: &str) {
		if !(is_keyword(name) || is_builtin_function(name) || is_reserved(name)) {
			self.t.pinned_names.insert(name.to_string());
		}
	}

	fn use_ident(&mut self, slot: &mut String) {
		match self.lookup(slot) {
			Some(id) => self.bind(slot, id, false),
			None => self.pin_name(&slot.clone()),
		}
	}

	fn type_qualifier(&mut self, q: &mut TypeQualifier) -> R {
		for spec in &mut q.qualifiers.0 {
			match spec {
				TypeQualifierSpec::Storage(StorageQualifier::Subroutine(names)) => {
					for n in names {
						self.t.pinned_names.insert(n.0.clone());
					}
				},
				TypeQualifierSpec::Layout(l) => {
					for id in &mut l.ids.0 {
						if let LayoutQualifierSpec::Identifier(_, Some(e)) = id {
							self.expr(e)?;
						}
					}
				},
				_ => {},
			}
		}
		Ok(())
	}

	fn has_subroutine(q: &Option<TypeQualifier>) -> bool {
		q.as_ref().is_some_and(|q| {
			q.qualifiers.0.iter().any(|s| {
				matches!(
					s,
					TypeQualifierSpec::Storage(StorageQualifier::Subroutine(_))
				)
			})
		})
	}

	fn array_spec(&mut self, spec: &mut ArraySpecifier) -> R {
		for d in &mut spec.dimensions.0 {
			if let ArraySpecifierDimension::ExplicitlySized(e) = d {
				self.expr(e)?;
			}
		}
		Ok(())
	}

	fn opt_array_spec(&mut self, spec: &mut Option<ArraySpecifier>) -> R {
		if let Some(spec) = spec {
			self.array_spec(spec)?;
		}
		Ok(())
	}

	fn struct_fields(&mut self, def: StructId, fields: &mut [StructFieldSpecifier]) -> R {
		for f in fields {
			if let Some(q) = &mut f.qualifier {
				self.type_qualifier(q)?;
			}
			let ft = self.type_spec(&mut f.ty)?;
			for id in &mut f.identifiers.0 {
				self.opt_array_spec(&mut id.array_spec)?;
				let fs = self.declare_field(def, &id.ident.0, ft)?;
				self.bind(&mut id.ident.0, fs, true);
			}
		}
		Ok(())
	}

	fn type_spec(&mut self, ts: &mut TypeSpecifier) -> Result<TypeRef, CrushError> {
		let r = match &mut ts.ty {
			TypeSpecifierNonArray::Struct(spec) => {
				let mut def = self.new_struct();
				if let Some(tn) = &mut spec.name {
					let s = self.declare(
						self.cur,
						&tn.0,
						SymbolKind::StructType,
						TypeRef::UserStruct(def),
					)?;
					// a redeclaration (`#if` duplicate) keeps the first definition
					if let TypeRef::UserStruct(existing) = self.t.symbols[s as usize].ty {
						def = existing;
					}
					self.t.structs[def as usize].type_symbol = Some(s);
					self.bind(&mut tn.0, s, true);
				}
				self.struct_fields(def, &mut spec.fields.0)?;
				TypeRef::UserStruct(def)
			},
			TypeSpecifierNonArray::TypeName(tn) => match self.lookup(&tn.0) {
				Some(s) => {
					self.bind(&mut tn.0, s, false);
					match self.t.symbols[s as usize] {
						Symbol {
							kind: SymbolKind::StructType,
							ty,
							..
						} => ty,
						_ => TypeRef::Opaque,
					}
				},
				None => {
					let n = tn.0.clone();
					if is_keyword(&n) || is_reserved(&n) {
						TypeRef::Builtin
					} else {
						self.pin_name(&n);
						TypeRef::Opaque
					}
				},
			},
			_ => TypeRef::Builtin,
		};
		self.opt_array_spec(&mut ts.array_specifier)?;
		Ok(r)
	}

	fn function(
		&mut self,
		proto: &mut FunctionPrototype,
		body: Option<&mut CompoundStatement>,
	) -> R {
		if let Some(q) = &mut proto.ty.qualifier {
			self.type_qualifier(q)?;
		}
		let ret = self.type_spec(&mut proto.ty.ty)?;
		let name = proto.name.0.clone();
		let sym = self.declare(0, &name, SymbolKind::Function, ret)?;
		if is_builtin_function(&name) {
			self.pin(sym, PinReason::BuiltinShadow);
		}
		if Self::has_subroutine(&proto.ty.qualifier) {
			self.pin(sym, PinReason::Subroutine);
		}
		self.bind(&mut proto.name.0, sym, true);
		self.push_scope(ScopeKind::Function);
		for p in &mut proto.parameters {
			match p {
				FunctionParameterDeclaration::Named(q, d) => {
					if let Some(q) = q {
						self.type_qualifier(q)?;
					}
					let t = self.type_spec(&mut d.ty)?;
					self.opt_array_spec(&mut d.ident.array_spec)?;
					let ps = self.declare(self.cur, &d.ident.ident.0, SymbolKind::Parameter, t)?;
					self.bind(&mut d.ident.ident.0, ps, true);
				},
				FunctionParameterDeclaration::Unnamed(q, ts) => {
					if let Some(q) = q {
						self.type_qualifier(q)?;
					}
					self.type_spec(ts)?;
				},
			}
		}
		if let Some(b) = body {
			for st in &mut b.statement_list {
				self.statement(st)?;
			}
		}
		self.pop_scope();
		Ok(())
	}

	fn initializer(&mut self, i: &mut Initializer) -> R {
		match i {
			Initializer::Simple(e) => self.expr(e),
			Initializer::List(l) => {
				for i in &mut l.0 {
					self.initializer(i)?;
				}
				Ok(())
			},
		}
	}

	fn declaration(&mut self, d: &mut Declaration) -> R {
		match d {
			Declaration::FunctionPrototype(p) => self.function(p, None),
			Declaration::InitDeclaratorList(l) => {
				if let Some(q) = &mut l.head.ty.qualifier {
					self.type_qualifier(q)?;
				}
				let t = self.type_spec(&mut l.head.ty.ty)?;
				let subroutine = Self::has_subroutine(&l.head.ty.qualifier);
				if let Some(name) = &mut l.head.name {
					self.opt_array_spec(&mut l.head.array_specifier)?;
					if let Some(i) = &mut l.head.initializer {
						self.initializer(i)?;
					}
					let s = self.declare(self.cur, &name.0, SymbolKind::Variable, t)?;
					if subroutine {
						self.pin(s, PinReason::Subroutine);
					}
					self.bind(&mut name.0, s, true);
				}
				for td in &mut l.tail {
					self.opt_array_spec(&mut td.ident.array_spec)?;
					if let Some(i) = &mut td.initializer {
						self.initializer(i)?;
					}
					let s = self.declare(self.cur, &td.ident.ident.0, SymbolKind::Variable, t)?;
					if subroutine {
						self.pin(s, PinReason::Subroutine);
					}
					self.bind(&mut td.ident.ident.0, s, true);
				}
				Ok(())
			},
			Declaration::Precision(_, ts) => {
				self.type_spec(ts)?;
				Ok(())
			},
			Declaration::Block(b) => {
				self.type_qualifier(&mut b.qualifier)?;
				self.t.pinned_names.insert(b.name.0.clone());
				let def = self.new_struct();
				self.struct_fields(def, &mut b.fields)?;
				for fs in self.t.structs[def as usize].fields.clone() {
					self.pin(fs, PinReason::BlockMember);
				}
				match &mut b.identifier {
					None => {
						// members are globals, addressed by name through the API
						for fs in self.t.structs[def as usize].fields.clone() {
							let (name, ty) = {
								let f = &self.t.symbols[fs as usize];
								(f.name.clone(), f.ty)
							};
							let gs = self.declare(0, &name, SymbolKind::Variable, ty)?;
							self.pin(gs, PinReason::BlockMember);
						}
					},
					Some(inst) => {
						self.opt_array_spec(&mut inst.array_spec)?;
						let s = self.declare(
							self.cur,
							&inst.ident.0,
							SymbolKind::BlockInstance,
							TypeRef::UserStruct(def),
						)?;
						self.bind(&mut inst.ident.0, s, true);
					},
				}
				Ok(())
			},
			Declaration::Global(q, ids) => {
				self.type_qualifier(q)?;
				for id in ids {
					self.use_ident(&mut id.0);
				}
				Ok(())
			},
		}
	}

	fn condition(&mut self, c: &mut Condition) -> R {
		match c {
			Condition::Expr(e) => self.expr(e),
			Condition::Assignment(ty, name, init) => {
				if let Some(q) = &mut ty.qualifier {
					self.type_qualifier(q)?;
				}
				let t = self.type_spec(&mut ty.ty)?;
				self.initializer(init)?;
				let s = self.declare(self.cur, &name.0, SymbolKind::Variable, t)?;
				self.bind(&mut name.0, s, true);
				Ok(())
			},
		}
	}

	/// A sub-statement that forms a scope of its own when unbraced.
	fn branch(&mut self, st: &mut Statement) -> R {
		if matches!(st, Statement::Compound(_)) {
			self.statement(st)
		} else {
			self.push_scope(ScopeKind::Block);
			let r = self.statement(st);
			self.pop_scope();
			r
		}
	}

	/// A loop body shares the loop header's scope, even when braced.
	fn loop_body(&mut self, st: &mut Statement) -> R {
		match st {
			Statement::Compound(c) => {
				for s in &mut c.statement_list {
					self.statement(s)?;
				}
				Ok(())
			},
			_ => self.statement(st),
		}
	}

	fn statement(&mut self, st: &mut Statement) -> R {
		match st {
			Statement::Compound(c) => {
				self.push_scope(ScopeKind::Block);
				for s in &mut c.statement_list {
					self.statement(s)?;
				}
				self.pop_scope();
				Ok(())
			},
			Statement::Simple(s) => self.simple_statement(s),
		}
	}

	fn simple_statement(&mut self, s: &mut SimpleStatement) -> R {
		match s {
			SimpleStatement::Declaration(d) => self.declaration(d),
			SimpleStatement::Expression(e) => {
				if let Some(e) = e {
					self.expr(e)?;
				}
				Ok(())
			},
			SimpleStatement::Selection(sel) => {
				self.expr(&mut sel.cond)?;
				match &mut sel.rest {
					SelectionRestStatement::Statement(st) => self.branch(st),
					SelectionRestStatement::Else(a, b) => {
						self.branch(a)?;
						self.branch(b)
					},
				}
			},
			SimpleStatement::Switch(sw) => {
				self.expr(&mut sw.head)?;
				self.push_scope(ScopeKind::Switch);
				for st in &mut sw.body {
					self.statement(st)?;
				}
				self.pop_scope();
				Ok(())
			},
			SimpleStatement::CaseLabel(CaseLabel::Case(e)) => self.expr(e),
			SimpleStatement::CaseLabel(CaseLabel::Def) => Ok(()),
			SimpleStatement::Iteration(IterationStatement::While(c, body)) => {
				self.push_scope(ScopeKind::Loop);
				self.condition(c)?;
				self.loop_body(body)?;
				self.pop_scope();
				Ok(())
			},
			SimpleStatement::Iteration(IterationStatement::DoWhile(body, e)) => {
				self.branch(body)?;
				self.expr(e)
			},
			SimpleStatement::Iteration(IterationStatement::For(init, rest, body)) => {
				self.push_scope(ScopeKind::Loop);
				match init {
					ForInitStatement::Expression(Some(e)) => self.expr(e)?,
					ForInitStatement::Expression(None) => {},
					ForInitStatement::Declaration(d) => self.declaration(d)?,
				}
				if let Some(c) = &mut rest.condition {
					self.condition(c)?;
				}
				if let Some(e) = &mut rest.post_expr {
					self.expr(e)?;
				}
				self.loop_body(body)?;
				self.pop_scope();
				Ok(())
			},
			SimpleStatement::Jump(JumpStatement::Return(Some(e))) => self.expr(e),
			SimpleStatement::Jump(_) => Ok(()),
		}
	}

	fn symbol_base(&self, id: SymbolId) -> Base {
		match self.t.symbols[id as usize].ty {
			TypeRef::UserStruct(d) => Base::Struct(d),
			TypeRef::Builtin => Base::NotStruct,
			TypeRef::Opaque => Base::Unknown,
		}
	}

	/// The struct type (if any) of an already-resolved expression.
	fn classify(&self, e: &Expr) -> Base {
		match e {
			Expr::Variable(s) => match sentinel_id(&s.0) {
				Some(id) => self.symbol_base(id),
				None => {
					if is_reserved(&s.0) {
						Base::NotStruct
					} else {
						Base::Unknown
					}
				},
			},
			Expr::IntConst(_)
			| Expr::UIntConst(_)
			| Expr::BoolConst(_)
			| Expr::FloatConst(_)
			| Expr::DoubleConst(_)
			| Expr::Unary(..)
			| Expr::Binary(..)
			| Expr::PostInc(_)
			| Expr::PostDec(_) => Base::NotStruct,
			Expr::Bracket(e, _) => self.classify(e),
			Expr::Dot(e, f) => match self.classify(e) {
				Base::Struct(_) => match sentinel_id(&f.0) {
					Some(id) => self.symbol_base(id),
					None => Base::NotStruct, // swizzle
				},
				Base::NotStruct => Base::NotStruct,
				Base::Unknown => Base::Unknown,
			},
			Expr::FunCall(FunIdentifier::Identifier(s), _) => match sentinel_id(&s.0) {
				Some(id) => self.symbol_base(id),
				None => {
					if is_keyword(&s.0) || is_builtin_function(&s.0) {
						Base::NotStruct
					} else {
						Base::Unknown
					}
				},
			},
			Expr::FunCall(FunIdentifier::Expr(inner), _) => match &**inner {
				Expr::Bracket(b, _) => match &**b {
					Expr::Variable(s) => match sentinel_id(&s.0) {
						Some(id) => self.symbol_base(id),
						None => {
							if is_keyword(&s.0) {
								Base::NotStruct
							} else {
								Base::Unknown
							}
						},
					},
					_ => Base::Unknown,
				},
				Expr::Dot(..) => Base::NotStruct, // `.length()`
				_ => Base::Unknown,
			},
			Expr::Ternary(_, a, b) => match (self.classify(a), self.classify(b)) {
				(Base::Struct(x), Base::Struct(y)) if x == y => Base::Struct(x),
				(Base::NotStruct, Base::NotStruct) => Base::NotStruct,
				_ => Base::Unknown,
			},
			Expr::Assignment(l, _, _) => self.classify(l),
			Expr::Comma(_, b) => self.classify(b),
		}
	}

	fn expr(&mut self, e: &mut Expr) -> R {
		match e {
			Expr::Variable(i) => {
				self.use_ident(&mut i.0);
				Ok(())
			},
			Expr::IntConst(_)
			| Expr::UIntConst(_)
			| Expr::BoolConst(_)
			| Expr::FloatConst(_)
			| Expr::DoubleConst(_) => Ok(()),
			Expr::Unary(_, e) | Expr::PostInc(e) | Expr::PostDec(e) => self.expr(e),
			Expr::Binary(_, l, r) | Expr::Assignment(l, _, r) | Expr::Comma(l, r) => {
				self.expr(l)?;
				self.expr(r)
			},
			Expr::Ternary(a, b, c) => {
				self.expr(a)?;
				self.expr(b)?;
				self.expr(c)
			},
			Expr::Bracket(e, spec) => {
				self.expr(e)?;
				self.array_spec(spec)
			},
			Expr::FunCall(fi, args) => {
				match fi {
					FunIdentifier::Identifier(i) => match self.lookup(&i.0) {
						Some(s) => self.bind(&mut i.0, s, false),
						None => self.pin_name(&i.0.clone()),
					},
					FunIdentifier::Expr(inner) => match &mut **inner {
						// `a.length()`: the method name is not a field
						Expr::Dot(base, _) => self.expr(base)?,
						other => self.expr(other)?,
					},
				}
				for a in args {
					self.expr(a)?;
				}
				Ok(())
			},
			Expr::Dot(base, f) => {
				self.expr(base)?;
				match self.classify(base) {
					// a struct has no swizzles: every selector is a field, whatever its shape
					Base::Struct(def) => match self.field_names[def as usize].get(&f.0).copied() {
						Some(fs) => self.bind(&mut f.0, fs, false),
						None => {
							self.t.pinned_field_names.insert(f.0.clone());
						},
					},
					// a swizzle or a member of a built-in struct
					Base::NotStruct => {},
					Base::Unknown => {
						self.t.pinned_field_names.insert(f.0.clone());
					},
				}
				Ok(())
			},
		}
	}

	fn finalize(mut self) -> SymbolTable {
		for &(sym, sc) in &self.use_sites {
			let mut s = Some(sc);
			while let Some(id) = s {
				let scope = &mut self.t.scopes[id as usize];
				scope.referenced.insert(sym);
				s = scope.parent;
			}
		}
		let _ = self.prot;
		for id in 0..self.t.symbols.len() {
			let (name, kind) = {
				let s = &self.t.symbols[id];
				(s.name.clone(), s.kind)
			};
			let id = id as SymbolId;
			if let SymbolKind::Field(_) = kind {
				if self.t.pinned_field_names.contains(&name) {
					self.pin(id, PinReason::FieldNameOpaque);
				}
				if is_swizzle(&name) {
					self.pin(id, PinReason::SwizzleShaped);
				}
			} else if self.t.pinned_names.contains(&name) {
				self.pin(id, PinReason::Protected);
			}
			if is_reserved(&name) {
				self.pin(id, PinReason::Reserved);
			}
		}
		// a pinned variable of struct type is addressed as `name.field` by the
		// application: its fields (recursively) keep their names too
		let mut work: Vec<StructId> = self
			.t
			.symbols
			.iter()
			.filter(|s| s.pinned.is_some() && !matches!(s.kind, SymbolKind::StructType))
			.filter_map(|s| match s.ty {
				TypeRef::UserStruct(d) => Some(d),
				_ => None,
			})
			.collect();
		let mut done: HashSet<StructId> = HashSet::new();
		while let Some(def) = work.pop() {
			if !done.insert(def) {
				continue;
			}
			for fs in self.t.structs[def as usize].fields.clone() {
				self.pin(fs, PinReason::ApiStruct);
				if let TypeRef::UserStruct(d) = self.t.symbols[fs as usize].ty {
					work.push(d);
				}
			}
		}
		self.t
	}
}

/// Resolve every identifier of `tu` (rewriting bound identifiers to
/// sentinels) and return the symbol table.
pub fn resolve(tu: &mut TranslationUnit, prot: &Protection) -> Result<SymbolTable, CrushError> {
	let mut r = Resolver::new(prot);
	for ed in &mut tu.0 .0 {
		match ed {
			ExternalDeclaration::Preprocessor(_) => {},
			ExternalDeclaration::FunctionDefinition(fd) => {
				r.function(&mut fd.prototype, Some(&mut fd.statement))?
			},
			ExternalDeclaration::Declaration(d) => r.declaration(d)?,
		}
	}
	Ok(r.finalize())
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::glsl::parser::Parse;

	fn table(src: &str) -> SymbolTable {
		let mut tu = TranslationUnit::parse(src).expect("parse");
		resolve(&mut tu, &Protection::default()).expect("resolve")
	}

	fn sym<'a>(t: &'a SymbolTable, name: &str) -> Vec<&'a Symbol> {
		t.symbols.iter().filter(|s| s.name == name).collect()
	}

	#[test]
	fn sentinel_roundtrip() {
		assert_eq!(sentinel_id(&sentinel(0)), Some(0));
		assert_eq!(sentinel_id(&sentinel(65533)), Some(65533));
		assert_eq!(sentinel_id("a"), None);
		assert_eq!(sentinel_id(""), None);
		assert_eq!(
			sentinel_id(&format!("{}{}", sentinel(1), sentinel(2))),
			None
		);
	}

	#[test]
	fn locals_hide_globals_and_initializers_see_the_outer_name() {
		let t = table("float a = 1.; void main() { float a = a; }");
		let a = sym(&t, "a");
		assert_eq!(a.len(), 2);
		assert_eq!(a[0].scope, 0);
		assert_ne!(a[1].scope, 0);
		// decl global, decl main, the initializer's use -> outer, then decl local
		let uses: Vec<_> = t.occ.iter().map(|o| (o.sym, o.is_decl)).collect();
		assert_eq!(uses, vec![(0, true), (1, true), (0, false), (2, true)]);
		assert!(t.scopes[a[1].scope as usize].referenced.contains(&0));
	}

	#[test]
	fn loop_header_and_body_share_a_scope_but_branches_do_not() {
		let t = table("void main() { for (int i = 0; i < 2; i++) { int j = i; } int i; if (true) float k; else float k; do float m; while (true); while (true) float n; switch (1) { case 1: int c; } }");
		let i = sym(&t, "i");
		assert_eq!(i.len(), 2, "loop i and the later i are distinct");
		let j = sym(&t, "j")[0];
		assert_eq!(
			j.scope, i[0].scope,
			"body declaration lives in the loop scope"
		);
		assert_eq!(t.scopes[j.scope as usize].kind, ScopeKind::Loop);
		let k = sym(&t, "k");
		assert_eq!(k.len(), 2);
		assert_ne!(k[0].scope, k[1].scope);
		assert_eq!(
			t.scopes[sym(&t, "m")[0].scope as usize].kind,
			ScopeKind::Block
		);
		assert_eq!(
			t.scopes[sym(&t, "n")[0].scope as usize].kind,
			ScopeKind::Loop
		);
		assert_eq!(
			t.scopes[sym(&t, "c")[0].scope as usize].kind,
			ScopeKind::Switch
		);
	}

	#[test]
	fn structs_have_their_own_field_namespace() {
		let t = table("struct S { float k; vec3 x; struct T { float k; } t; }; float k; uniform S s; void main() { k = s.k + s.t.k + s.x.y + s.x.x; }");
		let k = sym(&t, "k");
		assert_eq!(k.len(), 3, "field S.k, field T.k, global k");
		assert!(matches!(k[0].kind, SymbolKind::Field(0)));
		assert!(matches!(k[1].kind, SymbolKind::Field(1)));
		assert_eq!(k[2].kind, SymbolKind::Variable);
		assert_eq!(k[0].occurrences, 2, "decl + s.k");
		assert_eq!(k[1].occurrences, 2, "decl + s.t.k");
		assert_eq!(k[2].occurrences, 2, "decl + assignment");
		let x = sym(&t, "x")[0];
		assert_eq!(x.pinned, Some(PinReason::SwizzleShaped));
		assert_eq!(
			x.occurrences, 3,
			"s.x is a field access even though `x` looks like a swizzle"
		);
		let tf = sym(&t, "t")[0];
		assert_eq!(tf.pinned, Some(PinReason::SwizzleShaped));
		assert_eq!(
			tf.occurrences, 2,
			"s.t.k resolves through the swizzle-shaped field"
		);
		let tt = sym(&t, "T")[0];
		assert_eq!(
			tt.scope, 0,
			"nested struct type is declared in the enclosing scope"
		);
		assert_eq!(sym(&t, "S")[0].kind, SymbolKind::StructType);
	}

	#[test]
	fn unknown_bases_pin_field_names_builtin_bases_do_not() {
		let t = table("struct L { vec4 position; float diffuse; }; uniform L l; void main() { gl_FragColor = l.position * l.diffuse + gl_LightSource[0].position + gl_FrontMaterial.diffuse; }");
		assert!(
			sym(&t, "position")[0].pinned.is_none(),
			"gl_LightSource is a builtin base"
		);
		assert!(sym(&t, "diffuse")[0].pinned.is_none());
		let t = table("Unknown u; struct L { float diffuse; }; L l; void main() { float f = u.diffuse + l.diffuse; }");
		assert_eq!(
			sym(&t, "diffuse")[0].pinned,
			Some(PinReason::FieldNameOpaque)
		);
		assert!(t.pinned_names.contains("Unknown"));
	}

	#[test]
	fn functions_merge_prototypes_definitions_and_overloads() {
		let t = table("float f(float a); float f(float a) { return a; } float f(vec2 a) { return a.x; } float dot(float a, float b) { return a * b; } void main() { f(1.); f(vec2(1.)); dot(1., 2.); length(vec2(1.)); }");
		let f = sym(&t, "f");
		assert_eq!(f.len(), 1);
		assert_eq!(f[0].occurrences, 5);
		assert_eq!(sym(&t, "a").len(), 4, "a parameter per prototype scope");
		assert_eq!(sym(&t, "dot")[0].pinned, Some(PinReason::BuiltinShadow));
		assert!(sym(&t, "length").is_empty());
	}

	#[test]
	fn blocks_pin_names_and_members_but_not_instances() {
		let t = table("uniform B { vec4 member_a; } inst; uniform C { vec4 member_b; }; void main() { gl_Position = inst.member_a + member_b; }");
		assert!(t.pinned_names.contains("B") && t.pinned_names.contains("C"));
		assert_eq!(sym(&t, "member_a")[0].pinned, Some(PinReason::BlockMember));
		assert!(sym(&t, "member_b")
			.iter()
			.all(|s| s.pinned == Some(PinReason::BlockMember)));
		assert_eq!(sym(&t, "member_b").len(), 2, "field and global");
		let inst = sym(&t, "inst")[0];
		assert_eq!(inst.kind, SymbolKind::BlockInstance);
		assert!(inst.pinned.is_none());
		assert_eq!(sym(&t, "member_a")[0].occurrences, 2);
	}

	#[test]
	fn protected_struct_variables_pin_their_fields_transitively() {
		let mut prot = Protection::default();
		prot.names.insert("light".to_string());
		let mut tu = TranslationUnit::parse("struct In { float k; }; struct L { In inner; float v; }; uniform L light; uniform L other; void main() { gl_FragColor = vec4(light.v + other.inner.k); }").unwrap();
		let t = resolve(&mut tu, &prot).unwrap();
		assert_eq!(sym(&t, "light")[0].pinned, Some(PinReason::Protected));
		assert_eq!(sym(&t, "v")[0].pinned, Some(PinReason::ApiStruct));
		assert_eq!(sym(&t, "inner")[0].pinned, Some(PinReason::ApiStruct));
		assert_eq!(sym(&t, "k")[0].pinned, Some(PinReason::ApiStruct));
		assert!(sym(&t, "other")[0].pinned.is_none());
	}

	#[test]
	fn unresolved_identifiers_pin_every_symbol_of_that_name() {
		let t = table("void f() { int sum = 1; } void main() { sum = 2; }");
		assert!(t.pinned_names.contains("sum"));
		assert_eq!(sym(&t, "sum")[0].pinned, Some(PinReason::Protected));
	}

	#[test]
	fn nameless_declarations_and_constructors_are_uses() {
		let t = table("struct S { float v; }; varying vec4 Color1; invariant Color1; invariant gl_Position; void main() { S a[2] = S[2](S(1.), S(2.)); float f[2] = float[2](1., 2.); Color1 = vec4(a[0].v, f.length()); a; }");
		assert_eq!(sym(&t, "Color1")[0].occurrences, 3);
		assert_eq!(
			sym(&t, "S")[0].occurrences,
			5,
			"decl, S a, S[2], S(1.), S(2.)"
		);
		assert_eq!(sym(&t, "v")[0].occurrences, 2);
		assert_eq!(sym(&t, "a")[0].occurrences, 3);
		assert!(sym(&t, "length").is_empty());
		assert!(sym(&t, "gl_Position").is_empty());
	}

	#[test]
	fn duplicate_declarations_in_one_scope_are_one_symbol() {
		let t = table("#ifdef X\nuniform float u;\nstruct S { float a; };\n#else\nuniform float u;\nstruct S { float a; };\n#endif\nvoid main() { S s; gl_FragColor = vec4(u + s.a); }");
		assert_eq!(sym(&t, "u").len(), 1);
		assert_eq!(sym(&t, "u")[0].occurrences, 3);
		assert_eq!(sym(&t, "S").len(), 1);
		assert_eq!(
			sym(&t, "a").len(),
			1,
			"fields of the duplicate definition merge"
		);
		assert_eq!(t.structs.len(), 2);
	}

	#[test]
	fn condition_assignment_declares_in_the_loop_scope() {
		let mut tu = TranslationUnit::parse("void main() { while (true) { b; } }").unwrap();
		if let ExternalDeclaration::FunctionDefinition(fd) = &mut tu.0 .0[0] {
			if let Statement::Simple(s) = &mut fd.statement.statement_list[0] {
				if let SimpleStatement::Iteration(IterationStatement::While(c, _)) = &mut **s {
					*c = Condition::Assignment(
						FullySpecifiedType::new(TypeSpecifierNonArray::Bool),
						Identifier("b".to_string()),
						Initializer::Simple(Box::new(Expr::BoolConst(true))),
					);
				}
			}
		}
		let t = resolve(&mut tu, &Protection::default()).unwrap();
		let b = sym(&t, "b")[0];
		assert_eq!(b.kind, SymbolKind::Variable);
		assert_eq!(t.scopes[b.scope as usize].kind, ScopeKind::Loop);
		assert_eq!(b.occurrences, 2);
	}
}
