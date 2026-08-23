//! A small GLSL tokenizer for the text the parser keeps opaque: `#define`
//! bodies and `#if` conditions. Used to find the identifiers in them and to
//! squeeze their whitespace without gluing tokens together.

// `squeeze`/`needs_space` are used by the macro-squeezing simplify step.
#![cfg_attr(not(test), allow(dead_code))]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tok<'a> {
	Ident(&'a str),
	/// Preprocessing number: `1`, `1.5f`, `.5`, `1e-3`, `0x1F`.
	Number(&'a str),
	Punct(&'a str),
	/// Anything else (one character).
	Other(&'a str),
}

impl<'a> Tok<'a> {
	pub fn text(&self) -> &'a str {
		match *self {
			Tok::Ident(s) | Tok::Number(s) | Tok::Punct(s) | Tok::Other(s) => s,
		}
	}
}

const PUNCT3: &[&str] = &["<<=", ">>="];
const PUNCT2: &[&str] = &[
	"++", "--", "<<", ">>", "<=", ">=", "==", "!=", "&&", "||", "^^", "+=", "-=", "*=", "/=", "%=",
	"&=", "|=", "^=", "##",
];
const PUNCT1: &[u8] = b"+-*/%<>=!~&|^?:;,.(){}[]#";

fn is_ident_start(c: u8) -> bool {
	c.is_ascii_alphabetic() || c == b'_'
}

fn is_ident_char(c: u8) -> bool {
	c.is_ascii_alphanumeric() || c == b'_'
}

/// Split `s` into tokens. Whitespace, `\`-newline continuations and comments
/// are dropped.
pub fn tokenize(s: &str) -> Vec<Tok<'_>> {
	let b = s.as_bytes();
	let mut out = Vec::new();
	let mut i = 0;
	while i < b.len() {
		let c = b[i];
		if c.is_ascii_whitespace() {
			i += 1;
			continue;
		}
		if c == b'\\' {
			if b.get(i + 1) == Some(&b'\n') {
				i += 2;
				continue;
			}
			if b.get(i + 1) == Some(&b'\r') && b.get(i + 2) == Some(&b'\n') {
				i += 3;
				continue;
			}
		}
		if b[i..].starts_with(b"//") {
			while i < b.len() && b[i] != b'\n' {
				i += 1;
			}
			continue;
		}
		if b[i..].starts_with(b"/*") {
			i = match s[i + 2..].find("*/") {
				Some(p) => i + 2 + p + 2,
				None => b.len(),
			};
			continue;
		}
		if is_ident_start(c) {
			let start = i;
			while i < b.len() && is_ident_char(b[i]) {
				i += 1;
			}
			out.push(Tok::Ident(&s[start..i]));
			continue;
		}
		if c.is_ascii_digit() || (c == b'.' && b.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
			let start = i;
			i += 1;
			while i < b.len() {
				let d = b[i];
				if is_ident_char(d) || d == b'.' {
					i += 1;
				} else if (d == b'+' || d == b'-') && matches!(b[i - 1], b'e' | b'E') {
					i += 1;
				} else {
					break;
				}
			}
			out.push(Tok::Number(&s[start..i]));
			continue;
		}
		if let Some(p) = PUNCT3.iter().find(|p| s[i..].starts_with(*p)) {
			out.push(Tok::Punct(&s[i..i + p.len()]));
			i += p.len();
			continue;
		}
		if let Some(p) = PUNCT2.iter().find(|p| s[i..].starts_with(*p)) {
			out.push(Tok::Punct(&s[i..i + p.len()]));
			i += p.len();
			continue;
		}
		if PUNCT1.contains(&c) {
			out.push(Tok::Punct(&s[i..i + 1]));
			i += 1;
			continue;
		}
		let ch = s[i..].chars().next().unwrap();
		out.push(Tok::Other(&s[i..i + ch.len_utf8()]));
		i += ch.len_utf8();
	}
	out
}

/// All identifier tokens of `s`, in order, with repeats.
pub fn identifiers(s: &str) -> Vec<&str> {
	tokenize(s)
		.into_iter()
		.filter_map(|t| match t {
			Tok::Ident(i) => Some(i),
			_ => None,
		})
		.collect()
}

/// Whether the two adjacent tokens `a` and `b` need a space between them to
/// lex as the same two tokens.
pub fn needs_space(a: &str, b: &str) -> bool {
	if a.is_empty() || b.is_empty() {
		return false;
	}
	let joined = format!("{}{}", a, b);
	let toks = tokenize(&joined);
	!(toks.len() == 2 && toks[0].text() == a && toks[1].text() == b)
}

/// Re-join the tokens of `s` with the minimum whitespace: no comments, no
/// `\`-continuations, a single space only where two tokens would otherwise
/// merge.
pub fn squeeze(s: &str) -> String {
	let mut out = String::with_capacity(s.len());
	let mut prev: Option<&str> = None;
	for t in tokenize(s) {
		let text = t.text();
		if let Some(p) = prev {
			if needs_space(p, text) {
				out.push(' ');
			}
		}
		out.push_str(text);
		prev = Some(text);
	}
	out
}

#[cfg(test)]
mod tests {
	use super::*;

	fn texts(s: &str) -> Vec<&str> {
		tokenize(s).iter().map(|t| t.text()).collect()
	}

	#[test]
	fn tokenizes_numbers_identifiers_and_punctuation() {
		assert_eq!(texts("a+b"), ["a", "+", "b"]);
		assert_eq!(texts("x <<= 1"), ["x", "<<=", "1"]);
		assert_eq!(texts("a++ + ++b"), ["a", "++", "+", "++", "b"]);
		assert_eq!(
			texts("1.e+5 .5 1.5f 0x1F 1e-3"),
			["1.e+5", ".5", "1.5f", "0x1F", "1e-3"]
		);
		assert_eq!(texts("v.xy"), ["v", ".", "xy"]);
		assert_eq!(texts("a // c\n/* b */ d"), ["a", "d"]);
		assert_eq!(texts("x \\\n  y"), ["x", "y"]);
		assert_eq!(texts("a##b"), ["a", "##", "b"]);
		assert_eq!(texts("@"), ["@"]);
		assert!(matches!(tokenize("_a1")[0], Tok::Ident("_a1")));
		assert!(matches!(tokenize("1")[0], Tok::Number("1")));
		assert!(matches!(tokenize("(")[0], Tok::Punct("(")));
	}

	#[test]
	fn identifiers_of_macro_bodies() {
		assert_eq!(
			identifiers("((v)*(v)) + scale_factor*0.5"),
			["v", "v", "scale_factor"]
		);
		assert_eq!(
			identifiers("defined(FOO) && BAR > 1"),
			["defined", "FOO", "BAR"]
		);
		assert_eq!(identifiers("1.0e-5f"), Vec::<&str>::new());
	}

	#[test]
	fn needs_space_matrix() {
		for (a, b) in [
			("a", "b"),
			("+", "+"),
			("-", "--"),
			("<", "<"),
			("<", "<="),
			("/", "/"),
			("/", "*"),
			(".", "5"),
			("1", ".5"),
			("#", "#"),
			("a", "1"),
			("1", "e"),
			("&", "&"),
		] {
			assert!(needs_space(a, b), "{a} {b}");
		}
		for (a, b) in [
			("a", "+"),
			("+", "a"),
			(")", "("),
			("1", "+"),
			("-", "1"),
			("*", "/"),
			("x", "."),
		] {
			assert!(!needs_space(a, b), "{a} {b}");
		}
	}

	#[test]
	fn squeeze_keeps_tokens_apart() {
		assert_eq!(squeeze("( ( v ) * ( v ) )"), "((v)*(v))");
		assert_eq!(squeeze("a - -b"), "a- -b");
		assert_eq!(squeeze("a + ++ b"), "a+ ++b");
		assert_eq!(
			squeeze("int sum = 1; \\\n   sum = test; // c"),
			"int sum=1;sum=test;"
		);
		assert_eq!(squeeze("defined ( FOO ) && BAR > 1"), "defined(FOO)&&BAR>1");
		assert_eq!(squeeze(""), "");
		assert_eq!(squeeze("  "), "");
	}
}
