//! Golden-file test: every `data/golden/*.glsl` crushed with default options
//! must equal the `.crushed` file next to it. Run with `UPDATE_GOLDEN=1` to
//! rewrite the expectations after an intentional output change.

use std::fs;
use std::path::PathBuf;

use shader_crusher::{crush_str, Options};

#[test]
fn golden_files() {
	let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/golden");
	let update = std::env::var_os("UPDATE_GOLDEN").is_some();
	let mut inputs: Vec<PathBuf> = fs::read_dir(&dir)
		.expect("data/golden")
		.map(|e| e.expect("entry").path())
		.filter(|p| p.extension().is_some_and(|x| x == "glsl"))
		.collect();
	inputs.sort();
	assert!(!inputs.is_empty(), "no golden inputs in {}", dir.display());

	let mut failures = Vec::new();
	for input in inputs {
		let name = input.file_name().unwrap().to_string_lossy().to_string();
		let src = fs::read_to_string(&input).expect("read input");
		let out = match crush_str(&src, &Options::default()) {
			Ok((out, _)) => out,
			Err(e) => {
				failures.push(format!("{name}: crush failed: {e}"));
				continue;
			},
		};
		let expected_path = input.with_extension("crushed");
		if update {
			fs::write(&expected_path, &out).expect("write golden");
			continue;
		}
		match fs::read_to_string(&expected_path) {
			Ok(expected) if expected == out => {},
			Ok(expected) => failures.push(format!(
				"{name}: output differs from {} (UPDATE_GOLDEN=1 cargo test to accept)\n--- expected\n{expected}\n--- got\n{out}\n",
				expected_path.display()
			)),
			Err(_) => failures.push(format!(
				"{name}: missing {} (UPDATE_GOLDEN=1 cargo test to create)",
				expected_path.display()
			)),
		}
	}
	assert!(failures.is_empty(), "{}", failures.join("\n"));
}
