use std::env;
use std::fs::File;
use std::io::Write;
use std::path::Path;

fn all_combinations(input: &[u8; 4], l: u8) -> Vec<String> {
	let mut r = Vec::new();
	if l <= 0 {
	} else if l == 1 {
		for i in input {
			r.push(format!("{}", *i as char).to_string());
		}
	} else {
		let s = all_combinations(&input, l - 1);
		for e in s {
			r.push(e.clone()); // :HACK: not true
			for i in input {
				r.push(format!("{}{}", e, *i as char).to_string());
			}
		}
	}
	r
}
fn main() {
	let out_dir = env::var("OUT_DIR").unwrap();
	let dest_path = Path::new(&out_dir).join("glsl_keywords.rs");
	let mut f = File::create(&dest_path).unwrap();

	f.write_all(b"
    	pub struct GlslKeywords {
    	}

    	impl GlslKeywords {
        	pub fn get() -> std::vec::Vec<String> {
        		let mut r = Vec::new();
        		r.push(\"main\".to_string());	// technically not a keyword according to the spec, but we get in trouble if we 'fix' it, so blocklist it here
    \n").unwrap();

	// blocklist all swizzles, accroding to the glsl spec mixing components from different sets is not legal
	// :TODO: would be better if we could recognize a swizzle during parsing
	for e in all_combinations(b"xywz", 4) {
		f.write_fmt(format_args!("\t\t\t\tr.push(\"{}\".to_string());\n", e))
			.unwrap();
	}
	for e in all_combinations(b"rgba", 4) {
		f.write_fmt(format_args!("\t\t\t\tr.push(\"{}\".to_string());\n", e))
			.unwrap();
	}
	for e in all_combinations(b"stpq", 4) {
		f.write_fmt(format_args!("\t\t\t\tr.push(\"{}\".to_string());\n", e))
			.unwrap();
	}

	let ks = include_str!("src/glsl_keywords.txt");
	for k in ks.split_whitespace() {
		f.write_fmt(format_args!("\t\t\t\tr.push(\"{}\".to_string());\n", k))
			.unwrap();
	}
	f.write_all(
		b"
    			r
        	}
    	}
    ",
	)
	.unwrap();

	println!("cargo:rerun-if-changed=src/glsl_keywords.txt");
}
