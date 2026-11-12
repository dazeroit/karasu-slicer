# Reference research

## Objective

Other resin slicers, commercial or open source, may be studied for what they do: observable
workflows, interoperability requirements, parameter meanings, coordinate conventions, edge cases
and output characteristics. Research produces independent behavioural requirements; it never
provides implementation code. This document details the research step of the clean-room method in
[AGENTS.md](../AGENTS.md#clean-room-development), which governs it.

## Required separation

Every research question follows this chain:

```text
behavioural observation
    -> evidence-backed behavioural requirement
    -> independent algorithm and data-model design
    -> native Rust implementation
    -> validation with independent and comparative oracles
```

## Restrictions

- Observe other programs only as an ordinary user does: their interface, settings, messages and
  the files they write from inputs prepared for the purpose.
- Do not read another program's source code, and do not decompile, disassemble, debug or inspect
  its binaries, open source included.
- Do not bypass licensing, activation, encryption, signatures or access controls.
- Do not modify installed programs or run untrusted mutations against them.
- Do not inspect credentials, user documents, telemetry or unrelated private information.
- Do not copy or translate another program's code, pseudocode, resources, profiles or assets.
- Keep raw evidence in a separate research location, outside the repository.
- Prefer public specifications, manufacturers' documentation, legitimate sample files, controlled
  black-box experiments and independently generated geometry.
- Confirm that test artifacts can be redistributed before committing them.

## Evidence model

Give every observation a stable identifier. A research record contains:

| Field | Meaning |
| --- | --- |
| Evidence ID | Stable reference used by requirements and tests |
| Date and researcher | When and by whom the observation was made |
| Product identity | Product name, displayed version and edition |
| Input identity | The input files and their hashes |
| Environment | OS, architecture, locale, display scale and relevant hardware |
| Preconditions | Settings reset or state, selected printer and resin |
| Procedure | Reproducible user-visible actions |
| Raw observation | What was directly measured or displayed, without interpretation |
| Interpretation | A possible explanation derived from observations |
| Alternatives | Other explanations not ruled out |
| Confidence | High, medium or low, with rationale |
| Related records | Requirements, test scenarios, public specifications and prior observations |

Screenshots, hashes, logs and exported files are referenced by the record and stay in the separate
evidence store. Sanitize user paths and unrelated data.

## Requirement derivation

A behavioural requirement must:

1. cite one or more observations or a public specification;
2. describe externally observable input/output behaviour without implementation assumptions;
3. state units, coordinate space, preconditions and acceptable tolerance;
4. distinguish mandatory interoperability from optional parity;
5. include a reproducible validation scenario; and
6. mark uncertainty and conflicting evidence.

Example form: "For scenario `SCN-...`, input X and profile P produce N layers with the first
exposure plane at Z within T millimetres," rather than "do what product Y does internally."

## Method

1. **Controlled test models.** Create independently owned, analytically specified models: cube,
   hollow box, inclined prism, multi-component solid and selected invalid meshes. Record the
   generator, dimensions, tessellation, units, topology and hashes. Expand the corpus only to
   answer a named question.
2. **Scenario matrix.** Hold all but one variable constant. Cover import units and orientation,
   build-plate placement, layer-plane placement, boundary cases, raster dimensions and
   orientation, antialiasing, exposure metadata and output container structure. Reset and record
   the program's settings for every run.
3. **Capture behaviour.** For each program and scenario, record values shown in the interface,
   exported files, file hashes, metadata decoded by independent readers, layer dimensions, pixel
   coordinates and repeat-run stability. Keep raw outputs separate when redistribution is not
   authorized.
4. **Reconcile and specify.** Compare observations with analytic expectations and public format
   documents. Treat disagreement between programs as evidence of a choice, not proof that one is
   correct. Produce bounded behavioural requirements and tests that do not depend on proprietary
   artifacts.

## Typical research questions

- How does each program interpret STL unitless coordinates and 3MF units?
- What are the world, build-plate, viewport, layer-image and machine-file origins and axis
  directions?
- Where is the first layer plane relative to the build plate and the configured layer height?
- How are triangles exactly on, tangent to or crossing a layer plane treated, as far as the
  output shows?
- How are outer regions, holes, disconnected components and invalid open contours represented?
- How do pixel pitch, image dimensions, flips, rotations, antialiasing and grey values map to
  physical positions?
- Which exported metadata fields are deterministic, derived, user-supplied or machine-specific?
