# Code audit, 2026-10-18

## Scope and method

This is a read-only code audit of the workspace at commit `4d2cd24`, requested by the owner after the
sampled drainage work (TD-047) and the layer-plane fix (TD-048).

It ran in two passes:
1. A broad pass over the slicing engine was stopped early. It still found the silent step-face layer
   defect, which was verified independently and fixed in TD-048.
2. A reduced pass used four parallel read-only reviewers, one per area:
   - STL import and mesh;
   - LCD raster and print-file export;
   - the older preparation code (convex hollowing, convex drainage, supports, plan/session);
   - the CLI, with cross-cutting checks.

The sampled backend (`preparation::sampled`) was excluded because it had just been through five
adversarial review rounds. Every major finding below was re-read in the code before it was recorded.
Nothing was built or run during the audit.

Coverage was partial. The bodies of convex drainage and of the hull, clipping and certificate modules
were only skimmed, and `as` casts got a light sweep only.

## Findings

Status reflects the follow-up fixes recorded in TD-049.

### Major

| # | Area | Finding | Status |
| --- | --- | --- | --- |
| A1 | Raster / CLI | `RasterLimits::default().max_pixels` was 64 MiPx. The Anycubic P1 panel (13312 × 5120, 68.2 MiPx) is the main PP1 target, so the CLI failed on its first layer. `ExportLimits` had already been raised to 128 MiPx for it; only the validation example overrode the raster limit. | Fixed (TD-049) |
| A2 | Export headers | With `swap_xy`, pixel columns cover world Y, but the machine writers received `build_size_mm` unswapped. CTB/Photon bed sizes, GOO display sizes and PP1 pixel pitch/size fields therefore paired each pixel count with the other axis's length, declaring non-square pixels. No test used `swap_xy`. | Fixed (TD-049) |
| A3 | CLI | The "rest on the build plate" hint suggested `--translate 0 0 {-world_min_z}`. That ignores the Z the user already passed, so following it failed again whenever that Z was not zero. | Fixed (TD-049) |
| A4 | Export | Machine files were published only through a hard link (for no-clobber publication). FAT32/exFAT, the usual USB/SD media for MSLA printers, do not support hard links, so a complete slice failed at the last step. | Fixed (TD-049) |

### Minor

| # | Area | Finding | Status |
| --- | --- | --- | --- |
| B1 | CLI | After a Ctrl-C, any later error was reported as "interrupted, no output was published", even a failure to print the summary after the file was published. | Fixed (TD-049) |
| B2 | Export / docs | The README called the leftover staging entry "hidden", but its name `<file>.karasu-partial-<pid>-<n>` had no leading dot. | Fixed (TD-049) |
| B3 | PP1 | The default 8 MiB metadata budget caps PP1 jobs at about 10,600 layers on the P1 (explicit `Limit` error, no corrupt file). | Fixed (TD-049) |
| B4 | STL import | An ASCII STL containing a non-ASCII byte (UTF-8 BOM, accented solid name) or using bare CR line endings was rejected with a misleading binary or line-length error. | Open: rejection is intended; only the message is misleading |
| B5 | Supports | `convex_component` raises the vertex and triangle caps but not the plane cap. A loft between two independently rotated 256-gon stations (514 planes) fails with a resource error. The 128-vertex default is unaffected. | Fixed (TD-049); such a loft also needs a component payload above the 4 MiB default |

### Checked without findings

- **STL import:** ASCII/binary detection, size and count checks, the bounded reader, NaN/inf
  rejection, exact vertex identity, and checked index conversion.
- **Bounds and transforms:** overflow and non-finite results are reported.
- **Codecs:** GOO chunk tags, checksum and layout; CTB V2 offsets and RLE classes; Photon and PW0
  runs; ZIP headers and table/volume offsets.
- **Raster:** half-open spans and per-row accumulation without overflow.
- **Supports:** platform contact, tip/neck facet rules, and cavity and cutter separation.
- **Plan and session:** accept/undo invariants, checked arithmetic and cancellation.
- **CLI:** argument parsing (duplicates, non-finite values, leading dashes), create-only output and
  profile JSON bounds.
- **Cross-cutting:** HashMaps used for lookups only, pinned dependencies, and unwraps confined to
  tests or invariants.

## Known limitations confirmed (documented elsewhere)

- Bodies that touch along a vertical face make layers unresolved; nothing is merged or repaired.
- Supports are not exposed in the CLI and do not accept sampled (non-convex) hollowed models.
- The CLI has no rotation, scaling or automatic placement, and its masks are binary.
- Sampled drainage: axis-parallel holes only, a possibly quadratic fallback admission, and about
  three times the peak memory on very fine cavities (TD-047).
- Format compatibility on real printers and native Windows validation of M5.5-F/G remain M7 work.
