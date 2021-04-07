
use clap::{Arg, App, SubCommand};


use shader_crusher::ShaderCrusher;
use std::fs;

pub fn main() {
	let matches = App::new("shader-crusher")
					.version("0.1")
					.author("Andreas N. <andreas@omni-mad.com>")
					.about("Crushes glsl shaders.")
					.subcommand(SubCommand::with_name("test")
					)
					.subcommand(SubCommand::with_name("crush")
						.arg(Arg::with_name("input")
							.long("input")
							.value_name("INPUT")
							.help("Set the input filename")
							.takes_value(true)
						)
						.arg(Arg::with_name("output")
							.long("output")
							.value_name("OUTPUT")
							.help("Set the output filename")
							.takes_value(true)
						)
						.arg(Arg::with_name("blacklist")
							.long("blacklist")
							.value_name("BLACKLIST")
							.help("Add identifiers to blacklist")
							.takes_value(true)
						)						
						.arg(Arg::with_name("dump-input")
							.long("dump-input")
							.value_name("DUMP_INPUT")
							.takes_value(false)
						)
					)
					.get_matches();

	if let ("crush", Some( sub_matches ) ) = matches.subcommand() {
		let input = sub_matches.value_of("input").unwrap_or("input.glsl").to_string();
		let output = sub_matches.value_of("output").unwrap_or("").to_string();

		let data = fs::read_to_string(input).expect("// Unable to read file");
		match sub_matches.occurrences_of("dump-input") {
			0 => {},
			_ => {
				println!("{}", data );
			}
		};

		let mut sc = ShaderCrusher::new();
		match sub_matches.value_of( "blacklist" ){
			None => {},
			Some( bl ) => {
				for n in bl.split(",") {
					sc.blacklist_identifier( &n );
				}
			},
		};
		sc.set_input( &data );
		sc.crush();
		if output.len() == 0 {
			println!("Output:\n-----\n{}\n-----", sc.get_output() );
		} else {
			fs::write(output, sc.get_output()).expect("// Unable to write file");
		}
	} else { // just default to testing

		println!("ShaderCrusher - Testmode");
		let mut sc = ShaderCrusher::new();
		let input = r"
#version 410

#pragma

#pragma SHADER_CRUSHER_OFF

uniform float iTime;
layout (location=0) out vec4 co;
layout (location=0) in vec2 p;
#pragma SHADER_CRUSHER_ON

// totally useless function just for testing entropy
vec2 do_something_one( vec2 p )
{
	return p;
}

void main()
{
	vec2 pos = do_something_one( p );
	vec2 final_pos = do_something_one( pos );
	co = final_pos.xxyy;
}
";
		sc.set_input( &input );
		println!("Input         : >\n{:?}\n<", input );
		println!("Output        : >\n{:?}\n<", sc.get_output() );
		println!("---");
		sc.crush();
		println!("---");
		println!("Input         : >\n{:?}\n<", input );
		println!("Crushed Output: >\n{:?}\n<", sc.get_output() );
		println!("Crushed Output: >\n{}\n<", sc.get_output() );
	}
}
