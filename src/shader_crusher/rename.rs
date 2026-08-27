//! Choose new names for the renamable symbols of a resolved shader and
//! write them back into the AST.
//!
//! Names are handed out per scope in declaration order from a pool of
//! single letters (most frequent letters of the shader first) followed by
//! two-character names. On entering a scope, the names of outer symbols that
//! are not referenced anywhere inside it become available again ("shadowing",
//! spec-legal in every GLSL version); with shadowing off, global names are
//! never reused by locals.
//!
//! Struct fields get names from a separate pool, shared across all structs
//! so that equally named fields of different structs stay equally named.
//!
//! Struct *type* names are allocated first and reserved for the whole shader:
//! GLSL compilers feed declared type names back into the lexer (a visible type
//! name lexes as TYPE_NAME, not IDENTIFIER), so a variable, parameter or field
//! spelled like a type in scope makes the driver read the next statement as a
//! declaration — `struct t{...}; int t=0; while(t<9)` is a syntax error there,
//! even though the GLSL spec allows the shadowing and re-resolving the output
//! binds it correctly.

use std::collections::{HashMap, HashSet};

use super::builtins::{is_swizzle, never_generate};
use super::scope::{sentinel, sentinel_id, ScopeId, SymbolId, SymbolKind, SymbolTable};
use super::Scoring;
use crate::glsl::syntax::*;
use crate::glsl::visitor::{HostMut, Visit, VisitorMut};

const LOWER: &str = "abcdefghijklmnopqrstuvwxyz";
const UPPER: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &str = "0123456789";

/// `a..z` then `A..Z`, most frequent in `text` first (stable for ties).
fn letters_by_frequency(text: &str) -> Vec<char> {
	let mut count: HashMap<char, usize> = HashMap::new();
	for c in text.chars() {
		if c.is_ascii_alphabetic() {
			*count.entry(c).or_default() += 1;
		}
	}
	let mut letters: Vec<char> = LOWER.chars().chain(UPPER.chars()).collect();
	letters.sort_by_key(|c| std::cmp::Reverse(count.get(c).copied().unwrap_or(0)));
	letters
}

/// Counts of adjacent character pairs in the shader text. Symbols appear as
/// their sentinel characters, so a symbol's left and right contexts are the
/// neighbours of one distinct character.
struct Bigrams {
	next_of: HashMap<char, HashMap<char, u32>>,
	prev_of: HashMap<char, HashMap<char, u32>>,
}

impl Bigrams {
	fn new(text: &str) -> Self {
		let mut b = Bigrams {
			next_of: HashMap::new(),
			prev_of: HashMap::new(),
		};
		for (x, y) in text.chars().zip(text.chars().skip(1)) {
			b.add(x, y, 1);
		}
		b
	}

	fn add(&mut self, a: char, b: char, n: u32) {
		*self.next_of.entry(a).or_default().entry(b).or_default() += n;
		*self.prev_of.entry(b).or_default().entry(a).or_default() += n;
	}

	fn count(&self, a: char, b: char) -> u32 {
		self.next_of
			.get(&a)
			.and_then(|m| m.get(&b))
			.copied()
			.unwrap_or(0)
	}

	/// How well `cand` fits the contexts of the symbol printed as `s`: the
	/// frequency of the bigrams its first/last character would form with
	/// each distinct neighbour (`weighted`: times how often that neighbour
	/// occurs), minus a penalty that keeps any single letter ahead of any
	/// longer name.
	fn score(&self, s: char, cand: &str, weighted: bool) -> i64 {
		let first = cand.chars().next().expect("non-empty name");
		let last = cand.chars().next_back().expect("non-empty name");
		let mut score = 0i64;
		let mut occurrences = 0i64;
		if let Some(prev) = self.prev_of.get(&s) {
			for (&c, &w) in prev {
				occurrences += w as i64;
				let v = self.count(c, first) as i64;
				score += if weighted { v * w as i64 } else { v };
			}
		}
		if let Some(next) = self.next_of.get(&s) {
			for (&c, &w) in next {
				let v = self.count(last, c) as i64;
				score += if weighted { v * w as i64 } else { v };
			}
		}
		if cand.chars().count() > 1 {
			let inner = self.count(first, last) as i64;
			score += if weighted { inner * occurrences } else { inner };
			score -= 1000 * if weighted { occurrences.max(1) } else { 1 };
		}
		score
	}

	/// Account for the symbol printed as `s` now being spelled `name`.
	fn fold(&mut self, s: char, name: &str) {
		let first = name.chars().next().expect("non-empty name");
		let last = name.chars().next_back().expect("non-empty name");
		let prev = self.prev_of.remove(&s).unwrap_or_default();
		let next = self.next_of.remove(&s).unwrap_or_default();
		let mut occurrences = 0;
		for (c, w) in prev {
			if let Some(m) = self.next_of.get_mut(&c) {
				m.remove(&s);
			}
			self.add(c, first, w);
			occurrences += w;
		}
		for (c, w) in next {
			if let Some(m) = self.prev_of.get_mut(&c) {
				m.remove(&s);
			}
			self.add(last, c, w);
		}
		if name.chars().count() > 1 {
			self.add(first, last, occurrences);
		}
	}
}

/// Candidates examined per choice (the pool is in preference order).
const CANDIDATES: usize = 26;

struct Renamer<'a> {
	table:       &'a mut SymbolTable,
	letters:     Vec<char>,
	forbidden:   HashSet<String>,
	scoring:     Scoring,
	shadowing:   bool,
	next_triple: usize,
	bigrams:     Bigrams,
}

impl<'a> Renamer<'a> {
	fn allowed(&self, n: &str) -> bool {
		!never_generate(n) && !self.forbidden.contains(n)
	}

	/// Singles then pairs, in preference order.
	fn initial_pool(&self) -> Vec<String> {
		let mut pool: Vec<String> = self.letters.iter().map(|c| c.to_string()).collect();
		let second: Vec<char> = self.letters.iter().copied().chain(DIGITS.chars()).collect();
		for c1 in &self.letters {
			for c2 in &second {
				pool.push(format!("{}{}", c1, c2));
			}
		}
		pool.retain(|n| self.allowed(n));
		pool
	}

	/// Next never-used three-character name (only for shaders with more
	/// simultaneously visible symbols than the pool holds).
	fn triple(&mut self) -> String {
		let second: Vec<char> = self.letters.iter().copied().chain(DIGITS.chars()).collect();
		loop {
			let i = self.next_triple;
			self.next_triple += 1;
			let m = second.len();
			let c3 = second[i % m];
			let c2 = second[(i / m) % m];
			let c1 = self.letters[(i / m / m) % self.letters.len()];
			let n = format!("{}{}{}", c1, c2, c3);
			if self.allowed(&n) {
				return n;
			}
		}
	}

	fn choose(&mut self, sym: SymbolId, avail: &mut Vec<String>) -> String {
		if avail.is_empty() {
			let n = self.triple();
			avail.push(n);
		}
		let weighted = match self.scoring {
			Scoring::Frequency => return avail.remove(0),
			Scoring::Bigram => false,
			Scoring::BigramCount => true,
		};
		let s = sentinel(sym).chars().next().expect("sentinel");
		let mut best = 0;
		let mut best_score = i64::MIN;
		for (i, cand) in avail.iter().take(CANDIDATES).enumerate() {
			let score = self.bigrams.score(s, cand, weighted);
			if score > best_score {
				best_score = score;
				best = i;
			}
		}
		let name = avail.remove(best);
		self.bigrams.fold(s, &name);
		name
	}

	/// Give every renamable struct type a name and take it out of `pool` for
	/// good: no variable, parameter, function or field may be spelled like a
	/// type that is visible to the driver's lexer (see the module doc).
	fn reserve_type_names(&mut self, pool: &mut Vec<String>) {
		let types: Vec<SymbolId> = (0..self.table.symbols.len() as SymbolId)
			.filter(|&i| {
				let s = &self.table.symbols[i as usize];
				s.kind == SymbolKind::StructType && s.pinned.is_none()
			})
			.collect();
		if types.is_empty() {
			return;
		}
		// spellings that survive on struct members: a type must avoid them, but
		// they stay available to variables (a variable never lexes as a type)
		let mut kept_fields = self.table.pinned_field_names.clone();
		for s in &self.table.symbols {
			if matches!(s.kind, SymbolKind::Field(_)) && s.pinned.is_some() {
				kept_fields.insert(s.name.clone());
			}
		}
		let mut type_pool: Vec<String> = pool
			.iter()
			.filter(|n| !kept_fields.contains(*n))
			.cloned()
			.collect();
		let mut chosen: HashSet<String> = HashSet::new();
		for sym in types {
			// the loop only ever repeats for a `triple()`, which is never reused
			let name = loop {
				let n = self.choose(sym, &mut type_pool);
				if !kept_fields.contains(&n) {
					break n;
				}
			};
			self.table.symbols[sym as usize].new_name = Some(name.clone());
			chosen.insert(name);
		}
		pool.retain(|n| !chosen.contains(n));
	}

	fn visit_scope(
		&mut self,
		scope: ScopeId,
		mut avail: Vec<String>,
		mut live: Vec<(SymbolId, String)>,
	) {
		let sc = scope as usize;
		// names of outer symbols not referenced in this subtree are free here
		let mut freed = Vec::new();
		live.retain(|(sym, name)| {
			let referenced = self.table.scopes[sc].referenced.contains(sym);
			let global = self.table.symbols[*sym as usize].scope == 0;
			if referenced || (!self.shadowing && global) {
				true
			} else {
				freed.push(name.clone());
				false
			}
		});
		if !freed.is_empty() {
			freed.extend(avail);
			avail = freed;
		}
		for sym in self.table.scopes[sc].symbols.clone() {
			let s = &self.table.symbols[sym as usize];
			// types are pre-assigned in `assign` and never enter `live`, so their
			// names can never be freed for reuse by an inner scope
			if s.pinned.is_some()
				|| matches!(s.kind, SymbolKind::Field(_))
				|| s.kind == SymbolKind::StructType
			{
				continue;
			}
			let name = self.choose(sym, &mut avail);
			self.table.symbols[sym as usize].new_name = Some(name.clone());
			live.push((sym, name));
		}
		for child in self.table.scopes[sc].children.clone() {
			self.visit_scope(child, avail.clone(), live.clone());
		}
	}

	fn fields(&mut self) {
		let mut forbidden: HashSet<String> = self.table.pinned_field_names.clone();
		for s in &self.table.symbols {
			if matches!(s.kind, SymbolKind::Field(_)) && s.pinned.is_some() {
				forbidden.insert(s.name.clone());
			}
			// `float t;` inside a struct would lex as a type if `t` names one
			if s.kind == SymbolKind::StructType {
				forbidden.insert(s.new_name.clone().unwrap_or_else(|| s.name.clone()));
			}
		}
		let mut pool: Vec<String> = self.letters.iter().map(|c| c.to_string()).collect();
		let second: Vec<char> = self.letters.iter().copied().chain(DIGITS.chars()).collect();
		for c1 in &self.letters {
			for c2 in &second {
				pool.push(format!("{}{}", c1, c2));
			}
		}
		pool.retain(|n| !never_generate(n) && !is_swizzle(n) && !forbidden.contains(n));
		let mut pool = pool.into_iter();
		let mut map: HashMap<String, String> = HashMap::new();
		for def in 0..self.table.structs.len() {
			for fs in self.table.structs[def].fields.clone() {
				let (name, pinned) = {
					let s = &self.table.symbols[fs as usize];
					(s.name.clone(), s.pinned.is_some())
				};
				if pinned {
					continue;
				}
				let new = match map.get(&name) {
					Some(n) => n.clone(),
					None => {
						let n = match pool.next() {
							Some(n) => n,
							None => self.triple(),
						};
						map.insert(name, n.clone());
						n
					},
				};
				self.table.symbols[fs as usize].new_name = Some(new);
			}
		}
	}
}

/// Assign `new_name`s. `text` is the shader printed with sentinels: it
/// provides letter statistics.
pub fn assign(table: &mut SymbolTable, text: &str, scoring: Scoring, shadowing: bool) {
	let letters = letters_by_frequency(text);
	let mut forbidden = table.pinned_names.clone();
	for s in &table.symbols {
		if s.pinned.is_some() && !matches!(s.kind, SymbolKind::Field(_)) {
			forbidden.insert(s.name.clone());
		}
	}
	let mut r = Renamer {
		table,
		letters,
		forbidden,
		scoring,
		shadowing,
		next_triple: 0,
		bigrams: Bigrams::new(text),
	};
	let mut pool = r.initial_pool();
	r.reserve_type_names(&mut pool);
	r.visit_scope(0, pool, Vec::new());
	r.fields();
}

struct Apply<'a> {
	table:       &'a SymbolTable,
	only_pinned: bool,
}

impl<'a> Apply<'a> {
	fn name(&self, s: &mut String) {
		if let Some(id) = sentinel_id(s) {
			if !self.only_pinned || self.table.symbols[id as usize].pinned.is_some() {
				*s = self.table.new_name_or_original(id).to_string();
			}
		}
	}
}

impl<'a> VisitorMut for Apply<'a> {
	fn visit_identifier(&mut self, i: &mut Identifier) -> Visit {
		self.name(&mut i.0);
		Visit::Children
	}
	fn visit_type_name(&mut self, t: &mut TypeName) -> Visit {
		self.name(&mut t.0);
		Visit::Children
	}
}

/// Replace every sentinel by the symbol's new name (or its original name
/// when it has none).
pub fn apply(tu: &mut TranslationUnit, table: &SymbolTable) {
	tu.visit_mut(&mut Apply {
		table,
		only_pinned: false,
	});
}

/// Replace only the sentinels of pinned symbols by their (unchanged) names,
/// so the text used for statistics shows them as they will be printed.
pub fn apply_pinned(tu: &mut TranslationUnit, table: &SymbolTable) {
	tu.visit_mut(&mut Apply {
		table,
		only_pinned: true,
	});
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn letters_sorted_by_frequency_stable() {
		let l = letters_by_frequency("zzz yy x");
		assert_eq!(&l[..3], &['z', 'y', 'x']);
		assert_eq!(l[3], 'a');
		assert_eq!(l.len(), 52);
	}

	use crate::glsl::parser::Parse;
	use crate::shader_crusher::printer::print;
	use crate::shader_crusher::protect::Protection;
	use crate::shader_crusher::scope;
	use crate::shader_crusher::selfcheck::type_names_distinct;

	/// Resolve `src`, assign names with the given options, return the table.
	fn assigned(src: &str, scoring: Scoring, shadowing: bool) -> SymbolTable {
		let mut tu = TranslationUnit::parse(src).expect("parse");
		let mut table = scope::resolve(&mut tu, &Protection::default()).expect("resolve");
		let text = print(&tu);
		assign(&mut table, &text, scoring, shadowing);
		table
	}

	fn finals(
		table: &SymbolTable,
		of: impl Fn(&crate::shader_crusher::scope::Symbol) -> bool,
	) -> Vec<String> {
		table
			.symbols
			.iter()
			.filter(|s| of(s))
			.map(|s| s.new_name.clone().unwrap_or_else(|| s.name.clone()))
			.collect()
	}

	#[test]
	fn type_names_are_never_reused_by_any_symbol() {
		let src = "#version 410\nout vec4 out_color;\nstruct S { float v; };\nfloat f() { int i = 0; while (i < 9) i += 1; return float(i); }\nvoid main() { S s; s.v = f(); out_color = vec4(s.v); }\n";
		for shadowing in [true, false] {
			for scoring in [Scoring::Frequency, Scoring::Bigram, Scoring::BigramCount] {
				let table = assigned(src, scoring, shadowing);
				assert!(
					type_names_distinct(&table).is_ok(),
					"{scoring:?} {shadowing}"
				);
				let types = finals(&table, |s| s.kind == SymbolKind::StructType);
				assert_eq!(types.len(), 1);
				let others = finals(&table, |s| s.kind != SymbolKind::StructType);
				assert!(
					!others.contains(&types[0]),
					"{scoring:?} {shadowing}: type {} also used by {others:?}",
					types[0]
				);
			}
		}
	}

	#[test]
	fn local_struct_type_does_not_take_a_freed_outer_name() {
		// `unused_outer` is not referenced inside main, so its name is freed
		// there; the struct type declared in main must not receive it
		let table = assigned(
			"uniform float unused_outer; void main() { struct L { float a; } l; l.a = 1.; gl_FragColor = vec4(l.a); }",
			Scoring::BigramCount,
			true,
		);
		assert!(type_names_distinct(&table).is_ok());
		let ty = finals(&table, |s| s.kind == SymbolKind::StructType);
		let outer = finals(&table, |s| s.name == "unused_outer");
		assert_ne!(ty[0], outer[0], "type took the freed outer name");
	}

	#[test]
	fn fields_are_never_spelled_like_a_type() {
		// `x` is a swizzle-shaped field: it keeps its name, so no type may take it
		let table = assigned(
			"struct A { float x; float second; }; struct B { float third; }; uniform A a; uniform B b; void main() { gl_FragColor = vec4(a.x + a.second + b.third); }",
			Scoring::BigramCount,
			true,
		);
		assert!(type_names_distinct(&table).is_ok());
		let types = finals(&table, |s| s.kind == SymbolKind::StructType);
		let fields = finals(&table, |s| matches!(s.kind, SymbolKind::Field(_)));
		assert!(
			fields.contains(&"x".to_string()),
			"swizzle-shaped field kept: {fields:?}"
		);
		for t in &types {
			assert!(
				!fields.contains(t),
				"type {t} also names a field: {fields:?}"
			);
		}
	}

	#[test]
	fn bigram_scoring_prefers_names_that_repeat_existing_contexts() {
		let s = sentinel(0);
		let text = format!("f(a)f(a) x({s})+{s}", s = s);
		let sc = s.chars().next().unwrap();
		let mut b = Bigrams::new(&text);
		// `(a` and `a)` exist twice; `(b` / `b)` never
		assert!(b.score(sc, "a", false) > b.score(sc, "b", false));
		assert!(b.score(sc, "a", true) > b.score(sc, "b", true));
		// any single letter beats any pair
		assert!(b.score(sc, "q", false) > b.score(sc, "ab", false));
		b.fold(sc, "a");
		assert_eq!(b.count('(', 'a'), 3);
		assert_eq!(b.count('a', ')'), 3);
		assert_eq!(b.count('+', 'a'), 1);
		assert!(b.prev_of.get(&sc).is_none() && b.next_of.get(&sc).is_none());
		// a pair adds its inner bigram once per occurrence
		let text = format!("x {s}, {s}", s = s);
		let mut b = Bigrams::new(&text);
		b.fold(sc, "ab");
		assert_eq!(b.count('a', 'b'), 2);
	}
}
