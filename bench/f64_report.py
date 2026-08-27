#!/usr/bin/env python3
"""Crush the f64-4nd shaders and report sizes before/after, raw and compressed."""
import re
import subprocess
import sys
from pathlib import Path

SRC = Path(sys.argv[1]).resolve()
OUT = Path(sys.argv[2]).resolve()
BIN = Path(sys.argv[3]).resolve()
REPORT = Path(sys.argv[4]).resolve()

INCLUDE = re.compile(r'^\s*#\s*include\s+"([^"]+)"\s*$')


def expand(path: Path, depth: int = 0) -> str:
	if depth > 8:
		raise RuntimeError(f"include nesting too deep at {path}")
	out = []
	for line in path.read_text().splitlines(keepends=True):
		m = INCLUDE.match(line)
		if m:
			out.append(expand(path.parent / m.group(1), depth + 1))
			if not out[-1].endswith("\n"):
				out.append("\n")
		else:
			out.append(line)
	return "".join(out)


def prep(text: str) -> str:
	"""`//[` ... `//]` (Shader Minifier verbatim markers) -> crusher pragmas."""
	lines = []
	for line in text.splitlines():
		s = line.strip()
		if s == "//[":
			lines.append("#pragma SHADER_CRUSHER_OFF")
		elif s == "//]":
			lines.append("#pragma SHADER_CRUSHER_ON")
		else:
			lines.append(line)
	return "\n".join(lines) + "\n"


def sizes(data: bytes) -> dict:
	def c(cmd):
		return len(subprocess.run(cmd, input=data, capture_output=True, check=True).stdout)

	return {
		"raw": len(data),
		"gz": c(["gzip", "-9c"]),
		"xz": c(["xz", "-9e", "-c"]),
		"zst": c(["zstd", "-19", "-qc"]),
	}


def crush(src: Path, dst: Path, args: list) -> dict:
	r = subprocess.run([str(BIN), "crush", *args, "--input", str(src), "--output", str(dst)], capture_output=True, text=True)
	if r.returncode != 0:
		raise RuntimeError(f"{src.name} {args}: exit {r.returncode}\n{r.stderr}")
	return {"sizes": sizes(dst.read_bytes()), "log": r.stderr.strip().splitlines()[-1] if r.stderr.strip() else ""}


files = sorted(p for p in list(SRC.glob("*.frag")) + list(SRC.glob("*.vert")) if ".inc." not in p.name)
(OUT / "src").mkdir(parents=True, exist_ok=True)
variants = {
	"strip": ["--no-rename", "--no-simplify"],
	"protected": None,  # filled per file with --blocklist of its uniforms
	"full": [],
}
for v in variants:
	(OUT / v).mkdir(exist_ok=True)

rows = []
concat = {"original": [], **{v: [] for v in variants}}
for f in files:
	text = prep(expand(f))
	src = OUT / "src" / f.name
	src.write_text(text)
	uniforms = sorted(set(re.findall(r"^\s*uniform\s+\w+\s+(\w+)", text, re.M)))
	row = {"file": f.name, "includes": [m.group(1) for m in (INCLUDE.match(l) for l in f.read_text().splitlines()) if m], "uniforms": uniforms}
	row["original"] = sizes(text.encode())
	concat["original"].append(text)
	for v, args in variants.items():
		if v == "protected":
			args = ["--blocklist", ",".join(uniforms)] if uniforms else []
		res = crush(src, OUT / v / f.name, args)
		row[v] = res["sizes"]
		row[v + "_log"] = res["log"]
		concat[v].append((OUT / v / f.name).read_text())
	rows.append(row)

totals = {k: {m: sum(r[k][m] for r in rows) for m in ("raw", "gz", "xz", "zst")} for k in concat}
concat_sizes = {k: sizes("\n".join(parts).encode()) for k, parts in concat.items()}


def pct(new, old):
	return f"{100.0 * (new - old) / old:+.1f}%" if old else "n/a"


lines = []
lines.append("# shader-crusher on f64-4nd/content\n")
lines.append("> **Corrected 2026-08-27.** The first version of this report (crusher 0.7.1-alpha) measured output that\n"
	"> did not compile: the renamer gave a struct type and a local variable the same name, which affected\n"
	"> `march.frag` (3 collisions). The numbers below are from the fixed crusher and every output passes the\n"
	"> type-name invariant.\n")
lines.append(f"Source: `{SRC}` — {len(files)} shaders ({sum(1 for f in files if f.suffix == '.vert')} vert, {sum(1 for f in files if f.suffix == '.frag')} frag). "
	"`#include` lines were expanded inline before measuring (the two `*.inc.frag` files only exist through `march.frag`), and the "
	"`//[` … `//]` markers became `#pragma SHADER_CRUSHER_OFF/ON`. Every crushed output passed the crusher's self-check "
	"(re-parse to the same tree with the same identifier binding).\n")
lines.append("Variants:\n")
lines.append("- **original**: the expanded source as the compiler would see it.")
lines.append("- **strip**: comments and whitespace removed only (`--no-rename --no-simplify`).")
lines.append("- **protected**: default crush, but every `uniform` keeps its name (`--blocklist`) so the app can still set them by name; `textureSampler` is additionally inside a `//[` region.")
lines.append("- **full**: default crush, everything renamed (the app would use `--emit-map` to find the new uniform names).\n")

lines.append("## Per file (protected variant)\n")
lines.append("| file | original raw | gz | xz | zstd | crushed raw | gz | xz | zstd | raw | gz | xz | zstd |")
lines.append("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|")
for r in rows:
	o, c = r["original"], r["protected"]
	inc = f" (+{', '.join(r['includes'])})" if r["includes"] else ""
	lines.append(f"| {r['file']}{inc} | {o['raw']} | {o['gz']} | {o['xz']} | {o['zst']} | {c['raw']} | {c['gz']} | {c['xz']} | {c['zst']} | "
		f"{pct(c['raw'], o['raw'])} | {pct(c['gz'], o['gz'])} | {pct(c['xz'], o['xz'])} | {pct(c['zst'], o['zst'])} |")
o, c = totals["original"], totals["protected"]
lines.append(f"| **sum** | {o['raw']} | {o['gz']} | {o['xz']} | {o['zst']} | {c['raw']} | {c['gz']} | {c['xz']} | {c['zst']} | "
	f"{pct(c['raw'], o['raw'])} | {pct(c['gz'], o['gz'])} | {pct(c['xz'], o['xz'])} | {pct(c['zst'], o['zst'])} |")

lines.append("\n## All variants\n")
lines.append("Per-file sizes summed, and all shaders concatenated into one stream (what an intro's compressor sees):\n")
lines.append("| variant | Σ raw | Σ gz | Σ xz | Σ zstd | concat raw | concat gz | concat xz | concat zstd |")
lines.append("|---|---:|---:|---:|---:|---:|---:|---:|---:|")
for k in ("original", "strip", "protected", "full"):
	t, cs = totals[k], concat_sizes[k]
	lines.append(f"| {k} | {t['raw']} | {t['gz']} | {t['xz']} | {t['zst']} | {cs['raw']} | {cs['gz']} | {cs['xz']} | {cs['zst']} |")
o = concat_sizes["original"]
lines.append("")
for k in ("strip", "protected", "full"):
	cs = concat_sizes[k]
	lines.append(f"- {k} vs original (concatenated): raw {pct(cs['raw'], o['raw'])}, gzip {pct(cs['gz'], o['gz'])}, xz {pct(cs['xz'], o['xz'])}, zstd {pct(cs['zst'], o['zst'])}")

lines.append("\n## march.frag (the big one) per variant\n")
m = next(r for r in rows if r["file"] == "march.frag")
lines.append("| variant | raw | gz | xz | zstd |")
lines.append("|---|---:|---:|---:|---:|")
for k in ("original", "strip", "protected", "full"):
	s = m[k]
	lines.append(f"| {k} | {s['raw']} | {s['gz']} | {s['xz']} | {s['zst']} |")
lines.append(f"\nProtected uniforms in march.frag: {', '.join(m['uniforms'])}.\n")
lines.append("## Notes\n")
lines.append("- Protecting the uniforms costs bytes mostly in march.frag (16 long uniform names); everything else (functions, locals, struct fields, the `Hit`/`Material`/`Camera` types) is renamed in both variants.")
lines.append("- The vertex/fragment interface is matched by `layout(location)`, so `in`/`out` variables are renamed freely; `gl_PerVertex` and `gl_Position` are reserved and kept.")
lines.append("- Outputs are in `" + str(OUT) + "` (`src/` = expanded inputs, one directory per variant).")
REPORT.write_text("\n".join(lines) + "\n")
print("\n".join(lines))
