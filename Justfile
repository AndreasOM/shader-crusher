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
# Per-file line in piglit/results/report.tsv:
#	status  in-bytes  out-bytes  gzip-9  xz-9e  zstd-19  file
# Status (from the crusher's exit code, then extra checks):
#	OK			   crushed, self-check passed, output crushes again, no `gl_*` name lost
#	PARSE_FAIL	   exit 1: crusher could not parse the input (or parsed only a prefix of it)
#	SELFCHECK_FAIL exit 2: crushed output does not re-parse to the intended AST
#	UNSUPPORTED	   exit 3: valid GLSL the crusher does not handle
#	CRASH		   any other exit code
#	ROUNDTRIP_FAIL crushing the crushed output fails
#	ROUNDTRIP_GROW crushing the crushed output grows it (renaming not idempotent)
#	GL_RENAMED	   the set of `gl_*` names differs between input (comments stripped) and output
#	VALIDATE_FAIL  $GLSL_VALIDATOR (e.g. glslangValidator) rejected an OK output
# Compressed sizes are what the tool optimises for; the "concatenated" line compresses all OK outputs as one
# stream (closer to an intro's shader blob). Set PIGLIT_BASELINE=<old report.tsv> to diff against a previous run.
# Exit is non-zero if any status in $PIGLIT_FATAL occurs (default: "CRASH ROUNDTRIP_FAIL ROUNDTRIP_GROW GL_RENAMED SELFCHECK_FAIL").
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
	fatal="${PIGLIT_FATAL:-CRASH ROUNDTRIP_FAIL ROUNDTRIP_GROW GL_RENAMED SELFCHECK_FAIL}"
	validator="${GLSL_VALIDATOR:-}"
	tmp="$results/roundtrip.glsl"
	allok="$results/all_ok.glsl"
	: > "$allok"
	selected=0
	skipped=0
	gl_tokens() { perl -0pe 's{/\*.*?\*/}{}gs; s{//[^\n]*}{}g' "$1" | grep -oE '\bgl_[A-Za-z0-9_]+' | sort -u | tr '\n' ' '; }
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
		set +e
		"$bin" crush --input "$f" --output "$out" > "$log" 2>&1
		rc=$?
		set -e
		case $rc in
			0) ;;
			1) status=PARSE_FAIL ;;
			2) status=SELFCHECK_FAIL ;;
			3) status=UNSUPPORTED ;;
			*) status=CRASH ;;
		esac
		if [ "$status" = OK ]; then
			set +e
			"$bin" crush --input "$out" --output "$tmp" > "$log.roundtrip" 2>&1
			rc=$?
			set -e
			if [ $rc -ne 0 ]; then
				status=ROUNDTRIP_FAIL
			elif [ "$(wc -c < "$tmp" | tr -d ' ')" -gt "$(wc -c < "$out" | tr -d ' ')" ]; then
				status=ROUNDTRIP_GROW
			elif [ "$(gl_tokens "$f")" != "$(gl_tokens "$out")" ]; then
				status=GL_RENAMED
			elif [ -n "$validator" ] && ! "$validator" "$out" > "$log.validate" 2>&1; then
				status=VALIDATE_FAIL
			fi
		fi
		isz=$(wc -c < "$f" | tr -d ' ')
		osz=0; gz=0; xz=0; zst=0
		if [ -f "$out" ]; then
			osz=$(wc -c < "$out" | tr -d ' ')
			gz=$(gzip -9c "$out" | wc -c | tr -d ' ')
			xz=$(xz -9e -c "$out" | wc -c | tr -d ' ')
			zst=$(zstd -19 -qc "$out" | wc -c | tr -d ' ')
		fi
		case "$status" in OK|ROUNDTRIP_GROW|GL_RENAMED|VALIDATE_FAIL) cat "$out" >> "$allok" ;; esac
		printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$status" "$isz" "$osz" "$gz" "$xz" "$zst" "$f" >> "$report"
	done
	rm -f "$tmp"
	echo "piglit glslparsertest @ $(cat "$corpus/REF"): $selected selected, $skipped skipped (expect_result != pass)"
	cut -f1 "$report" | sort | uniq -c | sort -rn | sed 's/^/  /'
	# size totals cover every file that was actually crushed (OK plus the informative statuses)
	awk -F'\t' '$1=="OK"||$1=="ROUNDTRIP_GROW"||$1=="GL_RENAMED"||$1=="VALIDATE_FAIL"{i+=$2;o+=$3;g+=$4;x+=$5;z+=$6;n++} END{if(n) printf "  crushed %d files: %d -> %d bytes (%.1f%%) | gzip %d | xz %d | zstd %d\n", n, i, o, 100*o/i, g, x, z}' "$report"
	printf '  concatenated crushed outputs: %s bytes | gzip %s | xz %s | zstd %s\n' "$(wc -c < "$allok" | tr -d ' ')" "$(gzip -9c "$allok" | wc -c | tr -d ' ')" "$(xz -9e -c "$allok" | wc -c | tr -d ' ')" "$(zstd -19 -qc "$allok" | wc -c | tr -d ' ')"
	awk -F'\t' '$1!="OK"{printf "  %-14s %s\n", $1, $7}' "$report" | sort
	if [ -n "${PIGLIT_BASELINE:-}" ]; then
		echo "vs baseline $PIGLIT_BASELINE:"
		# baseline may be an old 4-column report (status in out file): key on the last column, compressed deltas then read as 0
		awk -F'\t' 'NR==FNR{b[$NF]=$1; next} !($NF in b){print "  NEW            " $1 "  " $NF; next} b[$NF]!=$1{print "  " b[$NF] " -> " $1 "  " $NF}' "$PIGLIT_BASELINE" "$report"
		awk -F'\t' 'function crushed(s){return s=="OK"||s=="ROUNDTRIP_GROW"||s=="GL_RENAMED"||s=="VALIDATE_FAIL"} NR==FNR{b[$NF]=$1; o[$NF]=$3; g[$NF]=(NF>=7?$4:0); x[$NF]=(NF>=7?$5:0); z[$NF]=(NF>=7?$6:0); next} ($NF in b) && crushed(b[$NF]) && crushed($1) { do_=$3-o[$NF]; dg=$4-g[$NF]; dx=$5-x[$NF]; dz=$6-z[$NF]; if (do_||dg||dx||dz) printf "  %+6d raw %+5d gz %+5d xz %+5d zst  %s\n", do_, dg, dx, dz, $NF }' "$PIGLIT_BASELINE" "$report" | sort -k1,1n
		awk -F'\t' 'function crushed(s){return s=="OK"||s=="ROUNDTRIP_GROW"||s=="GL_RENAMED"||s=="VALIDATE_FAIL"} NR==FNR{b[$NF]=$1; o[$NF]=$3; g[$NF]=(NF>=7?$4:0); x[$NF]=(NF>=7?$5:0); z[$NF]=(NF>=7?$6:0); next} ($NF in b) && crushed(b[$NF]) && crushed($1) { to+=$3-o[$NF]; tg+=$4-g[$NF]; tx+=$5-x[$NF]; tz+=$6-z[$NF]; n++ } END { printf "  total over %d files crushed in both: %+d raw %+d gz %+d xz %+d zst\n", n, to, tg, tx, tz }' "$PIGLIT_BASELINE" "$report"
	fi
	echo "report: $report"
	rc=0
	for s in $fatal; do
		n=$(awk -F'\t' -v s="$s" '$1==s' "$report" | wc -l | tr -d ' ')
		[ "$n" -eq 0 ] || { echo "FATAL: $n x $s"; rc=1; }
	done
	exit $rc

# Copy the last piglit report to bench/<name>.tsv and append its totals to bench/README.md
piglit-record name:
	#!/usr/bin/env sh
	set -eu
	report="{{piglit_dir}}/results/report.tsv"
	[ -f "$report" ] || { echo "no report; run 'just piglit' first"; exit 1; }
	cp "$report" "bench/{{name}}.tsv"
	awk -F'\t' -v name="{{name}}" -v rev="$(git rev-parse --short HEAD)" '$1=="OK"{ok++} $1=="OK"||$1=="ROUNDTRIP_GROW"||$1=="GL_RENAMED"||$1=="VALIDATE_FAIL"{i+=$2;o+=$3;g+=$4;x+=$5;z+=$6;n++} END{printf "| %s (%s) | %d / %d | %d | %d | %d | %d | %d |  |\n", name, rev, ok, n, i, o, g, x, z}' "$report" >> bench/README.md
	tail -1 bench/README.md

# Remove the downloaded piglit corpus and results
piglit-clean:
	rm -rf {{piglit_dir}}
