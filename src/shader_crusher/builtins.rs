//! Names the GLSL language owns: keywords, built-in functions, members of
//! built-in structures, reserved prefixes and swizzle selectors.
//!
//! The word lists live in `src/glsl/*.txt` (one word per line, `#` comments)
//! and are the union over every GLSL / GLSL ES version plus the extensions
//! glslang knows, so a name is treated as built-in regardless of `#version`.

use std::collections::HashSet;
use std::sync::OnceLock;

fn words(src: &'static str) -> HashSet<&'static str> {
	src.lines()
		.map(|l| l.split('#').next().unwrap_or(""))
		.flat_map(|l| l.split_whitespace())
		.collect()
}

fn keywords() -> &'static HashSet<&'static str> {
	static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
	SET.get_or_init(|| words(include_str!("../glsl/keywords.txt")))
}

fn builtin_functions() -> &'static HashSet<&'static str> {
	static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
	SET.get_or_init(|| words(include_str!("../glsl/builtin_functions.txt")))
}

fn builtin_members() -> &'static HashSet<&'static str> {
	static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
	SET.get_or_init(|| words(include_str!("../glsl/builtin_members.txt")))
}

/// Keyword or reserved word in any GLSL version (includes the built-in type
/// names such as `vec4`, `sampler2D`, `float16_t`).
pub fn is_keyword(s: &str) -> bool {
	keywords().contains(s)
}

/// Built-in function in any GLSL version or known extension.
pub fn is_builtin_function(s: &str) -> bool {
	builtin_functions().contains(s)
}

/// Member name of a built-in structure (`gl_LightSource[0].position`,
/// `gl_DepthRange.near`, ...). Only meaningful after a `.`.
pub fn is_builtin_member(s: &str) -> bool {
	builtin_members().contains(s)
}

/// Reserved by the spec: `gl_` / `GL_` prefix (built-in variables, predefined
/// macros and extension names) and anything containing `__`.
pub fn is_reserved(s: &str) -> bool {
	s.starts_with("gl_") || s.starts_with("GL_") || s.contains("__")
}

/// Vector component selector: 1–4 characters all from one of `xyzw`, `rgba`,
/// `stpq`. Swizzles live in their own namespace and are never renamed.
pub fn is_swizzle(s: &str) -> bool {
	if s.is_empty() || s.len() > 4 {
		return false;
	}
	["xyzw", "rgba", "stpq"]
		.iter()
		.any(|set| s.chars().all(|c| set.contains(c)))
}

/// A name the crusher must never invent: it would collide with the language
/// or the API in some GLSL version.
pub fn never_generate(s: &str) -> bool {
	s == "main" || is_reserved(s) || is_keyword(s) || is_builtin_function(s)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn lists_load_and_contain_expected_words() {
		for k in [
			"in",
			"do",
			"if",
			"vec4",
			"sampler2DRect",
			"precision",
			"struct",
			"true",
			"float16_t",
		] {
			assert!(is_keyword(k), "{k}");
		}
		// the merged tokens of the old keyword file are gone
		for k in [
			"usamplerCubeArraystruct",
			"inlinenoinline",
			"castnamespace",
			"removedTypes",
		] {
			assert!(!is_keyword(k) && !is_builtin_function(k), "{k}");
		}
		for f in [
			"texture2D",
			"texture2DRect",
			"texture2DProj",
			"texture1D",
			"textureCube",
			"shadow2D",
			"shadow2DRect",
			"ftransform",
			"noise1",
			"texture",
			"texelFetch",
			"length",
			"dFdx",
			"EmitVertex",
		] {
			assert!(is_builtin_function(f), "{f}");
		}
		for m in [
			"position",
			"diffuse",
			"specular",
			"halfVector",
			"near",
			"far",
			"color",
			"size",
		] {
			assert!(is_builtin_member(m), "{m}");
		}
		assert!(!is_builtin_member("foo"));
		// no 1- or 2-letter builtin function, so the short pool only loses keywords
		assert!(builtin_functions().iter().all(|f| f.len() > 2));
		assert!(keywords().iter().filter(|k| k.len() <= 2).count() == 3);
	}

	#[test]
	fn reserved_and_swizzle() {
		assert!(is_reserved("gl_FragColor"));
		assert!(is_reserved("GL_ES"));
		assert!(is_reserved("my__var"));
		assert!(!is_reserved("_private"));
		assert!(!is_reserved("glFoo"));
		for s in ["x", "xyzw", "rgb", "stpq", "wzyx", "aa"] {
			assert!(is_swizzle(s), "{s}");
		}
		for s in ["", "xr", "xyzwx", "foo", "xs", "1"] {
			assert!(!is_swizzle(s), "{s}");
		}
		assert!(never_generate("main"));
		assert!(never_generate("do"));
		assert!(never_generate("dot"));
		assert!(!never_generate("ab"));
	}
}
