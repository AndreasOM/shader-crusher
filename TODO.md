# In Progress

# Next

# TODO
[ ] Silent truncation: glsl's `run_parser` ignores unconsumed input, so the first unparseable top-level construct after a valid declaration ends the shader with exit 0 (e.g. `#if 0` block with non-GLSL inside). Exact fix = 3 lines in a vendored/patched glsl; fallback = heuristic guard comparing `{`/`;` counts of comment-stripped input vs output.
[ ] Parser gaps (need patched glsl): `while (bool b = expr)` (`condition` tries `expr` before `condition_assignment`), `m[i,j]` comma in subscript, preprocessor directives inside function bodies, `precision` keyword dropped by transpiler, comma-initializer parens dropped by transpiler.
[ ] Count identifier usage
[ ] Fix braces mess (or wait for glsl crate to fix it)
[ ] Remove dead #pragma
[ ] Use smarted identifier crushing
[ ] Add dead code removal

# Future
[ ] Allow crushing of multiple shaders, and create multiple outputs with include-lists

# DONE

## Unreleased
[x] Add `just piglit` recipes to run piglit's glslparsertest corpus through the crusher
[x] Never rename reserved identifiers (`gl_*`, `__`)
[x] Strip comments from preprocessor directive lines (parse `#extension ... // c`, stop leaking `#define` comments)

## Released

### Version 0.0
