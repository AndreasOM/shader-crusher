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

use std::collections::{HashMap, HashSet};

use super::builtins::{is_swizzle, never_generate};
use super::scope::{sentinel_id, ScopeId, SymbolId, SymbolKind, SymbolTable};
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

struct Renamer<'a> {
	table:       &'a mut SymbolTable,
	letters:     Vec<char>,
	forbidden:   HashSet<String>,
	scoring:     Scoring,
	shadowing:   bool,
	next_triple: usize,
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

	fn choose(&mut self, _sym: SymbolId, avail: &mut Vec<String>) -> String {
		if avail.is_empty() {
			let n = self.triple();
			avail.push(n);
		}
		match self.scoring {
			// bigram scoring is added in a later step; frequency order for now
			Scoring::Frequency | Scoring::Bigram | Scoring::BigramCount => avail.remove(0),
		}
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
			if s.pinned.is_some() || matches!(s.kind, SymbolKind::Field(_)) {
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
	};
	let pool = r.initial_pool();
	r.visit_scope(0, pool, Vec::new());
	r.fields();
}

struct Apply<'a> {
	table: &'a SymbolTable,
}

impl<'a> VisitorMut for Apply<'a> {
	fn visit_identifier(&mut self, i: &mut Identifier) -> Visit {
		if let Some(id) = sentinel_id(&i.0) {
			i.0 = self.table.new_name_or_original(id).to_string();
		}
		Visit::Children
	}
	fn visit_type_name(&mut self, t: &mut TypeName) -> Visit {
		if let Some(id) = sentinel_id(&t.0) {
			t.0 = self.table.new_name_or_original(id).to_string();
		}
		Visit::Children
	}
}

/// Replace every sentinel by the symbol's new name (or its original name
/// when it has none).
pub fn apply(tu: &mut TranslationUnit, table: &SymbolTable) {
	tu.visit_mut(&mut Apply { table });
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
}
