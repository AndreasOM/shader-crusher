
use libc::{c_char};
use std::ffi::CStr;
use std::collections::HashMap;

use glsl::parser::Parse;
use glsl::syntax::ShaderStage;

use glsl::syntax::{CompoundStatement, Expr, SingleDeclaration, Statement, TypeSpecifierNonArray};
use glsl::syntax::*;
use glsl::visitor::{Host, Visit, Visitor};

use regex::Regex;

struct IdentEntry{
	crushed_name: String,
	count: u32,
}

impl std::fmt::Debug for IdentEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{} (*{})", self.crushed_name, self.count )
    }
}

impl IdentEntry{
	pub fn new( n: &str ) -> IdentEntry {
		IdentEntry{
			crushed_name: n.to_string(),
			count: 0,
		}
	}
	fn set_crushed_name( &mut self, cn: &str ) {
		self.crushed_name = cn.to_string();
	}
}

struct IdentMap{
	entries: HashMap<String, IdentEntry>,	
}


impl std::fmt::Debug for IdentMap {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "entries: {:#?}", self.entries)
    }
}

impl IdentMap{
	pub fn new() -> IdentMap {
		IdentMap {
			entries: HashMap::new(),
		}
	}
	fn crush(&mut self) {
		let mut candidates = Vec::new();
		// :TODO: be smarter ;)
		// :TODO: e.g. count frequency of characters in input and use most used ones
		// :TODO: provide more than 26 candidates, or generate them on the fly when needed
		for c in (b'a'..=b'z').rev() {
			let c = c as char;
			candidates.push( c.to_string() );
		}
		println!("Best candidates {:?}", candidates );
//		let mut count_index: Vec<(&String, &u32)> = self.entries.iter().map(|a|
//			(a.0, &a.1.count)	// :TODO: count might be a bit simplistic here, total "cost" might be a better measure
//		).collect::<Vec<(&String, &u32)>>().clone();
		let mut count_index = Vec::new();
		for e in self.entries.iter(){
			count_index.push( ( e.0.clone(), e.1.count ) );
		}
		count_index.sort_by(|a,b|
			if b.1 != a.1 {
				b.1.cmp(&a.1)
			} else {
				a.0.cmp(&b.0)
			}
		);
		println!("{:?}", count_index);
		for k in count_index {
			match self.entries.get_mut( &k.0 ) {
				None => {}, // :WTF:
				Some( e ) => {
					let cn = match candidates.pop() {
						None => e.crushed_name.clone(),
						Some( cn ) => cn,
					};
					println!("Crushing {:?} to {:?}", e, cn );
					e.set_crushed_name( &cn );
				}
			}
		}
	}
	fn get_crushed_name(&self, n: &str ) -> Option< String > {
		self.entries.get( n ).map(|a| a.crushed_name.clone() )
	}
	fn add( &mut self, n: &str ) {
		let mut e = self.entries.entry(n.to_string()).or_insert_with(||IdentEntry::new( &n ));
		e.count += 1;
	}
}

#[derive(Debug,PartialEq)]
enum CounterPhase {
	Analysing,
	Crushing,
}

struct Counter {
	phase: CounterPhase,
	blacklist: Vec<String>,
	crushing: bool,
	identifiers_crushed: IdentMap,
	identifiers_uncrushed: IdentMap,
}

impl Counter {
	pub fn new() -> Counter {
		Counter {
			phase: CounterPhase::Analysing,
			blacklist: vec![ "main".to_string() ],
			crushing: true,
			identifiers_crushed: IdentMap::new(),
			identifiers_uncrushed: IdentMap::new(),
		}
	}

	pub fn crush_names( &mut self ) {
		self.identifiers_crushed.crush();
	}
}
impl Visitor for Counter {
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
	fn visit_preprocessor_pragma(&mut self, pragma: &mut PreprocessorPragma) -> Visit {

//		println!("Pragma: {:?} - {:?}", pragma, self.crushing );
		match pragma.command.as_ref() {
			"SHADER_CRUSHER_OFF" => {
				self.crushing = false;
				pragma.command = "".to_string();	// no idea how to remove the pragma completely :(
				println!("== Crusher: Off ==");
			},
			"SHADER_CRUSHER_ON" => {
				self.crushing = true;
				pragma.command = "".to_string();
				println!("== Crusher: On ==");
			},
			_ => {

			},
		}
		Visit::Children
	}
	fn visit_identifier(&mut self, e: &mut Identifier) -> Visit {
//		println!("Expr: {:?}", e );
		match e {
			Identifier( i ) => {
				match self.phase {
					CounterPhase::Crushing => {
//						println!("Expr Identifier {:?}", i );
						match self.identifiers_crushed.get_crushed_name( i ) {
							Some( n ) => {
								println!("Identifier: Found {:?} for {:?}", n, i );
								*e = Identifier( n.to_string() );
							},
							None => {
//								println!("No crushed version of {:?} found", i );
							},
						}
					},
					CounterPhase::Analysing => {
						// do nothing
					}
				}
			},
			_ => {

			},
		}
		Visit::Children
	}
	// we are only interested in single declaration with a name
	fn visit_single_declaration(&mut self, declaration: &mut SingleDeclaration) -> Visit {
//		println!("{:#?}", declaration );
		match &declaration.name {
			None => {

			},
			Some( name ) => {
				println!("{:?}", name );
				let n = name.to_string();
				match self.phase {
					CounterPhase::Analysing => {
						self.add_identifier( &n );
					},
					CounterPhase::Crushing => {
/*						
						match self.identifiers_crushed.get_crushed_name( &n ) {
							Some( cn ) => {
								println!("Declaration: Found {:?} for {:?}", cn, n );
// NOPE								declaration.name = Some( Identifier( cn.to_string() ) );
							},
							None => {
								println!("No crushed version of {:?} found", n );
							},
						}
*/
					}
				}
			},
		}
	// do not go deeper
//		Visit::Parent
	// DO go deeper
		Visit::Children
	}
	/*
	fn visit_arrayed_identifier(&mut self, ai: &mut ArrayedIdentifier) -> Visit {
		println!("visit_arrayed_identifier {:?}", ai );
		Visit::Children
	}
	*/
	fn visit_function_prototype(&mut self, fp: &mut FunctionPrototype) -> Visit {
//		println!("{:?}", fp );
//		println!("{}", fp.name );
		match self.phase {
			CounterPhase::Analysing => {
//q				self.add_identifier( &fp.name.as_str() );
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

}

impl Counter {
	fn add_identifier( &mut self, n: &str ) {
		let blacklisted = self.blacklist.contains( &n.to_string() );
		if self.crushing && !blacklisted {
			self.identifiers_crushed.add( &n );
		} else {
			self.identifiers_uncrushed.add( &n );
		}
	}	
}

pub struct ShaderCrusher {
	input: String,
	output: String,
	input_entropy: f32,
	output_entropy: f32,
}

impl ShaderCrusher {
	pub fn new() -> ShaderCrusher {
		ShaderCrusher {
			input: String::new(),
			output: String::new(),
			input_entropy: 0.0,
			output_entropy: 0.0,
		}
	}
	fn recalc_entropy( &mut self ) {
//		self.input_entropy = entropy::shannon_entropy( self.input.as_bytes() );
//		self.output_entropy = entropy::shannon_entropy( self.output.as_bytes() );
		self.input_entropy = entropy::metric_entropy( self.input.as_bytes() );
		self.output_entropy = entropy::metric_entropy( self.output.as_bytes() );
	}
	pub fn set_input( &mut self, input: &str ) {
		self.input = input.to_string();
		self.output = self.input.clone();

		self.recalc_entropy();
	}
	pub fn get_output( &self ) -> String {
		self.output.clone()
	}

	pub fn get_input_entropy( &self ) -> f32 {
		self.input_entropy
	}

	pub fn get_output_entropy( &self ) -> f32 {
		self.output_entropy
	}

	pub fn crush( &mut self ) {
		let mut stage = ShaderStage::parse(&self.input);
//		println!("Stage: {:?}", stage);
		let mut stage = match stage {
			Err( e ) => {
				println!("Error parsing shader {:?}", e );
				return;
			},
			Ok( stage ) => {
//				println!("Parsed shader {:#?}", stage );
				stage
			}
		};

//		let mut compound = stage.clone();
		let mut counter = Counter::new();
		stage.visit(&mut counter);
		counter.crush_names();
		// :TODO: fixup crushed identifiers names
		// skip crushing for now
		counter.phase = CounterPhase::Crushing;
		stage.visit(&mut counter);
		println!("Stats:\n-------");
		println!("Crushed Varnames: {:?}", counter.identifiers_crushed );
		println!("Uncrushed Varnames: {:?}", counter.identifiers_uncrushed );
        let mut glsl_buffer = String::new();
        let r = glsl::transpiler::glsl::show_translation_unit(&mut glsl_buffer, &stage);
//        println!("r {:?}", r);
//        println!("r {}", r);
//        let pr: PrettyPrint = From::from(stage);// as &PrettyPrint;
//		PrettyPrint::print_shaderstage( &stage );
//        println!("{:?}", pr);

		// cleanup empty pragmas
		let re = Regex::new(r"(?m)^\s*#\s*pragma\s*$").unwrap();
		let glsl_buffer = re.replace_all(
			&glsl_buffer,
			|c: &regex::Captures|{
//				println!("{:?}", c );
				"".to_string()
			}
		);

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
		let glsl_buffer = re.replace_all(
			&glsl_buffer,
			|c: &regex::Captures|{
//				println!("{:?}", c );
				let inner = c.get(1).map_or("", |m| m.as_str() );
//				println!("{}", inner );
				format!("({}", inner).clone()
			}
		);
println!("====");
		let re = Regex::new(r"(?m)\(\(([a-zA-Z0-9.]+)\)").unwrap();
		let glsl_buffer = re.replace_all(
			&glsl_buffer,
			|c: &regex::Captures|{
//				println!("{:?}", c );
				let inner = c.get(1).map_or("", |m| m.as_str() );
//				println!("{}", inner );
				format!("({}", inner).clone()
			}
		);

println!("====");

//		let re = Regex::new(r"(?m)([\n\s-+*]+)\(([a-zA-Z0-9.]+)\)").unwrap();
//		let re = Regex::new(r"(?m)([\n[[:space:]]-+*]+)\(([a-zA-Z0-9.]+)\)").unwrap();
//		let re = Regex::new(r"(?m)([\n[[:space:]]-+*<>=]+)\(([a-zA-Z0-9.]+)\)").unwrap();
//		let re = Regex::new(r"(?m)([\n-+*<>=]+)\(([a-zA-Z0-9.]+)\)").unwrap();
		let re = Regex::new(r"(?m)([-+*<>=]+)\(([a-zA-Z0-9.]+)\)").unwrap();
		
		let glsl_buffer = re.replace_all(
			&glsl_buffer,
			|c: &regex::Captures|{
				println!("{:?}", c );
				let prefix = c.get(1).map_or("", |m| m.as_str() );
				let inner = c.get(2).map_or("", |m| m.as_str() );
				println!("{}{}", prefix, inner );
				format!("{}{}", prefix, inner).clone()
			}
		);

        self.output = glsl_buffer.to_string();
        self.recalc_entropy();
        let il = self.input.len();
        let ie = self.input_entropy;
        let it = il as f32 * ie;
        let ol = self.output.len();
        let oe = self.output_entropy;
        let ot = ol as f32 * oe;
        println!("Input  Size: {}, Entropy: {} => {}", il, ie, it );
        println!("Output Size: {}, Entropy: {} => {}", ol, oe, ot );
	}
}


// API
#[no_mangle]
pub unsafe extern "C" fn shadercrusher_new() -> *mut ShaderCrusher {
	Box::into_raw(Box::new(ShaderCrusher::new()))
}

#[no_mangle]
pub extern fn shadercrusher_free(ptr: *mut ShaderCrusher) {
    if ptr.is_null() { return }
    unsafe { Box::from_raw(ptr); }
}

#[no_mangle]
pub extern fn shadercrusher_set_input(ptr: *mut ShaderCrusher, input: *const c_char) {
	let shadercrusher = unsafe {
	    assert!(!ptr.is_null());
	    &mut *ptr
	};
	let input = unsafe {
	    assert!(!input.is_null());
	    CStr::from_ptr(input)
	};
	let input = input.to_str().unwrap();
	shadercrusher.set_input( input );
}
