# Mesh repair: implementation plan

Status: approved by the owner on 2026-10-21 as milestone M6.4d (TD-092). Research basis: the
mesh repair comparison and the
defective-mesh research. Current behaviour: the
[defective-mesh specification](OPEN-MESH-SPEC.md) (TD-054) and TD-091.

## 1. Goal

**Every model a user brings can be printed with Karasu.** A model that cannot be sliced, hollowed
or supported is a usability failure, and makes Karasu uncompetitive. Commercial slicers get there
by repairing silently, with fixed tolerances, and by reporting unfillable holes as fixed. Karasu
gets there without lying:
- **Every repair exists**, including aggressive ones, up to rebuilding the surface.
- **Nothing is repaired silently.** Every change is counted, located and shown; every defect left
  is named, with what it will do to the print.
- **The user is in control:** repairs can run automatically or on request, from presets or stage
  by stage, with every tolerance adjustable. Exact repairs come first; tolerances are needed only
  when exact repairs cannot close the model, as with CHITUBOX's welding, but chosen by the user
  and reported.
- **The user is never stuck:** the interface always proposes the next step, either a stronger
  repair or an informed bypass of a small defect, until the model slices, hollows and prints.

No reference offers this combination: they repair with fixed tolerances or not at all, and report
little or nothing (comparison, section "Comparison by defect class").

## 2. Principles

1. **The source is immutable.** Repairs build a derived mesh, recorded as the object's repair
   settings; removing them returns exactly to the source (as TD-089 does for hollowing).
2. **Exact before tolerant, tolerant before rebuilding.** Each stage first does what needs no
   tolerance; tolerances apply only where set, and the last resort replaces the surface.
3. **Every change is measured:** counts, positions, and the largest geometric change (a vertex's
   displacement, a removed face's area, the deviation of a rebuilt surface).
4. **Every remaining defect is classified** by its effect: harmless, handled by slicing (closed in
   layers, slivers left empty), blocking hollowing or supports, or blocking slicing.
5. **Bypasses are explicit:** accepting a remaining defect is a recorded choice, shown in the
   summary and the export metadata, never a default that hides it.
6. **Deterministic and bounded:** identical results for any thread count; fallible allocation,
   cancellation and explicit limits, as the rest of the core (TD-053).
7. **Measured:** every stage has a time and memory budget on the 1-million-triangle models of the
   corpus (`AGENTS.md`, performance).

## 3. Repair stages

The pipeline runs these stages in order. Each is a core function over a mesh, returning a derived
mesh (or none) and its entry in the repair report. **New** marks what does not exist today.

| # | Stage | Exact mode | Tolerant / aggressive mode | Today |
| --- | --- | --- | --- | --- |
| R0 | **Diagnosis** | Every class counted and located, always | — | Partial: add needles, caps, cracks, folds, non-manifold vertices, small shells, open sheets, self-intersections (bounded count), overlapping shells |
| R1 | **Weld vertices** | Equal coordinates (import) | **New:** merge within a distance (true distance, not grid rounding), clusters bounded by the tolerance; faces that collapse are removed and reported; largest displacement reported | Exact only |
| R2 | **Degenerate faces** | **New:** needles (coincident vertices) removed and their two equal edges zipped; caps closed by splitting (TD-091) | **New:** caps and needles within a tolerance (distance of the middle vertex from the line, length of the short edge) | Caps on lines only (TD-091) |
| R3 | **Duplicates and folds** | Same-orientation duplicates collapse; opposite pairs cancel; **new:** folds anywhere (coincident reversed faces) cancel | — | Duplicates; folds only around cracks |
| R4 | **Cracks and T-junctions** | Exact collinear zipping (TD-091) | **New:** zipping within a tolerance (as CHITUBOX at 1 µm), vertex-to-line distance reported | Exact |
| R5 | **Non-manifold edges and vertices** | **New:** fins (an extra face on an edge) removed by a recorded rule: keep the face pair that makes the edge balanced and closest to flat, remove the rest; pinch vertices split per fan when a later stage needs it | **New:** remove every face on unresolvable edges and let R7 refill | None |
| R6 | **Orientation** | Consistent per component, larger area kept; outermost inside-out components flipped, cavities kept | **New:** open components oriented by ray visibility (as Lychee) | Exact modes |
| R7 | **Holes** | Validated minimum-weight caps, no new vertices, size bound | **New:** large holes with new vertices (refinement and fairing, Liepa 2003); rims that are not simple split at their repeated vertices; unbounded size; each cap still validated against contact | Exact mode |
| R8 | **Small and inner shells** | — | **New:** remove components under an area or volume bound, and closed shells entirely inside another shell when asked | None |
| R9 | **Rebuild as solid** (last resort) | — | **New:** rebuild the surface from the exact positive-winding field on a sampled grid at a user-chosen resolution, with the existing extraction and certification of the hollowing core: always closed, manifold and oriented; maximum deviation from the source reported | None |

The stages compose: R9 needs nothing before it, and R1 to R8 make R9 unnecessary for most models.

**Consumers that become tolerant (no repair needed):**
- **H1, hollowing ignores measure-zero defects.** Zero-area faces and zero-area cracks do not
  change which points are inside; admission ignores them, rays that meet them exactly are handled
  by simulation of simplicity, and the summary names them. This unblocks `Base_01.stl`.
- **H2, hollowing unions overlapping shells** by the positive winding rule, as slicing does,
  instead of refusing winding numbers above 1 (specification section 5.5).
- **S1, supports and drain holes** use the same tolerant inside test.

## 4. Settings, presets and tolerances

- **Per object**, with a plate default: models need different repairs. The settings are an
  operation of the object's timeline (TD-090), first, so the derived mesh is cached by the source
  and the settings (the pipeline cache already keys repairs this way).
- **Presets:**

  | Preset | Contents |
  | --- | --- |
  | Off | Diagnosis only |
  | Exact (proposed default) | R1–R4 exact, R6 exact, R7 exact: no vertex moves, no tolerance |
  | Standard | Exact, plus R1 at 1 µm, R2 and R4 at 1 µm, R5 exact, R7 with refinement up to a bound |
  | Aggressive | Standard, plus R5 aggressive, R7 unbounded, R8 with small bounds, R6 by visibility |
  | Rebuild | R9 at a resolution derived from the printer's pixel and layer, then Exact |
  | Custom | Any stage on or off, any tolerance |

- **Tolerances** are in millimetres or mm², each with an "auto" value derived from the model or the
  printer (for example half the LCD pixel for welding), shown with its resolved value.
- **Auto-repair on import:** an application setting, `Off`, `Detect and propose` or a preset
  (proposed default: `Exact`, since it moves no vertex), always followed by the report.
- **Command line:** `--repair` keeps its current syntax and gains presets and stage options, for
  example `--repair standard`, `--repair weld=0.001,cracks=0.001,fins,holes=unbounded`,
  `--repair rebuild=0.05`. Its default stays `auto` (duplicates and orientation), so earlier
  outputs do not change.
- **Plate document:** per-object `repair` in a new version, the current plate-level fields still
  read.

## 5. Report and states

- **One report per object:** for each stage, what changed (counts, positions, largest change)
  and what it refused; before and after diagnoses; the time taken.
- **Printability state**, recomputed after every change:

  | State | Meaning |
  | --- | --- |
  | Ready | No defect left, or only harmless ones |
  | Ready with notes | Defects that slicing handles (gaps closed in layers, slivers left empty), named |
  | Needs repair for hollowing / supports | Slicing works; those operations need a stronger repair or a bypass |
  | Needs repair | Slicing would fail where named |

- **Locations:** every defect and every repair carries positions, so the viewport can mark them.
- **Export metadata and summary:** the repairs applied and the defects accepted, so a print file
  says how its model was prepared.

## 6. The guided flow (interface)

1. **Import:** the diagnosis runs (and the auto-repair, when set). A notice says what was found
   and done: "3 problems found, 2 repaired (Exact). 1 crack of 0.008 mm left: closed in layers;
   blocks hollowing."
2. **Repair panel** for the selected object:
   - the state and the list of defects, each with its effect and a "Show" that frames it in the
     viewport;
   - presets and the stage list with tolerances (custom mode);
   - **Repair**, which runs the chosen settings in the background and shows the before/after
     report and the largest change;
   - **the next step** when something remains: the smallest stronger preset that would resolve
     it (computed by trying it on a background copy when it is cheap), or the bypasses that
     apply ("Accept: hollow despite 1 zero-area crack");
   - **Undo** like any edit, and **Remove repairs** to return to the source.
3. **Viewport markers:** defects as GPU overlay points and lines (boundary rims, cracks, fins,
   inverted regions), repaired places in another colour; the comparison of a rebuilt surface
   with the source as a colour map of deviation.
4. **Slicing and export:** never blocked by a defect the user accepted; blocked only by one that
   would make layers fail, with the repair that resolves it proposed in the same message.

## 7. Phases

Each phase has its tests, gates and records; core phases first.

| Phase | Content | Verifiable result |
| --- | --- | --- |
| **r1 Foundations** | The repair pipeline and settings model (stages, presets, tolerances, per object, timeline operation), the unified report with locations and largest changes, the extended diagnosis (R0), the printability state, CLI presets with the current default unchanged | The comparison suite byte-identical; every fixture's diagnosis complete and located |
| **r2 Welding, degenerate faces, cracks** | R1 tolerant, R2 needles and tolerant caps, R3 folds everywhere, R4 tolerant | `Base_01.stl` repaired with Standard, or hollowed with H1; welding never merges points farther than the tolerance (property test) |
| **r3 Tolerant hollowing** | H1 and H2, S1 | `Base_01.stl` and overlapping shells hollowed; the certificate unchanged for clean models |
| **r4 Non-manifold** | R5 exact and aggressive | FX-10B slices and hollows after Standard |
| **r5 Holes** | R7 refinement and fairing, non-simple rims, unbounded mode, still validated | A model with a large irregular hole is closed without intersections |
| **r6 Shells and orientation** | R8, R6 by visibility | Small shells and inner shells removed when asked, reported |
| **r7 Rebuild** | R9 on the sampled winding field, with a deviation map | Any corpus model becomes closed and manifold; the deviation within the requested resolution |
| **r8 Interface** | Repair panel, guided flow, auto-repair setting, notices, markers, accepted defects, export metadata | Every corpus model reaches slicing from the interface |
| **r9 Validation** | Corpus, performance budgets, native Windows, the owner's models, black-box comparison with CHITUBOX and Lychee where lawful | Recorded runs and the corpus table |

### Progress

- **r1 (2026-10-21), core, project and command line:** the extended diagnosis with positions
  (R0) and self-intersections on request; located repair changes and the largest cap; the `Off`
  and `Exact` presets; the printability state; an object's own repair as the first operation of
  its timeline; `--repair off|exact` and `inspect --diagnosis`. The command line's default and
  every existing output are unchanged. Tolerances enter with the stages that use them (r2), the
  interface's use of the state, presets and positions with r8.
- **r2 (2026-10-21), welding, degenerate faces, cracks:** R1 (open-edge vertices only, clusters
  no wider than the tolerance), R2 (needles, short-edge collapse, flat-face flips), R3 by
  ordering (duplicate collapse runs after them), R4 tolerant, and exact cracks closing into
  balanced edges; the `Standard` preset. `Base_01.stl` closes with `Exact` and is hollowed and
  sliced with `Standard` and a reversed tolerance for its self-intersection slivers.
- **r3 (2026-10-21), tolerant hollowing:** H1 (zero-area faces and cracks left out at admission),
  H2 (overlapping bodies hollowed as their union, each void emptied once per body), S1 (drain
  holes certified on such shells; supports already independent of topology). `Base_01.stl` is
  hollowed without any repair.
- **r4 (2026-10-21), non-manifold edges:** R5 fins and aggressive cut, the `Aggressive` preset;
  FX-10B slices and hollows after `Standard`. The rule differs from section 3's "closest to flat
  pair": a flap folded onto a face is exactly as flat as the face, so the rule removes the open
  part that causes the imbalance instead (TD-092 r4). Splitting pinch vertices moves to r5, where
  hole filling needs it.
- **r5 (2026-10-21), holes:** R7 refinement (Liepa) with thin-plate fairing (Kobbelt), non-simple
  rims capped as their loops (which resolves the pinch vertices that hole filling meets), the
  unbounded mode, the unrefined fallback; every cap still validated. A large irregular hole
  closes without intersections (sphere and Yorick tests).
- **r6 (2026-10-21), shells and orientation:** R8 small shells (area, volume) and inner shells on
  request, R6 orientation of open parts by visibility; `Aggressive` gains visibility and
  `small-shells=1`. Cavities and bodies in cavities are never removed.
- **r7 (2026-10-21), rebuild:** R9 on the hollowing core's field and extraction, the `Rebuild`
  preset, both deviations reported. Every tracked fixture becomes closed and manifold; the
  certified deviation is about two cells at sharp oblique edges, not one (TD-092 r7). The colour
  map of the deviation belongs to r8 (the report already locates the largest). The rebuilt
  surface is dense; its certified simplification is left to r9's budgets.
- **r8 (2026-10-21), interface:** the Repair section of the object panel (state, defects with
  "Show", presets and custom stages, report, next step, remove), auto-repair on import (Exact),
  GPU markers for defects and repairs, and failed slices offering the repair. Every tracked
  fixture reaches slicing from the interface. Not done: the setting is per session (the
  application has no saved settings yet); the deviation colour map of a rebuilt surface; export
  metadata, which the print formats have no field for; "the next step" is the next preset on
  the ladder, not a trial on a background copy.

- **r9 (2026-10-21), validation:** the crafted corpus (26 cases through Exact, Standard and
  Aggressive, with the plan's properties), the budgets (every stage within budget on the Yorick,
  `Base_01.stl` and FX-12; rebuild 8.7 s on the Yorick; memory linear), a sample of 40 of the
  owner's models, and native Windows. The validation found and fixed four faults (TD-092 r9): an
  inside-out model's cavity turned into a double body, planar rims at a crease faired into domes,
  stacked duplicates collapsed into unbalanced edges (the 17-shell 3DBenchy), and contact
  validation indexing every face. Not done: the black-box comparison with CHITUBOX and Lychee
  (the corpus can be exported as STL for it), and the hollowing preview's memory with drain holes
  on large cavities, which is outside repair. Then, from the owner's sample and the first runtime
  comparison with CHITUBOX and Lychee, the `non-finite` repair (part of `Exact`): facets without
  a position are left out and named instead of refusing the file.

## 8. Corpus and validation

- **Crafted fixtures** (generated, redistributable): needles and caps at T-junctions, folds,
  near-collinear cracks at given distances, vertices across grid-cell borders, fins with 3 and 4
  faces, pinch vertices, non-simple and large holes, inside-out cavities, overlapping and nested
  shells, small shells, open sheets.
- **Existing fixtures:** FX-06 to FX-12.
- **The owner's models** (local, not committed): `Base_01.stl`, the Yorick miniature, and others
  the owner adds.
- **Properties:** every repair keeps the winding number wherever it was defined, except where it
  reports a change; welding never moves a vertex farther than the tolerance; results are identical
  for any thread count.
- **Performance budgets** (Linux release, measured before and after): each exact stage under
  0.5 s and each tolerant stage under 2 s on 1 million triangles; rebuild under 10 s at 0.05 mm on
  the Yorick; memory linear in the mesh.
- **Comparison suite:** byte-identical for the existing cases at every phase, since the CLI
  default does not change.

## 9. Risks

- **Aggressive repairs change geometry:** bounded by reported tolerances and deviations, and never
  applied without the user choosing them (presets or auto-repair setting).
- **Tolerant welding can fuse thin features:** merges are bounded by the tolerance, and the report
  lists collapsed faces and their area.
- **Rebuild cost and fidelity:** sampled at a resolution the user sees, with the deviation map;
  thin features below the resolution are reported as lost.
- **Scope:** R9 and R7 refinement are the largest items; they can move after r8 if needed, since
  H1 and H2 already unblock hollowing for most models.

## 10. Decisions (owner, 2026-10-21)

- Placement in the roadmap: **M6.4d Repair**, before manual supports, because supports and
  hollowing need closed models (supports become M6.4e and the mandatory scenarios M6.4f).
- The defaults: auto-repair on import `Exact`, CLI unchanged.
- The preset contents and tolerances above (1 µm for Standard welding and cracks follows CHITUBOX).
