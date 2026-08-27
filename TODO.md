# In Progress

# Next
[ ] Parser gaps (patch in src/glsl): `while (bool b = expr)` (`condition` tries `expr` before `condition_assignment`), `m[i,j]` comma in subscript, preprocessor directives inside function bodies.

# TODO
[ ] Find a validator that catches driver-level breakage: `glslc`/glslang is spec-correct and happily accepts a variable shadowing a struct type, so it would not have caught the 0.7.1-alpha bug. The `--verbose` symbol-table clash scan did.
[ ] Rewrites that need type knowledge: `vec3(1.,1.,1.)` → `vec3(1)`, `if(c)x=a;else x=b;` → `x=c?a:b;`, `while` → `for`.
[ ] Top-level declaration hoisting/merging across functions (Shader Minifier does it; interacts with `precision` and `#if`).
[ ] Measure Crinkler/kkrunchy directly instead of gzip/xz/zstd proxies.
[ ] Dead code removal (unused functions, unused locals)

# Future
[ ] Allow crushing of multiple shaders, and create multiple outputs with include-lists

# DONE

## Unreleased

## Released

### Version 0.7.2-alpha
[x] Never give a struct type name to any other symbol (reported 2026-08-27 against 0.7.1-alpha: `struct S`->`t` plus a local `int i`->`t` made drivers read `while(t<9)` as a declaration; every shader with a `struct` was affected)
[x] Check the type-name invariant unconditionally, also with `--no-selfcheck`: the re-parse self-check cannot see it, because the shadowing it produces is spec-legal

### Version 0.7.0-alpha
[x] Vendor the glsl 7.0.0 parser in-tree (BSD-3-Clause); detect silently unparsed input (partial parse is an error)
[x] Errors with exit codes; passthrough output on error; quiet CLI, `--verbose` symbol table; C API returns status + `shadercrusher_get_error`
[x] Never rename built-in functions of any GLSL version (`texture2D`, `ftransform`, ...), built-in struct members, macro names/bodies/`#if` identifiers, interface block names/members, layout qualifier ids
[x] Own whitespace-free printer faithful to the parser (precision statements, `a- -b`, comma parens, hex for negative int constants, shortest float literals)
[x] Self-check: output re-parsed and compared to the intended AST and identifier binding
[x] Scope-aware renaming with a symbol table: per-scope name reuse (`--no-shadowing`), struct fields per struct, swizzle letters usable as names
[x] Occurrence-weighted bigram name scoring (`--score count|bigram|freq`)
[x] Rewrites: declaration/field merging, block unwrapping, `x=x+y`→`x+=y`, `(void)`/`in`/unary `+` removal, macro squeeze (`--no-rewrite NAME`)
[x] `--emit-map FILE` with the original→new names of renamed globals
[x] piglit harness with gzip/xz/zstd sizes, baselines (`PIGLIT_BASELINE`), `PIGLIT_ARGS`, `just piglit-record`; golden tests in data/golden; corpus property test

### Version 0.6.0-alpha
[x] Add `just piglit` recipes to run piglit's glslparsertest corpus through the crusher
[x] Never rename reserved identifiers (`gl_*`, `__`)
[x] Strip comments from preprocessor directive lines (parse `#extension ... // c`, stop leaking `#define` comments)
[x] Normalize line endings (CRLF/CR → LF); 0.5.0-alpha ignored `#pragma SHADER_CRUSHER_*` and leaked `\r` on CRLF sources
[x] Report crate version in `--version`

### Version 0.0
