r:
	cargo run --release

d:
	cargo run

cr:
	cargo check --release

c:
	cargo check
	
fmt:
	cargo +nightly fmt

test-noisy:
	cargo test -- --nocapture

# --- piglit glslparsertest corpus --------------------------------------------
# Corpus: https://gitlab.freedesktop.org/mesa/piglit/-/tree/main/tests/glslparsertest
# Fetched into ./piglit (gitignored). Only shaders whose [config] header says
# `expect_result: pass` are run; `expect_result: fail` shaders are intentionally
# invalid GLSL and are skipped.

piglit_dir := "piglit"

# Download piglit's tests/glslparsertest into ./piglit (path-scoped tarball, ~70 KB)
piglit-fetch ref="main":
	#!/usr/bin/env sh
	set -eu
	url="https://gitlab.freedesktop.org/mesa/piglit/-/archive/{{ref}}/piglit-{{ref}}.tar.gz?path=tests/glslparsertest"
	rm -rf "{{piglit_dir}}"
	mkdir -p "{{piglit_dir}}"
	echo "Fetching $url"
	curl -fsSL "$url" | tar xz -C "{{piglit_dir}}" --strip-components=3
	printf '%s\n' "{{ref}}" > "{{piglit_dir}}/REF"
	n=$(find "{{piglit_dir}}" -name '*.vert' -o -name '*.frag' | wc -l | tr -d ' ')
	echo "Fetched $n shaders into {{piglit_dir}}/"

# Run every `expect_result: pass` piglit shader through the crusher and report.
#	just piglit				 all shaders
#	just piglit 'loop-*'	 only files matching the glob
# Per-file status (piglit/results/report.tsv: status, in bytes, out bytes, file):
#	OK			   crushed, output re-parses, no reserved `gl_*` name renamed
#	GL_RENAMED	   crushed + re-parses, but a `gl_*` builtin was renamed (semantically broken)
#	PARSE_FAIL	   crusher could not parse the input
#	ROUNDTRIP_FAIL crushed output does not re-parse through the crusher
#	CRASH		   crusher exited non-zero
#	VALIDATE_FAIL  $GLSL_VALIDATOR (e.g. glslangValidator) rejected an OK output
# Exit is non-zero if any status in $PIGLIT_FATAL occurs (default: "CRASH ROUNDTRIP_FAIL").
[doc("Run every `expect_result: pass` piglit shader through the crusher; report + gate (see comments)")]
piglit pattern="*":
	#!/usr/bin/env sh
	set -eu
	corpus="{{piglit_dir}}"
	[ -d "$corpus/glsl2" ] || just piglit-fetch
	cargo build --release --quiet
	bin="target/release/shader-crusher"
	results="$corpus/results"
	rm -rf "$results"
	mkdir -p "$results/out" "$results/log"
	report="$results/report.tsv"
	: > "$report"
	fatal="${PIGLIT_FATAL:-CRASH ROUNDTRIP_FAIL}"
	validator="${GLSL_VALIDATOR:-}"
	tmp="$results/roundtrip.glsl"
	selected=0
	skipped=0
	for f in $(find "$corpus/glsl2" "$corpus/shaders" \( -name '*.vert' -o -name '*.frag' \) -name "{{pattern}}" | sort); do
		# header line is either "// expect_result: pass" or " * expect_result: pass"
		expect=$(grep -m1 -E '^[[:space:]]*(//|\*)[[:space:]]*expect_result:' "$f" | sed -E 's/.*expect_result:[[:space:]]*//; s/[[:space:]]*$//')
		if [ "$expect" != "pass" ]; then
			skipped=$((skipped + 1))
			continue
		fi
		selected=$((selected + 1))
		name=$(basename "$f")
		out="$results/out/$name"
		log="$results/log/$name.log"
		status=OK
		# NOTE: the greps below depend on messages printed by src/shader_crusher/shadercrusher.rs:
		#	"Error parsing shader"	 parse failure (CLI currently still exits 0 and writes the input unchanged)
		#	'Replacing "gl_'		 a reserved builtin identifier was renamed
		if ! "$bin" crush --input "$f" --output "$out" > "$log" 2>&1; then
			status=CRASH
		elif grep -q 'Error parsing shader' "$log"; then
			status=PARSE_FAIL
		elif ! "$bin" crush --input "$out" --output "$tmp" > "$log.roundtrip" 2>&1 || grep -q 'Error parsing shader' "$log.roundtrip"; then
			status=ROUNDTRIP_FAIL
		elif grep -q 'Replacing "gl_' "$log"; then
			status=GL_RENAMED
		elif [ -n "$validator" ] && ! "$validator" "$out" > "$log.validate" 2>&1; then
			status=VALIDATE_FAIL
		fi
		isz=$(wc -c < "$f" | tr -d ' ')
		osz=0
		[ -f "$out" ] && osz=$(wc -c < "$out" | tr -d ' ')
		printf '%s\t%s\t%s\t%s\n' "$status" "$isz" "$osz" "$f" >> "$report"
	done
	rm -f "$tmp"
	echo "piglit glslparsertest @ $(cat "$corpus/REF"): $selected selected, $skipped skipped (expect_result != pass)"
	cut -f1 "$report" | sort | uniq -c | sort -rn | sed 's/^/  /'
	awk -F'\t' '$1=="OK"||$1=="GL_RENAMED"||$1=="VALIDATE_FAIL"{i+=$2;o+=$3;n++} END{if(n) printf "	 crushed %d files: %d -> %d bytes (%.1f%%)\n", n, i, o, 100*o/i}' "$report"
	awk -F'\t' '$1!="OK"{printf "  %-14s %s\n", $1, $4}' "$report" | sort
	echo "report: $report"
	rc=0
	for s in $fatal; do
		n=$(awk -F'\t' -v s="$s" '$1==s' "$report" | wc -l | tr -d ' ')
		[ "$n" -eq 0 ] || { echo "FATAL: $n x $s"; rc=1; }
	done
	exit $rc

# Remove the downloaded piglit corpus and results
piglit-clean:
	rm -rf {{piglit_dir}}
