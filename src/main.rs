use std::fs;
use std::io::Write;

use clap::{Arg, ArgAction, Command};
use shader_crusher::{Options, Rewrites, Scoring, ShaderCrusher};

pub fn main() {
	let matches = Command::new("shader-crusher")
		.version(env!("CARGO_PKG_VERSION"))
		.author("Andreas N. <andreas@omni-mad.com>")
		.about("Crushes glsl shaders.")
		.subcommand_required(true)
		.subcommand(
			Command::new("crush")
				.about("Crush one shader file")
				.arg(
					Arg::new("input")
						.long("input")
						.value_name("INPUT")
						.help("Set the input filename"),
				)
				.arg(
					Arg::new("output")
						.long("output")
						.value_name("OUTPUT")
						.help("Set the output filename (default: stdout)"),
				)
				.arg(
					Arg::new("blocklist")
						.long("blocklist")
						.value_name("BLOCKLIST")
						.help("Comma separated identifiers that must not be renamed"),
				)
				.arg(
					Arg::new("verbose")
						.long("verbose")
						.short('v')
						.action(ArgAction::SetTrue)
						.help("Per-identifier diagnostics on stderr"),
				)
				.arg(
					Arg::new("no-rename")
						.long("no-rename")
						.action(ArgAction::SetTrue)
						.help("Do not rename identifiers"),
				)
				.arg(
					Arg::new("no-simplify")
						.long("no-simplify")
						.action(ArgAction::SetTrue)
						.help("Do not apply AST-level rewrites"),
				)
				.arg(
					Arg::new("no-rewrite")
						.long("no-rewrite")
						.value_name("NAMES")
						.help(format!(
							"Comma separated rewrites to skip: {}",
							Rewrites::NAMES.join(", ")
						)),
				)
				.arg(
					Arg::new("no-shadowing")
						.long("no-shadowing")
						.action(ArgAction::SetTrue)
						.help("Never let a local reuse the name of a global, function or type"),
				)
				.arg(
					Arg::new("no-selfcheck")
						.long("no-selfcheck")
						.action(ArgAction::SetTrue)
						.help("Skip re-parsing the output to verify it"),
				)
				.arg(
					Arg::new("score")
						.long("score")
						.value_name("SCORING")
						.value_parser(["count", "bigram", "freq"])
						.default_value("count")
						.help("How new names are chosen: count = bigram contexts weighted by occurrence (best after compression), bigram = unweighted, freq = most frequent letter first"),
				)
				.arg(
					Arg::new("dump-input")
						.long("dump-input")
						.action(ArgAction::SetTrue)
						.help("Echo the input on stderr"),
				),
		)
		.get_matches();

	let Some(("crush", sub_matches)) = matches.subcommand() else {
		unreachable!("subcommand required");
	};
	let input = sub_matches
		.get_one::<String>("input")
		.map(|s| s.as_str())
		.unwrap_or("input.glsl")
		.to_string();
	let output = sub_matches
		.get_one::<String>("output")
		.map(|s| s.as_str())
		.unwrap_or("")
		.to_string();

	let data = match fs::read_to_string(&input) {
		Ok(d) => d,
		Err(e) => {
			eprintln!("error: cannot read {}: {}", input, e);
			std::process::exit(1);
		},
	};
	if sub_matches.get_flag("dump-input") {
		eprintln!("{}", data);
	}

	let mut rewrites = Rewrites::default();
	if let Some(names) = sub_matches.get_one::<String>("no-rewrite") {
		for name in names.split(',').map(str::trim).filter(|n| !n.is_empty()) {
			if !rewrites.disable(name) {
				eprintln!(
					"error: unknown rewrite {:?}; known: {}",
					name,
					Rewrites::NAMES.join(", ")
				);
				std::process::exit(1);
			}
		}
	}
	let opts = Options {
		blocklist: sub_matches
			.get_one::<String>("blocklist")
			.map(|bl| bl.split(',').map(|s| s.to_string()).collect())
			.unwrap_or_default(),
		verbose: sub_matches.get_flag("verbose"),
		rename: !sub_matches.get_flag("no-rename"),
		simplify: !sub_matches.get_flag("no-simplify"),
		rewrites,
		shadowing: !sub_matches.get_flag("no-shadowing"),
		selfcheck: !sub_matches.get_flag("no-selfcheck"),
		scoring: Scoring::parse(sub_matches.get_one::<String>("score").unwrap()).unwrap(),
	};

	let mut sc = ShaderCrusher::with_options(opts);
	sc.set_input(&data);
	let code = match sc.crush() {
		Ok(()) => {
			eprintln!("{}: {}", input, sc.stats());
			0
		},
		Err(e) => {
			eprintln!("error: {}: {}", input, e);
			eprintln!("error: output is the unchanged input");
			e.exit_code()
		},
	};
	if output.is_empty() {
		let mut stdout = std::io::stdout().lock();
		let _ = stdout.write_all(sc.get_output().as_bytes());
		let _ = stdout.flush();
	} else if let Err(e) = fs::write(&output, sc.get_output()) {
		eprintln!("error: cannot write {}: {}", output, e);
		std::process::exit(1);
	}
	std::process::exit(code);
}
