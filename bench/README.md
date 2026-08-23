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
