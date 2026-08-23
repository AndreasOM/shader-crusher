use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, CString};
use std::fmt;

use libc::{c_char, c_int};

use super::builtins::{is_builtin_function, is_keyword, is_reserved, is_swizzle, never_generate};
use super::preprocess::{normalize_line_endings, strip_directive_comments};
use super::protect::{self, Protection};
use super::{printer, selfcheck, simplify};
use super::{CrushError, Options, Scoring};
use crate::glsl::parser::parse_translation_unit_with_rest;
use crate::glsl::syntax::*;
use crate::glsl::visitor::{HostMut, Visit, VisitorMut};

struct IdentEntry {
	crushed_name: String,
	count:        u32,
}

impl fmt::Debug for IdentEntry {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
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

impl fmt::Debug for IdentMap {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "entries: {:#?}", self.entries)
	}
}

impl IdentMap {
	pub fn new() -> IdentMap {
		IdentMap {
			entries: HashMap::new(),
		}
	}
	fn keys(&self) -> Vec<String> {
		self.entries.keys().map(|k| k.into()).collect()
	}
	fn len(&self) -> usize {
		self.entries.len()
	}
	/// Assign short names by descending use count. `forbidden` rejects
	/// candidate names that already mean something in this shader.
	fn crush(&mut self, forbidden: &dyn Fn(&str) -> bool) {
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
		let mut candidates = candidates
			.into_iter()
			.filter(|n| !forbidden(n))
			.collect::<Vec<String>>();

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
		for k in count_index {
			match self.entries.get_mut(&k.0) {
				None => {}, // :WTF:
				Some(e) => {
					let cn = match candidates.pop() {
						None => e.crushed_name.clone(),
						Some(cn) => cn,
					};
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

/// Flat (one namespace) identifier renamer. Interim: replaced by the
/// scope-aware renamer in a later step.
struct Counter {
	phase:                 CounterPhase,
	/// Names pinned by `protect` (macros, OFF regions, blocklist, blocks,
	/// built-in members seen as selectors).
	pinned:                HashSet<String>,
	verbose:               bool,
	identifiers_crushed:   IdentMap,
	identifiers_uncrushed: IdentMap,
}

impl Counter {
	pub fn new(protection: &Protection, verbose: bool) -> Counter {
		let mut pinned = protection.names.clone();
		pinned.extend(protection.field_names.iter().cloned());
		Counter {
			phase: CounterPhase::Analysing,
			pinned,
			verbose,
			identifiers_crushed: IdentMap::new(),
			identifiers_uncrushed: IdentMap::new(),
		}
	}

	/// A name that must not be used for anything else in this shader.
	fn is_pinned(&self, n: &str) -> bool {
		is_reserved(n)
			|| is_keyword(n)
			|| is_builtin_function(n)
			|| is_swizzle(n)
			|| self.pinned.contains(n)
	}

	pub fn crush_names(&mut self) {
		let uncrushed = self.identifiers_uncrushed.keys();
		let pinned = &self.pinned;
		let forbidden = |n: &str| {
			never_generate(n)
				|| is_swizzle(n)
				|| pinned.contains(n)
				|| uncrushed.iter().any(|u| u == n)
		};
		self.identifiers_crushed.crush(&forbidden);
	}
}
impl VisitorMut for Counter {
	/// Macro lines are opaque: the name and body identifiers are pinned by
	/// `protect`, the parameters belong to the line alone.
	fn visit_preprocessor_define(&mut self, _pd: &mut PreprocessorDefine) -> Visit {
		Visit::Parent
	}

	/// `layout(location = 0)`: the qualifier name is an identifier to the
	/// parser but a keyword to the compiler; only the value is an expression.
	fn visit_layout_qualifier_spec(&mut self, l: &mut LayoutQualifierSpec) -> Visit {
		if let LayoutQualifierSpec::Identifier(_, Some(e)) = l {
			e.visit_mut(self);
		}
		Visit::Parent
	}

	fn visit_identifier(&mut self, e: &mut Identifier) -> Visit {
		let Identifier(i) = e;
		match self.phase {
			CounterPhase::Crushing => {
				if let Some(n) = self.identifiers_crushed.get_crushed_name(i) {
					if self.verbose {
						eprintln!("Identifier: Replacing {:?} with {:?}", i, n);
					}
					*e = Identifier(n);
				}
			},
			CounterPhase::Analysing => {
				self.add_identifier(&i.clone());
			},
		}
		Visit::Children
	}
	fn visit_type_name(&mut self, tn: &mut TypeName) -> Visit {
		let TypeName(i) = tn;
		match self.phase {
			CounterPhase::Crushing => {
				if let Some(n) = self.identifiers_crushed.get_crushed_name(i) {
					if self.verbose {
						eprintln!("TypeName/Identifier: Replacing {:?} with {:?}", i, n);
					}
					*tn = TypeName(n);
				}
			},
			CounterPhase::Analysing => {
				self.add_identifier(&i.clone());
			},
		}
		Visit::Children
	}
}

impl Counter {
	fn add_identifier(&mut self, n: &str) {
		let pinned = self.is_pinned(n);
		let c = if pinned {
			self.identifiers_uncrushed.add(n)
		} else {
			self.identifiers_crushed.add(n)
		};
		if self.verbose {
			eprintln!(
				"{: >8} x {: <20} {}",
				c,
				n,
				if pinned { "[uncrushed]" } else { "[-crushed-]" },
			);
		}
	}
}

/// Numbers about the last successful crush.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Stats {
	pub input_len:      usize,
	pub output_len:     usize,
	pub input_entropy:  f32,
	pub output_entropy: f32,
	/// Distinct identifiers that got a new name.
	pub renamed:        usize,
	/// Distinct identifiers that were kept (keywords, builtins, protected).
	pub kept:           usize,
}

impl fmt::Display for Stats {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		let pct = if self.input_len > 0 {
			100.0 * self.output_len as f32 / self.input_len as f32
		} else {
			0.0
		};
		write!(
			f,
			"{} -> {} bytes ({:.1}%), entropy {:.3} -> {:.3}, {} identifiers renamed, {} kept",
			self.input_len,
			self.output_len,
			pct,
			self.input_entropy,
			self.output_entropy,
			self.renamed,
			self.kept
		)
	}
}

/// Crush `src` with `opts`; returns the crushed shader and statistics.
pub fn crush_str(src: &str, opts: &Options) -> Result<(String, Stats), CrushError> {
	let mut sc = ShaderCrusher::with_options(opts.clone());
	sc.set_input(src);
	sc.crush()?;
	Ok((sc.get_output(), sc.stats()))
}

pub struct ShaderCrusher {
	input:        String,
	output:       String,
	options:      Options,
	stats:        Stats,
	last_error:   Option<CrushError>,
	/// C string copy of `last_error` for `shadercrusher_get_error`.
	last_error_c: Option<CString>,
}

impl ShaderCrusher {
	pub fn new() -> ShaderCrusher {
		Self::with_options(Options::default())
	}

	pub fn with_options(options: Options) -> ShaderCrusher {
		ShaderCrusher {
			input: String::new(),
			output: String::new(),
			options,
			stats: Stats::default(),
			last_error: None,
			last_error_c: None,
		}
	}

	pub fn options(&self) -> &Options {
		&self.options
	}
	pub fn options_mut(&mut self) -> &mut Options {
		&mut self.options
	}
	pub fn blocklist_identifier(&mut self, n: &str) {
		if !self.options.blocklist.iter().any(|b| b == n) {
			self.options.blocklist.push(n.to_string());
		}
	}

	pub fn set_input(&mut self, input: &str) {
		self.input = input.to_string();
		self.output = self.input.clone();
		self.stats = Stats {
			input_len: self.input.len(),
			output_len: self.output.len(),
			input_entropy: entropy::metric_entropy(self.input.as_bytes()),
			output_entropy: entropy::metric_entropy(self.output.as_bytes()),
			..Stats::default()
		};
		self.last_error = None;
		self.last_error_c = None;
	}
	/// The crushed shader, or the unchanged input if the last crush failed
	/// (or none ran).
	pub fn get_output(&self) -> String {
		self.output.clone()
	}
	pub fn stats(&self) -> Stats {
		self.stats
	}
	pub fn last_error(&self) -> Option<&CrushError> {
		self.last_error.as_ref()
	}

	pub fn get_input_entropy(&self) -> f32 {
		self.stats.input_entropy
	}

	pub fn get_output_entropy(&self) -> f32 {
		self.stats.output_entropy
	}

	/// Crush the input set with `set_input`. On error the output stays the
	/// unchanged input.
	pub fn crush(&mut self) -> Result<(), CrushError> {
		let r = self.crush_inner();
		if let Err(e) = &r {
			self.output = self.input.clone();
			self.last_error_c = CString::new(e.to_string().replace('\0', " ")).ok();
			self.last_error = Some(e.clone());
		}
		r
	}

	fn crush_inner(&mut self) -> Result<(), CrushError> {
		let verbose = self.options.verbose;
		let source = strip_directive_comments(&normalize_line_endings(&self.input));
		let (mut stage, rest) =
			parse_translation_unit_with_rest(&source).map_err(|e| CrushError::Parse(e.info))?;
		if !rest.trim().is_empty() {
			let consumed = source.len() - rest.len();
			let rest: String = rest.trim_start().chars().take(60).collect();
			return Err(CrushError::PartialParse { consumed, rest });
		}

		let protection = protect::run(&mut stage, &self.options.blocklist)?;
		if verbose {
			let mut names: Vec<_> = protection.names.iter().collect();
			names.sort();
			eprintln!("Protected names: {:?}", names);
			let mut fields: Vec<_> = protection.field_names.iter().collect();
			fields.sort();
			eprintln!("Protected field names: {:?}", fields);
		}

		if self.options.simplify {
			simplify::run(&mut stage, &simplify::Flags::default());
		}

		let mut counter = Counter::new(&protection, verbose);
		stage.visit_mut(&mut counter);
		if self.options.rename {
			counter.crush_names();
		}
		counter.phase = CounterPhase::Crushing;
		stage.visit_mut(&mut counter);
		if verbose {
			eprintln!("Crushed Varnames: {:?}", counter.identifiers_crushed);
			eprintln!("Uncrushed Varnames: {:?}", counter.identifiers_uncrushed);
		}
		let out = printer::print(&stage);
		if self.options.selfcheck {
			selfcheck::reparse_equals(&out, &stage)?;
		}

		self.output = out;
		self.stats = Stats {
			input_len:      self.input.len(),
			output_len:     self.output.len(),
			input_entropy:  entropy::metric_entropy(self.input.as_bytes()),
			output_entropy: entropy::metric_entropy(self.output.as_bytes()),
			renamed:        counter.identifiers_crushed.len(),
			kept:           counter.identifiers_uncrushed.len(),
		};
		Ok(())
	}
}

impl Default for ShaderCrusher {
	fn default() -> Self {
		Self::new()
	}
}

// C API
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

/// Add an identifier that must not be renamed.
#[no_mangle]
pub extern "C" fn shadercrusher_blocklist_identifier(ptr: *mut ShaderCrusher, name: *const c_char) {
	let shadercrusher = unsafe {
		assert!(!ptr.is_null());
		&mut *ptr
	};
	let name = unsafe {
		assert!(!name.is_null());
		CStr::from_ptr(name)
	};
	shadercrusher.blocklist_identifier(name.to_str().unwrap());
}

/// Set a boolean/enum option by name: `verbose`, `rename`, `simplify`,
/// `shadowing`, `selfcheck` (0/1) and `scoring` (0 frequency, 1 bigram,
/// 2 bigram-count). Returns 0 on success, -1 for an unknown key.
#[no_mangle]
pub extern "C" fn shadercrusher_set_option(
	ptr: *mut ShaderCrusher,
	key: *const c_char,
	value: c_int,
) -> c_int {
	let shadercrusher = unsafe {
		assert!(!ptr.is_null());
		&mut *ptr
	};
	let key = unsafe {
		assert!(!key.is_null());
		CStr::from_ptr(key)
	};
	let o = shadercrusher.options_mut();
	let b = value != 0;
	match key.to_str().unwrap_or("") {
		"verbose" => o.verbose = b,
		"rename" => o.rename = b,
		"simplify" => o.simplify = b,
		"shadowing" => o.shadowing = b,
		"selfcheck" => o.selfcheck = b,
		"scoring" => {
			o.scoring = match value {
				0 => Scoring::Frequency,
				1 => Scoring::Bigram,
				2 => Scoring::BigramCount,
				_ => return -1,
			}
		},
		_ => return -1,
	}
	0
}

#[no_mangle]
pub extern "C" fn shadercrusher_get_ouput(ptr: *mut ShaderCrusher) -> *mut c_char {
	let shadercrusher = unsafe {
		assert!(!ptr.is_null());
		&mut *ptr
	};
	let output = shadercrusher.get_output();

	let output_cs = CString::new(output).unwrap();
	output_cs.into_raw()
}

#[no_mangle]
pub extern "C" fn shadercrusher_free_ouput(_ptr: *mut ShaderCrusher, output_cs: *mut c_char) {
	unsafe {
		if output_cs.is_null() {
			return;
		}
		drop(CString::from_raw(output_cs));
	}
}

/// Crush the input. Returns 0 on success, otherwise the error's exit code
/// (1 parse error, 2 self-check failure, 3 unsupported input); the output is
/// then the unchanged input and `shadercrusher_get_error` describes the error.
#[no_mangle]
pub extern "C" fn shadercrusher_crush(ptr: *mut ShaderCrusher) -> c_int {
	let shadercrusher = unsafe {
		assert!(!ptr.is_null());
		&mut *ptr
	};
	match shadercrusher.crush() {
		Ok(()) => 0,
		Err(e) => e.exit_code(),
	}
}

/// Message of the last failed crush, or NULL. Owned by the crusher; valid
/// until the next `shadercrusher_set_input`/`shadercrusher_crush`/free.
#[no_mangle]
pub extern "C" fn shadercrusher_get_error(ptr: *mut ShaderCrusher) -> *const c_char {
	let shadercrusher = unsafe {
		assert!(!ptr.is_null());
		&*ptr
	};
	match &shadercrusher.last_error_c {
		Some(c) => c.as_ptr(),
		None => std::ptr::null(),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn crush(src: &str) -> String {
		crush_str(src, &Options::default()).expect("crush failed").0
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

	#[test]
	fn parse_failure_is_an_error_with_passthrough_output() {
		let src = "void main() { float a = ; }\n";
		let err = crush_str(src, &Options::default()).unwrap_err();
		assert!(matches!(err, CrushError::Parse(_)), "{err:?}");
		assert_eq!(err.exit_code(), 1);
		let mut sc = ShaderCrusher::new();
		sc.set_input(src);
		assert!(sc.crush().is_err());
		assert_eq!(
			sc.get_output(),
			src,
			"failed crush must pass the input through"
		);
		assert!(sc.last_error().is_some());
	}

	#[test]
	fn unparsed_trailing_input_is_an_error() {
		// The parser accepts the first declaration and stops before the garbage.
		let src = "uniform float some_value;\n@@@ not glsl @@@\nvoid main() { gl_FragColor = vec4(some_value); }\n";
		let err = crush_str(src, &Options::default()).unwrap_err();
		match err {
			CrushError::PartialParse { consumed, rest } => {
				assert_eq!(consumed, "uniform float some_value;\n".len(), "{rest}");
				assert!(rest.starts_with("@@@"), "{rest}");
			},
			e => panic!("expected PartialParse, got {e:?}"),
		}
	}

	#[test]
	fn legacy_builtin_functions_are_never_renamed() {
		let out = crush(
			"#version 110\nuniform sampler2D tex_a;\nuniform sampler2DRect tex_b;\nuniform samplerCube tex_c;\nuniform sampler2DShadow tex_d;\nvoid main() {\n\tgl_Position = ftransform();\n\tvec4 accum = texture2D(tex_a, gl_MultiTexCoord0.xy) + texture2DRect(tex_b, gl_MultiTexCoord0.xy) + textureCube(tex_c, gl_Normal) + shadow2D(tex_d, gl_Normal) + vec4(noise1(gl_Normal.x));\n\tgl_FrontColor = accum;\n}\n",
		);
		for f in [
			"ftransform()",
			"texture2D(",
			"texture2DRect(",
			"textureCube(",
			"shadow2D(",
			"noise1(",
		] {
			assert!(out.contains(f), "{f} was renamed: {out}");
		}
		assert!(!out.contains("tex_a") && !out.contains("accum"), "{out}");
	}

	#[test]
	fn builtin_struct_members_are_never_renamed() {
		let out = crush(
			"#version 110\nstruct Light { vec3 dir; float intensity; };\nuniform Light light_src;\nvoid main() {\n\tvec4 p = gl_LightSource[0].position;\n\tvec4 d = gl_FrontMaterial.diffuse * gl_Fog.color;\n\tgl_FragColor = p + d + vec4(light_src.dir, light_src.intensity) + vec4(gl_DepthRange.near, gl_DepthRange.far, gl_Point.size, 1.0);\n}\n",
		);
		for m in [".position", ".diffuse", ".color", ".near", ".far", ".size"] {
			assert!(out.contains(m), "{m} was renamed: {out}");
		}
		assert!(
			!out.contains("intensity"),
			"user struct field must be crushed: {out}"
		);
	}

	#[test]
	fn macro_lines_are_left_alone() {
		let out = crush(
			"#version 110\n#define SQ(v) ((v)*(v))\n#define NEG(q) -(q)\n#define HALF (scale_factor*0.5)\nuniform float scale_factor;\nuniform float other_value;\nvoid main() {\n\tfloat v = other_value;\n\tgl_FragColor = vec4(SQ(v) + NEG(v) + HALF);\n}\n",
		);
		assert!(out.contains("#define SQ(v) ((v)*(v))\n"), "{out}");
		assert!(out.contains("#define NEG(q) -(q)\n"), "{out}");
		assert!(out.contains("#define HALF (scale_factor*0.5)\n"), "{out}");
		assert!(
			out.contains("uniform float scale_factor;"),
			"macro body reference must pin the uniform: {out}"
		);
		assert!(!out.contains("other_value"), "{out}");
	}

	#[test]
	fn identifiers_declared_inside_macro_bodies_are_pinned() {
		// piglit CorrectPreprocess5.frag: `sum` only exists inside the macro
		let out = crush(
			"#define test1 int sum = 1;\nvoid main(void)\n{\n test1\n sum = 2;\n gl_FragColor = vec4(float(sum));\n}\n",
		);
		assert!(out.contains("sum=2;"), "{out}");
		assert!(out.contains("float(sum)"), "{out}");
	}

	#[test]
	fn layout_qualifier_names_are_never_renamed() {
		let out = crush(
			"#version 330\nlayout(location = 0) in vec4 in_position;\nlayout(location = 1) out vec4 out_color;\nlayout(std140, binding = 2) uniform Block { vec4 block_member; } block_inst;\nvoid main() { out_color = in_position + block_inst.block_member; }\n",
		);
		assert!(out.contains("location=0"), "{out}");
		assert!(out.contains("location=1"), "{out}");
		assert!(out.contains("std140,binding=2"), "{out}");
		assert!(
			out.contains("uniform Block{"),
			"block name is API-visible: {out}"
		);
		assert!(
			out.contains("block_member"),
			"block members are API-visible: {out}"
		);
		assert!(
			!out.contains("block_inst"),
			"block instance names are private: {out}"
		);
		assert!(!out.contains("in_position"), "{out}");
	}

	#[test]
	fn c_api_reports_errors() {
		unsafe {
			let sc = shadercrusher_new();
			let src = CString::new("void main() { float a = ; }").unwrap();
			shadercrusher_set_input(sc, src.as_ptr());
			assert_eq!(shadercrusher_crush(sc), 1);
			let msg = shadercrusher_get_error(sc);
			assert!(!msg.is_null());
			assert!(CStr::from_ptr(msg)
				.to_str()
				.unwrap()
				.starts_with("parse error"));
			let ok = CString::new("void main() { gl_FragColor = vec4(1.0); }").unwrap();
			shadercrusher_set_input(sc, ok.as_ptr());
			assert!(shadercrusher_get_error(sc).is_null());
			assert_eq!(shadercrusher_crush(sc), 0);
			let key = CString::new("bogus").unwrap();
			assert_eq!(shadercrusher_set_option(sc, key.as_ptr(), 1), -1);
			let key = CString::new("verbose").unwrap();
			assert_eq!(shadercrusher_set_option(sc, key.as_ptr(), 1), 0);
			shadercrusher_free(sc);
		}
	}
}
