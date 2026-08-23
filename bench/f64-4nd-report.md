# shader-crusher on f64-4nd/content

Source: `/Users/anti/data/work/fake/64/f64-4nd/content` — 8 shaders (3 vert, 5 frag). `#include` lines were expanded inline before measuring (the two `*.inc.frag` files only exist through `march.frag`), and the `//[` … `//]` markers became `#pragma SHADER_CRUSHER_OFF/ON`. Every crushed output passed the crusher's self-check (re-parse to the same tree with the same identifier binding).

Variants:

- **original**: the expanded source as the compiler would see it.
- **strip**: comments and whitespace removed only (`--no-rename --no-simplify`).
- **protected**: default crush, but every `uniform` keeps its name (`--blocklist`) so the app can still set them by name; `textureSampler` is additionally inside a `//[` region.
- **full**: default crush, everything renamed (the app would use `--emit-map` to find the new uniform names).

## Per file (protected variant)

| file | original raw | gz | xz | zstd | crushed raw | gz | xz | zstd | raw | gz | xz | zstd |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| default.frag | 757 | 413 | 488 | 426 | 312 | 233 | 304 | 225 | -58.8% | -43.6% | -37.7% | -47.2% |
| default.vert | 173 | 146 | 200 | 136 | 154 | 130 | 188 | 122 | -11.0% | -11.0% | -6.0% | -10.3% |
| gbuffer.frag | 584 | 333 | 396 | 337 | 276 | 198 | 264 | 190 | -52.7% | -40.5% | -33.3% | -43.6% |
| gbuffer.vert | 173 | 146 | 200 | 136 | 154 | 130 | 188 | 122 | -11.0% | -11.0% | -6.0% | -10.3% |
| gbuffer2.frag | 1351 | 577 | 648 | 587 | 490 | 284 | 356 | 284 | -63.7% | -50.8% | -45.1% | -51.6% |
| gbufferblur.frag | 1307 | 536 | 600 | 542 | 602 | 317 | 384 | 317 | -53.9% | -40.9% | -36.0% | -41.5% |
| march.frag (+common.inc.frag, materials.inc.frag) | 28123 | 8060 | 7608 | 7697 | 11462 | 4180 | 3988 | 4038 | -59.2% | -48.1% | -47.6% | -47.5% |
| march.vert | 183 | 151 | 208 | 143 | 160 | 137 | 192 | 127 | -12.6% | -9.3% | -7.7% | -11.2% |
| **sum** | 32651 | 10362 | 10348 | 10004 | 13610 | 5609 | 5864 | 5425 | -58.3% | -45.9% | -43.3% | -45.8% |

## All variants

Per-file sizes summed, and all shaders concatenated into one stream (what an intro's compressor sees):

| variant | Σ raw | Σ gz | Σ xz | Σ zstd | concat raw | concat gz | concat xz | concat zstd |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| original | 32651 | 10362 | 10348 | 10004 | 32658 | 8844 | 8344 | 8430 |
| strip | 19338 | 6891 | 7128 | 6745 | 19345 | 5875 | 5632 | 5741 |
| protected | 13610 | 5609 | 5864 | 5425 | 13617 | 4658 | 4412 | 4477 |
| full | 12798 | 5415 | 5676 | 5231 | 12805 | 4527 | 4292 | 4348 |

- strip vs original (concatenated): raw -40.8%, gzip -33.6%, xz -32.5%, zstd -31.9%
- protected vs original (concatenated): raw -58.3%, gzip -47.3%, xz -47.1%, zstd -46.9%
- full vs original (concatenated): raw -60.8%, gzip -48.8%, xz -48.6%, zstd -48.4%

## march.frag (the big one) per variant

| variant | raw | gz | xz | zstd |
|---|---:|---:|---:|---:|
| original | 28123 | 8060 | 7608 | 7697 |
| strip | 16822 | 5386 | 5188 | 5279 |
| protected | 11462 | 4180 | 3988 | 4038 |
| full | 10840 | 4076 | 3884 | 3932 |

Protected uniforms in march.frag: background_R, cam0, cam1, camAngleOffset, camMix, iAspect, iTime, scene02ConeA, scene02ConeB, sceneSelector, sphereColorB, sphereColorG, sphereColorR, sphereColorScale, sphereMetallness, sphereRoughness, textureSampler.

## Notes

- Protecting the uniforms costs bytes mostly in march.frag (16 long uniform names); everything else (functions, locals, struct fields, the `Hit`/`Material`/`Camera` types) is renamed in both variants.
- The vertex/fragment interface is matched by `layout(location)`, so `in`/`out` variables are renamed freely; `gl_PerVertex` and `gl_Position` are reserved and kept.
- Outputs are in `/private/tmp/claude-501/-Users-anti-data-work-fake-shader-crusher/83a780b5-d8c7-4132-9af9-4eaa8485ee35/scratchpad/f64` (`src/` = expanded inputs, one directory per variant).
