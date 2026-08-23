//! Text-level preparation that runs before the GLSL parser.

/// GLSL (§3.2) accepts CR, LF and CRLF as line terminators, but the glsl
/// parser's directive lexer (`str_till_eol`) keeps a trailing `\r`, which
/// breaks `#pragma SHADER_CRUSHER_*` matching and leaks `\r` into `#define`
/// values. Normalize to LF up front; output is always LF.
pub fn normalize_line_endings(src: &str) -> String {
	src.replace("\r\n", "\n").replace('\r', "\n")
}

/// Remove `// ...` and single-line `/* ... */` comments from preprocessor
/// directive lines (`#extension`, `#define`, `#pragma`, ...).
///
/// The glsl parser handles comments in code, but directive lines are lexed
/// "till end of line": a trailing comment after `#extension` is a parse error,
/// and a comment after `#define X 1` would be copied into the output verbatim.
/// Non-directive lines are passed through untouched. A line continued with
/// a trailing `\` keeps the following line inside the directive.
/// Expects LF line endings (see `normalize_line_endings`); re-emits `\n`.
pub fn strip_directive_comments(src: &str) -> String {
	let mut out = String::with_capacity(src.len());
	let mut continued = false;
	for line in src.lines() {
		if continued || line.trim_start().starts_with('#') {
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
			let line = line.trim_end();
			continued = line.ends_with('\\');
			out.push_str(line);
		} else {
			continued = false;
			out.push_str(line);
		}
		out.push('\n');
	}
	if !src.ends_with('\n') && !src.is_empty() {
		out.pop();
	}
	out
}

#[cfg(test)]
mod tests {
	use super::*;

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
		// continuation lines belong to the directive
		assert_eq!(
			strip_directive_comments(
				"#define M a \\\n  b /* c */ \\\n  d // e\nfloat x; // keep\n"
			),
			"#define M a \\\n  b   \\\n  d\nfloat x; // keep\n"
		);
	}

	#[test]
	fn normalize_line_endings_handles_crlf_and_cr() {
		assert_eq!(normalize_line_endings("a\r\nb\rc\n"), "a\nb\nc\n");
		assert_eq!(normalize_line_endings("a\nb"), "a\nb");
		assert_eq!(normalize_line_endings(""), "");
	}
}
