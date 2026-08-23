//! Property test over piglit's glslparsertest corpus (`just piglit-fetch`
//! puts it into ./piglit; skipped when absent): every shader the parser
//! accepts must crush with the self-check on, crush again without growing,
//! and crush identically with every rewrite disabled.

use std::fs;
use std::path::{Path, PathBuf};

use shader_crusher::{crush_str, CrushError, Options};

fn expect_pass(src: &str) -> bool {
	src.lines().take(10).any(|l| {
		let l = l.trim_start_matches(|c: char| c == '/' || c == '*' || c == ' ' || c == '\t');
		l.starts_with("expect_result:") && l["expect_result:".len()..].trim() == "pass"
	})
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
	if let Ok(entries) = fs::read_dir(dir) {
		for e in entries.flatten() {
			let p = e.path();
			if p.is_dir() {
				collect(&p, out);
			} else if p.extension().is_some_and(|x| x == "vert" || x == "frag") {
				out.push(p);
			}
		}
	}
}

#[test]
fn piglit_corpus_crushes_and_verifies() {
	let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("piglit");
	if !root.join("glsl2").is_dir() {
		eprintln!("piglit corpus not fetched (just piglit-fetch); skipping");
		return;
	}
	let mut files = Vec::new();
	collect(&root.join("glsl2"), &mut files);
	collect(&root.join("shaders"), &mut files);
	files.sort();
	let mut crushed = 0;
	let mut failures = Vec::new();
	for f in files {
		let src = fs::read_to_string(&f).expect("read");
		if !expect_pass(&src) {
			continue;
		}
		let name = f.strip_prefix(&root).unwrap().display().to_string();
		let (out, _) = match crush_str(&src, &Options::default()) {
			Ok(r) => r,
			Err(CrushError::Parse(_)) | Err(CrushError::PartialParse { .. }) => continue,
			Err(e) => {
				failures.push(format!("{name}: {e}"));
				continue;
			},
		};
		crushed += 1;
		match crush_str(&out, &Options::default()) {
			Ok((again, _)) if again.len() <= out.len() => {},
			Ok((again, _)) => failures.push(format!(
				"{name}: re-crushing grew {} -> {}",
				out.len(),
				again.len()
			)),
			Err(e) => failures.push(format!("{name}: re-crushing failed: {e}")),
		}
		let plain = Options {
			simplify: false,
			..Options::default()
		};
		if let Err(e) = crush_str(&src, &plain) {
			failures.push(format!("{name}: crush without rewrites failed: {e}"));
		}
	}
	assert!(failures.is_empty(), "{}", failures.join("\n"));
	assert!(crushed > 150, "only {crushed} shaders crushed");
}
