# Danger - Status
Works for me, but might wipe your harddisk.

![Windows Status](https://github.com/AndreasOM/shader-crusher/workflows/Rust-Windows/badge.svg)
![MacOS Status](https://github.com/AndreasOM/shader-crusher/workflows/Rust-MacOS/badge.svg)
![Linux Status](https://github.com/AndreasOM/shader-crusher/workflows/Rust-Linux/badge.svg)

# What?

Takes a glsl shader, removes white space, comments, etc, and replaces symbols/identifiers/type names by shorter ones that compress better.

The goal is the smallest shader *after* compression (Crinkler, kkrunchy, UPX, ...), without changing what it does:

- identifiers are resolved with a real scope model (GLSL 1.10–4.60, ES 1.00–3.20 rules: parameters+body are one scope,
  loop headers share the body's scope, `int x = x;` binds the outer `x`, struct members have their own namespace),
  then renamed per scope: a local may reuse the name of any outer symbol it does not use (spec-legal "hiding";
  `--no-shadowing` turns that off),
- names are chosen Shader-Minifier style: the letter whose bigrams with the surrounding characters are most frequent
  (`--score count|bigram|freq`),
- always-safe rewrites: `float a;float b;` → `float a,b;` (also struct members), `{x;}` → `x;`, `x=x+y` → `x+=y`,
  `(void)` → `()`, `in` dropped from parameters, `+x` → `x`, macro bodies squeezed, shortest float literals
  (`1.0` → `1.`, `0.5` → `.5`, `100.0` → `1e2`),
- **self-check**: the output is parsed again and must be the same syntax tree with the same identifier binding;
  anything else is an error, never silently shipped.

Never renamed: keywords and reserved words of every GLSL version, every built-in function (`texture2D`, `ftransform`,
...), `gl_*`/`GL_*`/`__` names, members of built-in structs (`gl_LightSource[0].position`), interface block names and
members, layout qualifier names, `main`, macro names and every identifier that appears inside a `#define` body or
`#if` condition (macros are opaque text), and anything you protect (below).

# Why?

I got tired of installing mono to get shader-minifier working,
and needed something that I could embedd into my tools.

# Why rust?

Because.
And I want to learn rust.

And it's portable, and fast, and future proof.

# Usage

## Commandline

```
cargo run -- crush --help

cargo run -- crush --input shader.glsl --output shader_crushed.glsl
```

Without `--output` the crushed shader goes to stdout; a one-line statistic goes to stderr
(`--verbose` prints every symbol with its new name and why it was kept).

Use ```--blocklist "dont,crush,these"```
or
```glsl

// code

#pragma SHADER_CRUSHER_OFF

// code

#pragma SHADER_CRUSHER_ON

// code
```
to keep certain identifiers untouched, e.g. uniforms that you need to resolve externally. Everything declared in an OFF
region keeps its name everywhere (including the members of protected struct uniforms). Alternatively let the crusher
rename them and read the mapping back: `--emit-map names.tsv` writes `original<TAB>new` for every renamed global.

Shaders that are linked together are crushed independently: protect the names they share (varyings, uniforms) or
crush them with the same `--emit-map` discipline.

Other switches: `--no-rename`, `--no-simplify` / `--no-rewrite merge-decls,unwrap-blocks,...` (one rewrite at a
time), `--no-shadowing`, `--score freq`, `--no-selfcheck`.

Exit codes: 0 ok; 1 the input did not parse (or only a prefix of it did — nothing is silently truncated);
2 the self-check failed; 3 unsupported input. On any error the output is the unchanged input.

Input may use LF, CRLF or CR line endings; output is always LF.

Known parser limits (the output is then the unchanged input, exit 1): `while (bool b = ...)`, a comma expression inside
`[]`, preprocessor directives inside function bodies.

## Piglit corpus

[piglit](https://gitlab.freedesktop.org/mesa/piglit)'s `tests/glslparsertest` suite (~530 GLSL 1.10–1.30 shaders, each tagged `expect_result: pass|fail`) doubles as a regression corpus:

```
just piglit-fetch          # download tests/glslparsertest into ./piglit (gitignored)
just piglit                # crush every `expect_result: pass` shader, report, gate
just piglit 'loop-*'       # only files matching a glob
PIGLIT_BASELINE=bench/step6-simplify.tsv just piglit   # per-file size deltas against an earlier run
PIGLIT_ARGS="--score freq" just piglit                  # pass flags to every crush
just piglit-record my-change                            # keep the report + totals in bench/
just piglit-clean
```

Each shader ends up as one of `OK`, `PARSE_FAIL`, `SELFCHECK_FAIL`, `UNSUPPORTED`, `CRASH`, `ROUNDTRIP_FAIL`/`ROUNDTRIP_GROW`
(crushing the output again fails / grows it), `GL_RENAMED` (a `gl_*` name vanished) or `VALIDATE_FAIL`
(only with `GLSL_VALIDATOR=glslangValidator` set). Details land in `piglit/results/report.tsv` with raw, gzip, xz and
zstd sizes per file; `bench/README.md` tracks the totals per change. `cargo test` also runs the corpus when it is present.

## Embedded/Linked

From C/C++

```c++
shader_crusher::ShaderCrusher* pShaderCrusher = shader_crusher::shadercrusher_new();
shader_crusher::shadercrusher_set_input( pShaderCrusher, fragmentString.c_str() );
shader_crusher::shadercrusher_blocklist_identifier( pShaderCrusher, "iTime" );   // optional
if( shader_crusher::shadercrusher_crush( pShaderCrusher ) != 0 )                 // 0 = ok, else see exit codes
	fprintf( stderr, "%s\n", shader_crusher::shadercrusher_get_error( pShaderCrusher ) );
char* pOutput = shader_crusher::shadercrusher_get_ouput( pShaderCrusher );      // the input itself on error
fragmentString = std::string( pOutput );
shader_crusher::shadercrusher_free_ouput( pShaderCrusher, pOutput );
shader_crusher::shadercrusher_free( pShaderCrusher );
```
`shadercrusher_set_option( p, "shadowing", 0 )` etc. sets the boolean options (`verbose`, `rename`, `simplify`,
`shadowing`, `selfcheck`) and `scoring` (0 frequency, 1 bigram, 2 bigram-count).
Don't forget do include the cbindgen generated header file, and link against the lib.

# Stats

On piglit's 206 crushable `glslparsertest` shaders (183,874 bytes of GLSL 1.10–1.30): 49,091 bytes raw (26.7%),
and vs. 0.6.0-alpha −19% raw, −7% gzip, −6% xz, −8% zstd (see `bench/README.md`). A 4.8 KB ray-marching fragment
shader crushes to 2,400 bytes / 1,139 gzip.

# Parser

`src/glsl/` is a vendored copy of the [glsl](https://github.com/phaazon/glsl) crate 7.0.0 (BSD-3-Clause, see
`LICENSE-glsl`); its transpiler was replaced by the crusher's own printer and the parser reports unconsumed input.

# Future

(aka not going to happen anytime soon)

 - Dead code removal
 - Smart replacement of repeated blocks
 - Multishader passes for smart extraction of shared code

# Help

- Run this against your shader, and see if it breaks anything, and what compression ratio you get.
- If the self-check ever reports a mismatch, please file the shader: that is exactly the bug report it exists for.
