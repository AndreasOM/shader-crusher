use std::collections::HashMap;
use std::ffi::CStr;

use libc::c_char;
use regex::Regex;

use crate::glsl::parser::Parse;
use crate::glsl::syntax::ShaderStage;
use crate::glsl::syntax::*;
use crate::glsl::visitor::{HostMut, Visit, VisitorMut};

include!(concat!(env!("OUT_DIR"), "/glsl_keywords.rs"));

struct IdentEntry {
	crushed_name: String,
	count:        u32,
}

impl std::fmt::Debug for IdentEntry {
	fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
		write!(f, "{} (*{})", self.crushed_name, self.count)
	}
}

impl IdentEntry {
	pub fn new(n: &str) -> IdentEntry {
		IdentEntry {
			crushed_name: n.to_string(),
			count:        0,
		}
	}
	fn set_crushed_name(&mut self, cn: &str) {
		self.crushed_name = cn.to_string();
	}
}

struct IdentMap {
	entries: HashMap<String, IdentEntry>,
}

impl std::fmt::Debug for IdentMap {
	fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
		write!(f, "entries: {:#?}", self.entries)
	}
}

impl IdentMap {
	pub fn new() -> IdentMap {
		IdentMap {
			entries: HashMap::new(),
		}
	}
	fn contains(&self, k: &str) -> bool {
		self.entries.contains_key(k)
	}
	fn keys(&self) -> Vec<String> {
		//		users.iter().map(|(_, user)| &user.reference.clone()).collect();
		self.entries.keys().map(|k| k.into()).collect()
		//			self.entries.iter().map( |k, v| k )
		//		self.entries.keys().map( |e| e.clone() ).to_vec()
	}
	fn crush(&mut self, used_identifiers: Vec<String>, blocklist: &[String]) {
		let mut candidates = Vec::new();
		// :TODO: be smarter ;)
		// :TODO: e.g. count frequency of characters in input and use most used ones
		// :TODO: provide more than 26 candidates, or generate them on the fly when needed
		for c in (b'a'..=b'z').rev() {
			let c = c as char;
			for c2 in (b'a'..=b'z').rev() {
				let c2 = c2 as char;
				candidates.push(format!("{}{}", c, c2).to_string());
			}
		}
		for c in (b'a'..=b'z').rev() {
			let c = c as char;
			candidates.push(c.to_string());
		}
		// filter out used identifiers to avoid unwanted aliasing
		let mut candidates = candidates
			.into_iter()
			.filter(|n| !used_identifiers.contains(n) && !blocklist.contains(n))
			.collect::<Vec<String>>();

		//		println!("Used identifiers {:?}", used_identifiers );
		//		println!("Best candidates {:?}", candidates );
		//		let mut count_index: Vec<(&String, &u32)> = self.entries.iter().map(|a|
		//			(a.0, &a.1.count)	// :TODO: count might be a bit simplistic here, total "cost" might be a better measure
		//		).collect::<Vec<(&String, &u32)>>().clone();
		let mut count_index = Vec::new();
		for e in self.entries.iter() {
			count_index.push((e.0.clone(), e.1.count));
		}
		count_index.sort_by(|a, b| {
			if b.1 != a.1 {
				b.1.cmp(&a.1)
			} else {
				a.0.cmp(&b.0)
			}
		});
		//		println!("{:?}", count_index);
		for k in count_index {
			match self.entries.get_mut(&k.0) {
				None => {}, // :WTF:
				Some(e) => {
					let cn = match candidates.pop() {
						None => e.crushed_name.clone(),
						Some(cn) => cn,
					};
					//					println!("Crushing {:?} to {:?}", e, cn );
					e.set_crushed_name(&cn);
				},
			}
		}
	}
	fn get_crushed_name(&self, n: &str) -> Option<String> {
		self.entries.get(n).map(|a| a.crushed_name.clone())
	}
	fn add(&mut self, n: &str) -> u32 {
		let e = self
			.entries
			.entry(n.to_string())
			.or_insert_with(|| IdentEntry::new(n));
		e.count += 1;
		e.count
	}
}

#[derive(Debug, PartialEq)]
enum CounterPhase {
	Analysing,
	Crushing,
}

struct Counter {
	phase:                 CounterPhase,
	blocklist:             Vec<String>,
	crushing:              bool,
	identifiers_crushed:   IdentMap,
	identifiers_uncrushed: IdentMap,
}

impl Counter {
	pub fn new() -> Counter {
		Counter {
			phase:                 CounterPhase::Analysing,
			blocklist:             vec!["main".to_string()],
			crushing:              true,
			identifiers_crushed:   IdentMap::new(),
			identifiers_uncrushed: IdentMap::new(),
		}
	}

	pub fn crush_names(&mut self) {
		self.identifiers_crushed
			.crush(self.identifiers_uncrushed.keys().to_vec(), &self.blocklist);
	}
}
impl VisitorMut for Counter {
	/*
	fn visit_translation_unit(&mut self, tu: &mut TranslationUnit) -> Visit {
		println!("{:?}", tu );
		Visit::Children
	}
	*/
	/*
	fn visit_preprocessor(&mut self, p: &mut Preprocessor) -> Visit {
		println!("Preprocessor: {:?}", p );
		match p {
			Preprocessor::Pragma( pragma ) => {
				match pragma.command.as_ref() {
					"SHADER_CRUSHER_OFF" => {
						self.crushing = false;
						pragma.command = "".to_string();
					},
					"SHADER_CRUSHER_ON" => {
						self.crushing = true;
						pragma.command = "".to_string();
					},
					_ => {

					},
				};
			},
			_ => {},
		};
		Visit::Children
	}
	*/
	fn visit_preprocessor_define(&mut self, pd: &mut PreprocessorDefine) -> Visit {
		//		println!("Define: {:?} - {:?}", pd, self.crushing );
		match pd {
			PreprocessorDefine::ObjectLike { ident, value: _ } => {
				println!("{:?}", ident);
				match ident {
					Identifier(i) => {
						println!("{:?}", i);
						match self.phase {
							CounterPhase::Crushing => {},
							CounterPhase::Analysing => {
								let c = self.crushing;
								self.crushing = false;
								// :HACK: always add #define identifiers as uncrushed, so we don't have to parse all potential usages
								self.add_identifier(i);
								self.crushing = c;
							},
						}
					},
					// _ => {},
				}
			},
			PreprocessorDefine::FunctionLike {
				ident,
				args: _,
				value: _,
			} => {
				println!("{:?}", ident);
				match ident {
					Identifier(i) => {
						println!("{:?}", i);
						match self.phase {
							CounterPhase::Crushing => {},
							CounterPhase::Analysing => {
								let c = self.crushing;
								self.crushing = false;
								// :HACK: always add #define identifiers as uncrushed, so we don't have to parse all potential usages
								self.add_identifier(i);
								self.crushing = c;
							},
						}
					},
					// _ => {},
				}
			},
			/*
			x => {
				println!("{:?}", x);
			},
			*/
		};
		Visit::Children
	}

	fn visit_preprocessor_pragma(&mut self, pragma: &mut PreprocessorPragma) -> Visit {
		//		println!("Pragma: {:?} - {:?}", pragma, self.crushing );
		match pragma.command.as_ref() {
			"SHADER_CRUSHER_OFF" => {
				self.crushing = false;
				pragma.command = "".to_string(); // no idea how to remove the pragma completely :(
				println!("== Crusher: Off ==");
			},
			"SHADER_CRUSHER_ON" => {
				self.crushing = true;
				pragma.command = "".to_string();
				println!("== Crusher: On ==");
			},
			_ => {},
		}
		Visit::Children
	}
	fn visit_identifier(&mut self, e: &mut Identifier) -> Visit {
		//		println!("Identifier: {:?}", e );
		match e {
			Identifier(i) => {
				match self.phase {
					CounterPhase::Crushing => {
						//						println!("Expr Identifier {:?}", i );
						match self.identifiers_crushed.get_crushed_name(i) {
							Some(n) => {
								println!("Identifier: Replacing {:?} with {:?}", i, n);
								*e = Identifier(n.to_string());
							},
							None => {
								//								println!("No crushed version of {:?} found", i );
							},
						}
					},
					CounterPhase::Analysing => {
						self.add_identifier(i);
					},
				}
			},
			// _ => {},
		}
		Visit::Children
	}
	fn visit_type_name(&mut self, tn: &mut TypeName) -> Visit {
		//		println!("TypeName {:#?}", tn );
		match tn {
			TypeName(i) => {
				match self.phase {
					CounterPhase::Crushing => {
						//						println!("Expr Identifier {:?}", i );
						match self.identifiers_crushed.get_crushed_name(i) {
							Some(n) => {
								println!("TypeName/Identifier: Replacing {:?} with {:?}", i, n);
								*tn = TypeName(n.to_string());
							},
							None => {
								//								println!("No crushed version of {:?} found", i );
							},
						}
					},
					CounterPhase::Analysing => {
						self.add_identifier(i);
					},
				}
			},
			// _ => {},
		}
		Visit::Children
	}
	/*
		fn visit_single_declaration(&mut self, declaration: &mut SingleDeclaration) -> Visit {
	//		println!("{:#?}", declaration );
			println!("SingleDeclaration: {:#?}", declaration );
			match &declaration.name {
				None => {

				},
				Some( name ) => {
					println!("declaration.name {:?}", name );
					let n = name.to_string();
					match self.phase {
						CounterPhase::Analysing => {
							self.add_identifier( &n );
						},
						CounterPhase::Crushing => {
						}
					}
				},
			}
			Visit::Children
	//		Visit::Parent
		}
	*/
	/*
	fn visit_arrayed_identifier(&mut self, ai: &mut ArrayedIdentifier) -> Visit {
		println!("visit_arrayed_identifier {:?}", ai );
		Visit::Children
	}
	*/
	/*
		fn visit_function_prototype(&mut self, fp: &mut FunctionPrototype) -> Visit {
	//		println!("{:?}", fp );
	//		println!("{}", fp.name );
			match self.phase {
				CounterPhase::Analysing => {
	//				self.add_identifier( &fp.name.as_str() );
				},
				CounterPhase::Crushing => {
					/* :TODO:
					match self.identifiers_crushed.get_crushed_name( &n ) {
						Some( cn ) => {
							println!("Found {:?} for {:?}", cn, n );
							declaration.name = Some( Identifier( cn.to_string() ) );
						},
						None => {
							println!("No crushed version of {:?} found", n );
						},
					}
					*/

				}
			}
			Visit::Children
		}
	*/
}

impl Counter {
	/// Identifiers reserved by the GLSL spec: anything starting with `gl_`
	/// (built-in variables, constants, functions) and anything containing `__`.
	/// They are never renamed, independent of the blocklist.
	fn is_reserved(n: &str) -> bool {
		n.starts_with("gl_") || n.contains("__")
	}
	fn add_identifier(&mut self, n: &str) {
		let blocklisted = Self::is_reserved(n) || self.blocklist.iter().any(|s| s == n);
		let uncrushed = self.identifiers_uncrushed.contains(n);
		if self.crushing && !blocklisted && !uncrushed {
			let c = self.identifiers_crushed.add(n);
			println!(
				"{: >8} x {: <20} [-crushed-] {} {} {}",
				c,
				&n,
				if self.crushing {
					"[--CRUSHING--]"
				} else {
					"[NOT CRUSHING]"
				},
				if blocklisted {
					"[--BLOCKLISTED--]"
				} else {
					"[NOT BLOCKLISTED]"
				},
				if uncrushed {
					"[--UNCRUSHED--]"
				} else {
					"[NOT UNCRUSHED]"
				},
			);
		} else {
			let c = self.identifiers_uncrushed.add(n);
			println!(
				"{: >8} x {: <20} [uncrushed] {} {} {}",
				c,
				&n,
				if self.crushing {
					"[--CRUSHING--]"
				} else {
					"[NOT CRUSHING]"
				},
				if blocklisted {
					"[--BLOCKLISTED--]"
				} else {
					"[NOT BLOCKLISTED]"
				},
				if uncrushed {
					"[--UNCRUSHED--]"
				} else {
					"[NOT UNCRUSHED]"
				},
			);
		}
	}
	fn blocklist_identifier(&mut self, n: &str) {
		if !self.blocklist.contains(&n.to_string()) {
			self.blocklist.push(n.to_string());
		}
	}
}

/// GLSL (§3.2) accepts CR, LF and CRLF as line terminators, but the glsl
/// crate's directive lexer (`str_till_eol`) keeps a trailing `\r`, which
/// breaks `#pragma SHADER_CRUSHER_*` matching and leaks `\r` into `#define`
/// values. Normalize to LF up front; output is always LF.
fn normalize_line_endings(src: &str) -> String {
	src.replace("\r\n", "\n").replace('\r', "\n")
}

/// Remove `// ...` and single-line `/* ... */` comments from preprocessor
/// directive lines (`#extension`, `#define`, `#pragma`, ...).
///
/// The glsl parser handles comments in code, but directive lines are lexed
/// "till end of line": a trailing comment after `#extension` is a parse error,
/// and a comment after `#define X 1` would be copied into the output verbatim.
/// Non-directive lines are passed through untouched.
/// Expects LF line endings (see `normalize_line_endings`); re-emits `\n`.
fn strip_directive_comments(src: &str) -> String {
	let mut out = String::with_capacity(src.len());
	for line in src.lines() {
		if line.trim_start().starts_with('#') {
			let line = match line.find("//") {
				Some(pos) => &line[..pos],
				None => line,
			};
			let mut line = line.to_string();
			while let Some(start) = line.find("/*") {
				match line[start + 2..].find("*/") {
					Some(len) => line.replace_range(start..start + 2 + len + 2, " "),
					None => break, // block comment continues on the next line; leave it to the parser
				}
			}
			out.push_str(line.trim_end());
		} else {
			out.push_str(line);
		}
		out.push('\n');
	}
	if !src.ends_with('\n') && !src.is_empty() {
		out.pop();
	}
	out
}

pub struct ShaderCrusher {
	input:          String,
	output:         String,
	input_entropy:  f32,
	output_entropy: f32,
	blocklist:      Vec<String>,
}

impl ShaderCrusher {
	pub fn new() -> ShaderCrusher {
		let blocklist = GlslKeywords::get();
		ShaderCrusher {
			input: String::new(),
			output: String::new(),
			input_entropy: 0.0,
			output_entropy: 0.0,
			blocklist,
		}
	}
	pub fn blocklist_identifier(&mut self, n: &str) {
		if !self.blocklist.contains(&n.to_string()) {
			self.blocklist.push(n.to_string());
		}
	}

	fn recalc_entropy(&mut self) {
		//		self.input_entropy = entropy::shannon_entropy( self.input.as_bytes() );
		//		self.output_entropy = entropy::shannon_entropy( self.output.as_bytes() );
		self.input_entropy = entropy::metric_entropy(self.input.as_bytes());
		self.output_entropy = entropy::metric_entropy(self.output.as_bytes());
	}
	pub fn set_input(&mut self, input: &str) {
		self.input = input.to_string();
		self.output = self.input.clone();

		self.recalc_entropy();
	}
	pub fn get_output(&self) -> String {
		self.output.clone()
	}

	pub fn get_input_entropy(&self) -> f32 {
		self.input_entropy
	}

	pub fn get_output_entropy(&self) -> f32 {
		self.output_entropy
	}

	pub fn crush(&mut self) {
		let source = strip_directive_comments(&normalize_line_endings(&self.input));
		let stage = ShaderStage::parse(&source);
		//		println!("Stage: {:?}", stage);
		let mut stage = match stage {
			Err(e) => {
				println!("Error parsing shader {:?}", e);
				return;
			},
			Ok(stage) => {
				//				println!("Parsed shader {:#?}", stage );
				stage
			},
		};

		//		let mut compound = stage.clone();
		let mut counter = Counter::new();
		//		println!("Blaocklist {:?}", self.blocklist );
		for n in &self.blocklist {
			counter.blocklist_identifier(n);
		}
		stage.visit_mut(&mut counter);
		counter.crush_names();
		// :TODO: fixup crushed identifiers names
		// skip crushing for now
		counter.phase = CounterPhase::Crushing;
		stage.visit_mut(&mut counter);
		println!("Stats:\n-------");
		println!("Crushed Varnames: {:?}", counter.identifiers_crushed);
		println!("Uncrushed Varnames: {:?}", counter.identifiers_uncrushed);
		let mut glsl_buffer = String::new();
		crate::glsl::transpiler::glsl::show_translation_unit(&mut glsl_buffer, &stage);
		//        println!("r {:?}", r);
		//        println!("r {}", r);
		//        let pr: PrettyPrint = From::from(stage);// as &PrettyPrint;
		//		PrettyPrint::print_shaderstage( &stage );
		//        println!("{:?}", pr);

		// cleanup empty pragmas
		let re = Regex::new(r"(?m)^\s*#\s*pragma\s*$").unwrap();
		let glsl_buffer = re.replace_all(&glsl_buffer, |_c: &regex::Captures| {
			//				println!("{:?}", c );
			"".to_string()
		});

		// cleanup double braces e.g. "((x))"
		/*		// :TODO: this is to agressive, or maybe even wrong
				let re = Regex::new(r"(?m)\(\(([^)]*)\)\)").unwrap();
				let glsl_buffer = re.replace_all(
					&glsl_buffer,
					|c: &regex::Captures|{
		//				println!("{:?}", c );
						let inner = c.get(1).map_or("", |m| m.as_str() );
		//				println!("{}", inner );
						format!("({}))", inner).clone()
					}
				);
		*/
		let re = Regex::new(r"(?m)\(\(([a-zA-Z0-9.]+)\)").unwrap();
		let glsl_buffer = re.replace_all(&glsl_buffer, |c: &regex::Captures| {
			//				println!("{:?}", c );
			let inner = c.get(1).map_or("", |m| m.as_str());
			//				println!("{}", inner );
			format!("({}", inner).clone()
		});
		//println!("====");
		let re = Regex::new(r"(?m)\(\(([a-zA-Z0-9.]+)\)").unwrap();
		let glsl_buffer = re.replace_all(&glsl_buffer, |c: &regex::Captures| {
			//				println!("{:?}", c );
			let inner = c.get(1).map_or("", |m| m.as_str());
			//				println!("{}", inner );
			format!("({}", inner).clone()
		});

		//println!("====");

		//		let re = Regex::new(r"(?m)([\n\s-+*]+)\(([a-zA-Z0-9.]+)\)").unwrap();
		//		let re = Regex::new(r"(?m)([\n[[:space:]]-+*]+)\(([a-zA-Z0-9.]+)\)").unwrap();
		//		let re = Regex::new(r"(?m)([\n[[:space:]]-+*<>=]+)\(([a-zA-Z0-9.]+)\)").unwrap();
		//		let re = Regex::new(r"(?m)([\n-+*<>=]+)\(([a-zA-Z0-9.]+)\)").unwrap();
		let re = Regex::new(r"(?m)([-+*<>=]+)\(([a-zA-Z0-9.]+)\)").unwrap();

		let glsl_buffer = re.replace_all(&glsl_buffer, |c: &regex::Captures| {
			//				println!("{:?}", c );
			let prefix = c.get(1).map_or("", |m| m.as_str());
			let inner = c.get(2).map_or("", |m| m.as_str());
			//				println!("{}{}", prefix, inner );
			format!("{}{}", prefix, inner).clone()
		});

		self.output = glsl_buffer.to_string();
		self.recalc_entropy();
		let il = self.input.len();
		let ie = self.input_entropy;
		let it = il as f32 * ie;
		let ol = self.output.len();
		let oe = self.output_entropy;
		let ot = ol as f32 * oe;
		println!("Input  Size: {}, Entropy: {} => {}", il, ie, it);
		println!("Output Size: {}, Entropy: {} => {}", ol, oe, ot);
	}
}

impl Default for ShaderCrusher {
	fn default() -> Self {
		Self::new()
	}
}

// API
#[no_mangle]
pub unsafe extern "C" fn shadercrusher_new() -> *mut ShaderCrusher {
	Box::into_raw(Box::new(ShaderCrusher::new()))
}

#[no_mangle]
pub extern "C" fn shadercrusher_free(ptr: *mut ShaderCrusher) {
	if ptr.is_null() {
		return;
	}
	unsafe {
		drop(Box::from_raw(ptr));
	}
}

#[no_mangle]
pub extern "C" fn shadercrusher_set_input(ptr: *mut ShaderCrusher, input: *const c_char) {
	let shadercrusher = unsafe {
		assert!(!ptr.is_null());
		&mut *ptr
	};
	let input = unsafe {
		assert!(!input.is_null());
		CStr::from_ptr(input)
	};
	let input = input.to_str().unwrap();
	shadercrusher.set_input(input);
}
/*
#[no_mangle]
pub extern fn theme_song_free(s: *mut c_char) {
	unsafe {
		if s.is_null() { return }
		CString::from_raw(s)
	};
}
*/
#[no_mangle]
pub extern "C" fn shadercrusher_get_ouput(ptr: *mut ShaderCrusher) -> *mut c_char {
	let shadercrusher = unsafe {
		assert!(!ptr.is_null());
		&mut *ptr
	};
	let output = shadercrusher.get_output();

	let output_cs = std::ffi::CString::new(output).unwrap();
	output_cs.into_raw()
}

#[no_mangle]
pub extern "C" fn shadercrusher_free_ouput(_ptr: *mut ShaderCrusher, output_cs: *mut c_char) {
	unsafe {
		if output_cs.is_null() {
			return;
		}
		drop(std::ffi::CString::from_raw(output_cs));
	}
}

#[no_mangle]
pub extern "C" fn shadercrusher_crush(ptr: *mut ShaderCrusher) {
	let shadercrusher = unsafe {
		assert!(!ptr.is_null());
		&mut *ptr
	};
	shadercrusher.crush();
}

#[cfg(test)]
mod tests {
	use super::*;

	fn crush(src: &str) -> String {
		let mut sc = ShaderCrusher::new();
		sc.set_input(src);
		sc.crush();
		sc.get_output()
	}

	#[test]
	fn gl_prefixed_identifiers_are_never_renamed() {
		let out = crush(
			"#version 110\nuniform vec4 color_tint;\nvoid main() { gl_FragColor = gl_Color * color_tint; }\n",
		);
		assert!(out.contains("gl_FragColor"), "{out}");
		assert!(out.contains("gl_Color"), "{out}");
		assert!(
			!out.contains("color_tint"),
			"user identifiers must still be crushed: {out}"
		);
	}

	#[test]
	fn double_underscore_identifiers_are_never_renamed() {
		let out = crush("#version 110\nuniform float my__value;\nvoid main() { gl_FragColor = vec4(my__value); }\n");
		assert!(out.contains("my__value"), "{out}");
	}

	#[test]
	fn strip_directive_comments_only_touches_directive_lines() {
		let src = "#extension all : disable // no error\n  # define X 1 /* one */ // c\n#define Y /* a */ 2 /* b */\nfloat a = 1.0; // keep me\n/* keep */ float b;\n#pragma once";
		let expected = "#extension all : disable\n  # define X 1\n#define Y   2\nfloat a = 1.0; // keep me\n/* keep */ float b;\n#pragma once";
		assert_eq!(strip_directive_comments(src), expected);
		assert_eq!(
			strip_directive_comments("#define A 1 // c\n"),
			"#define A 1\n"
		);
		assert_eq!(strip_directive_comments(""), "");
	}

	#[test]
	fn comments_on_directive_lines_do_not_break_parsing_or_leak() {
		let out = crush(
			"#version 110\n#extension all : warn // no error\n#define SCALE 2.0 // twice\nuniform float some_value;\nvoid main() { gl_FragColor = vec4(some_value * SCALE); }\n",
		);
		assert!(out.contains("#extension all : warn\n"), "{out}");
		assert!(out.contains("#define SCALE 2.0\n"), "{out}");
		assert!(!out.contains("//"), "comment leaked: {out}");
		assert!(!out.contains("some_value"), "shader was not crushed: {out}");
	}

	#[test]
	fn pragma_with_trailing_comment_still_toggles_crushing() {
		let out = crush(
			"#version 110\n#pragma SHADER_CRUSHER_OFF // keep uniforms\nuniform float keep_me;\n#pragma SHADER_CRUSHER_ON // back on\nuniform float crush_me;\nvoid main() { gl_FragColor = vec4(keep_me + crush_me); }\n",
		);
		assert!(out.contains("keep_me"), "{out}");
		assert!(!out.contains("crush_me"), "{out}");
		assert!(!out.contains("#pragma"), "pragma should be removed: {out}");
	}

	#[test]
	fn normalize_line_endings_handles_crlf_and_cr() {
		assert_eq!(normalize_line_endings("a\r\nb\rc\n"), "a\nb\nc\n");
		assert_eq!(normalize_line_endings("a\nb"), "a\nb");
		assert_eq!(normalize_line_endings(""), "");
	}

	/// Every directive kind the glsl crate lexes "till end of line", plus the
	/// crusher's own pragmas: with CRLF input, 0.5.0-alpha kept the `\r` in the
	/// pragma command (so OFF/ON was ignored) and in `#define` values.
	const LINE_ENDING_FIXTURE: &str = "#version 110\n#extension GL_ARB_shader_texture_lod : enable\n#define SCALE 2.0\n#define SQ(v) ((v)*(v))\n#pragma SHADER_CRUSHER_OFF\nuniform float keep_me;\n#pragma SHADER_CRUSHER_ON\nuniform float crush_me;\nvoid main() {\n\tfloat scaled_value = keep_me * SCALE + SQ(crush_me);\n\tgl_FragColor = vec4(scaled_value);\n}\n";

	fn assert_crushes_like_lf(input: &str) {
		let expected = crush(LINE_ENDING_FIXTURE);
		let out = crush(input);
		assert_eq!(out, expected);
		assert!(!out.contains('\r'), "CR leaked into output: {out:?}");
		assert!(out.contains("keep_me"), "pragma OFF ignored: {out}");
		assert!(!out.contains("crush_me"), "shader was not crushed: {out}");
		assert!(!out.contains("#pragma"), "pragma should be removed: {out}");
		assert!(out.contains("#define SCALE 2.0\n"), "{out}");
	}

	#[test]
	fn crlf_input_crushes_identically_to_lf() {
		assert_crushes_like_lf(&LINE_ENDING_FIXTURE.replace('\n', "\r\n"));
	}

	#[test]
	fn cr_only_input_crushes_identically_to_lf() {
		assert_crushes_like_lf(&LINE_ENDING_FIXTURE.replace('\n', "\r"));
	}
}
