# piglit corpus numbers

`just piglit` results per implementation step. Sizes are summed over every
*crushed* file (status `OK`, `ROUNDTRIP_GROW`, `GL_RENAMED`, `VALIDATE_FAIL`)
of the `expect_result: pass` shaders in piglit's `tests/glslparsertest`
(`main`); the first column is `OK / crushed`. Reports live next to this file;
diff two with `PIGLIT_BASELINE=bench/<old>.tsv just piglit`. Record a step
with `just piglit-record <name>`.

| step | OK / crushed | raw in | raw out | gzip -9 | xz -9e | zstd -19 | notes |
|---|---|---|---|---|---|---|---|
| step0-baseline (2d48df0) | 207 / 207 | 216684 | 60955 | 38349 | 44608 | 32351 | glsl 7.0 transpiler + regex, flat renaming; old 4-column report, compressed sizes computed separately |
| step1-harness (f7f788c) | 187 / 205 | 183535 | 60943 | 38239 | 44464 | 32313 | same output; exit codes, remainder check (2 silently truncated files now PARSE_FAIL), ROUNDTRIP_GROW measured (18 files, old renamer not idempotent) |
| step2-protect (b75d013) | 187 / 205 | 183535 | 62334 | 38594 | 44828 | 32689 | correctness: legacy builtins (texture2D…), builtin struct members, macro names/bodies/#if idents, block names/members, layout ids never renamed; old regex passes dropped |
| step3-printer (ffc85a3) | 206 / 206 | 183874 | 53381 | 36343 | 42516 | 30542 | own whitespace-free printer (precision, `a- -b`, comma parens, shortest floats), `(void)`→`()`, `{}`→`;`; output re-parsed and compared to the AST; all ROUNDTRIP_* gone |
