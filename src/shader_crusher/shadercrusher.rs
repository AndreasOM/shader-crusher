use std::ffi::{CStr, CString};
use std::fmt;

use libc::{c_char, c_int};

use super::preprocess::{normalize_line_endings, strip_directive_comments};
use super::{printer, protect, rename, scope, selfcheck, simplify};
use super::{CrushError, Options, Scoring};
use crate::glsl::parser::parse_translation_unit_with_rest;

/// Numbers about the last successful crush.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Stats {
	pub input_len:      usize,
	pub output_len:     usize,
	pub input_entropy:  f32,
	pub output_entropy: f32,
	/// Distinct symbols that got a new name.
	pub renamed:        usize,
	/// Distinct symbols that kept their name (protected, reserved, ...).
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
			"{} -> {} bytes ({:.1}%), entropy {:.3} -> {:.3}, {} symbols renamed, {} kept",
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
	/// `(original, new)` for every renamed global (uniforms, attributes,
	/// varyings, functions, ...), in declaration order.
	rename_map:   Vec<(String, String)>,
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
			rename_map: Vec::new(),
			last_error: None,
			last_error_c: None,
		}
	}

	/// `(original, new)` name of every renamed global of the last crush:
	/// what an application must use to address uniforms/attributes by name.
	pub fn rename_map(&self) -> &[(String, String)] {
		&self.rename_map
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
		self.rename_map.clear();
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
			simplify::run(&mut stage, &self.options.rewrites);
		}

		let mut table = scope::resolve(&mut stage, &protection)?;
		if self.options.rename {
			// statistics text: pinned symbols as they will print, the rest as sentinels
			let mut stats_tree = stage.clone();
			rename::apply_pinned(&mut stats_tree, &table);
			let text = printer::print(&stats_tree);
			rename::assign(
				&mut table,
				&text,
				self.options.scoring,
				self.options.shadowing,
			);
		}
		rename::apply(&mut stage, &table);
		if verbose {
			table.dump();
		}

		let out = printer::print(&stage);
		if self.options.selfcheck {
			selfcheck::run(&out, &stage, &table)?;
		}

		self.output = out;
		self.rename_map = table
			.symbols
			.iter()
			.filter(|s| s.scope == 0)
			.filter_map(|s| s.new_name.as_ref().map(|n| (s.name.clone(), n.clone())))
			.collect();
		self.stats = Stats {
			input_len:      self.input.len(),
			output_len:     self.output.len(),
			input_entropy:  entropy::metric_entropy(self.input.as_bytes()),
			output_entropy: entropy::metric_entropy(self.output.as_bytes()),
			renamed:        table
				.symbols
				.iter()
				.filter(|s| s.new_name.is_some())
				.count(),
			kept:           table.symbols.iter().filter(|s| s.pinned.is_some()).count(),
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

	fn crush_with(src: &str, f: impl FnOnce(&mut Options)) -> String {
		let mut o = Options::default();
		f(&mut o);
		crush_str(src, &o).expect("crush failed").0
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
			out.contains("uniform float scale_factor,"),
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

	/// The identifier that follows the `n`th occurrence of `prefix`.
	fn ident_after<'a>(s: &'a str, prefix: &str, n: usize) -> &'a str {
		let rest = s.split(prefix).nth(n + 1).expect("prefix");
		let end = rest
			.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
			.unwrap_or(rest.len());
		&rest[..end]
	}

	#[test]
	fn locals_reuse_names_of_globals_they_do_not_use() {
		let src = "uniform float u_one; uniform float u_two; void main() { float local_x = u_one; gl_FragColor = vec4(local_x); }";
		let out = crush_with(src, |o| o.scoring = Scoring::Frequency);
		let (one, two) = (
			ident_after(&out, "uniform float ", 0),
			ident_after(&out, ",", 0),
		);
		assert_ne!(one, two);
		// main does not use u_two, so its local may take u_two's name
		assert_eq!(
			out,
			format!("uniform float {one},{two};void main(){{float {two}={one};gl_FragColor=vec4({two});}}")
		);
		let out = crush_with(src, |o| {
			o.shadowing = false;
			o.scoring = Scoring::Frequency;
		});
		let local = ident_after(&out, "void main(){float ", 0);
		assert_ne!(local, one);
		assert_ne!(local, two);
		assert_eq!(
			out,
			format!("uniform float {one},{two};void main(){{float {local}={one};gl_FragColor=vec4({local});}}")
		);
	}

	#[test]
	fn initializers_keep_binding_to_the_outer_name() {
		let out = crush("float v_outer = 1.; void main() { float v_outer = v_outer * 2.; gl_FragColor = vec4(v_outer); }");
		let outer = ident_after(&out, "float ", 0);
		let inner = ident_after(&out, "void main(){float ", 0);
		assert_ne!(outer, inner, "{out}");
		assert_eq!(
			out,
			format!("float {outer}=1.;void main(){{float {inner}={outer}*2.;gl_FragColor=vec4({inner});}}")
		);
	}

	#[test]
	fn nested_scopes_crush_and_verify() {
		let out = crush("void main() { for (int i = 0; i < 2; i++) { float k = float(i); } float i = 1.; if (i > 0.) float j = i; else { float j = 2.; } do { float m = i; } while (false); switch (1) { case 1: { int c = 0; } } gl_FragColor = vec4(i); }");
		assert!(!out.contains(" i ") && !out.contains("float k"), "{out}");
	}

	#[test]
	fn twenty_six_locals_get_single_letters() {
		let mut src = String::from("void main(){float acc=0.;");
		for i in 0..30 {
			src.push_str(&format!("float local_{i}={i}.;acc+=local_{i};"));
		}
		src.push_str("gl_FragColor=vec4(acc);}");
		let out = crush(&src);
		// every declarator of every `float ...;` declaration
		let names: Vec<&str> = out
			.split("float ")
			.skip(1)
			.flat_map(|s| s.split(';').next().unwrap().split(','))
			.map(|d| d.split('=').next().unwrap())
			.collect();
		assert_eq!(names.len(), 31, "{out}");
		assert!(names.iter().all(|n| n.len() == 1), "{names:?}");
		for n in ["x", "y", "z", "r", "g", "b", "a"] {
			assert!(
				names.contains(&n),
				"swizzle letters are valid variable names: {names:?}"
			);
		}
	}

	#[test]
	fn struct_fields_are_renamed_unless_swizzle_shaped() {
		let out = crush("struct S { vec3 pos; float w; vec2 uv; }; uniform S s; void main() { gl_FragColor = vec4(s.pos, s.w) + s.uv.xyxy; }");
		assert!(!out.contains("pos") && !out.contains("uv"), "{out}");
		assert!(out.contains(".w)"), "{out}");
		assert!(out.contains("float w;"), "{out}");
	}

	#[test]
	fn builtin_named_function_is_kept_but_builtin_named_variable_is_renamed() {
		let out = crush("float dot(float a, float b) { return a * b; } void main() { float length = 2.; float arr[2]; gl_FragColor = vec4(dot(length, 3.) * float(arr.length())); }");
		assert!(out.contains("float dot(") && out.contains("dot("), "{out}");
		assert!(out.contains(".length()"), "{out}");
		assert!(!out.contains("float length"), "{out}");
	}

	#[test]
	fn declarations_duplicated_by_the_preprocessor_rename_consistently() {
		let out = crush("#ifdef A\nuniform float dup_u;\n#else\nuniform float dup_u;\n#endif\nvoid main(){gl_FragColor=vec4(dup_u);}");
		assert!(!out.contains("dup_u"), "{out}");
		let decls: Vec<&str> = out.lines().filter(|l| l.starts_with("uniform")).collect();
		assert_eq!(decls.len(), 2);
		assert_eq!(decls[0], decls[1], "{out}");
	}

	#[test]
	fn overloads_prototypes_and_definitions_share_one_name() {
		let out = crush("float f_over(float a); float f_over(float a) { return a; } float f_over(vec2 a) { return a.x; } void main() { gl_FragColor = vec4(f_over(1.) + f_over(vec2(1.))); }");
		assert!(!out.contains("f_over"), "{out}");
		let name = out
			.split("float ")
			.nth(1)
			.unwrap()
			.split('(')
			.next()
			.unwrap();
		assert_eq!(out.matches(&format!("float {name}(")).count(), 3, "{out}");
		assert_eq!(out.matches(&format!("{name}(1.)")).count(), 1, "{out}");
		assert_eq!(
			out.matches(&format!("{name}(vec2(1.))")).count(),
			1,
			"{out}"
		);
	}

	#[test]
	fn invariant_redeclarations_follow_the_rename() {
		let out = crush("varying vec4 Color1; invariant Color1; invariant gl_Position; void main() { Color1 = vec4(1.); }");
		assert!(!out.contains("Color1"), "{out}");
		assert!(out.contains("invariant gl_Position;"), "{out}");
		let name = out
			.split("varying vec4 ")
			.nth(1)
			.unwrap()
			.split(';')
			.next()
			.unwrap();
		assert!(out.contains(&format!("invariant {name};")), "{out}");
	}

	#[test]
	fn struct_and_array_constructors_resolve() {
		let out = crush("struct S { float val; }; void main() { S arr[2] = S[2](S(1.), S(2.)); float f[2] = float[2](1., 2.); gl_FragColor = vec4(arr[0].val, f[1], 0., 1.); }");
		assert!(out.contains("[2](") && out.contains("float[2]("), "{out}");
		assert!(!out.contains("val"), "{out}");
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
