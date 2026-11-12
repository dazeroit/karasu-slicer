# Project Instructions for Agents and Contributors

These rules apply to the entire repository and must be followed in every development session.

## Mission and scope

Build an independent, high-performance, cross-platform MSLA/LCD resin slicer. Implement the core,
geometry, and slicing engine in native Rust. Follow the active milestone in `ROADMAP.md`; do not pull
advanced features into an earlier milestone without an explicit architectural need and project-owner
approval.

## Product and crate naming

The product is **karasu-slicer**. Application crates are `karasu-cli` and `karasu-core`;
Rust imports use `karasu_core`. Every future crate must use the `karasu-<module>` package prefix.
Do not restore the previous product name in current application identity or new modules.
Historical evidence paths and the version-1 reference corpus retain their original identifiers and
bytes. The `resin-cli` executable is a compatibility alias, not a new or retained legacy crate.
The existing `reference-fixtures` tool/package remains unchanged to preserve its corpus contract.

## Mandatory language policy

Every repository artifact must be written in professional English, including source code,
identifiers, comments, documentation, tests, diagnostics, CLI text, commit messages, pull request
descriptions, research records, and technical specifications. Do not add Italian or other languages
to project artifacts unless future product localization is explicitly introduced in a scoped task.

Product localization of the desktop interface was introduced in M6.3b (TD-080). The only exception
to the English-only rule is translation catalogs under `assets/locales/<locale>/`, which are written
in their own language. Everything else stays in English, including the reference catalog
`assets/locales/en-US`, message identifiers, catalog comments, code and tests. The command line and
the headless crates are not localized.

## Implementation rules

1. Rust is the definitive implementation language for the application core and geometry/slicing
   engine.
2. Use stable Rust unless a documented, approved requirement needs another channel.
3. Do not add C or C++ geometry kernels, FFI wrappers around foreign geometry engines, or Cargo
   build scripts that compile bundled foreign geometry implementations.
4. Prefer safe Rust. The workspace currently forbids `unsafe`. Any future exception requires a
   narrow scope, written safety invariants, focused tests, and an explicit architecture decision.
5. Prioritize correctness, numerical robustness, memory safety, determinism, maintainability,
   measured performance, and cross-platform behavior—in that order.
6. Do not silently repair, discard, or reinterpret invalid or ambiguous geometry. Report it or
   require an explicit policy.
7. Do not expose placeholder APIs that imply unimplemented geometry or slicing behavior works.
8. Keep the processing core headless and independent of the future desktop framework.
9. Avoid speculative abstractions, premature optimization, empty future modules, and unnecessary
   crates.
10. Keep public interfaces documented. Update architecture and test documentation with relevant
    behavior changes.

## Performance and GPU rendering

Pay attention to performance throughout development, not as a late pass. Every change to the
processing core or the interface must consider its cost on large inputs (millions of triangles,
thousands of layers) and keep the interface responsive. This complements rule 9 above:
performance work is expected, but it must be measured, not speculative.

- The desktop interface must make full use of GPU hardware acceleration. Render the 3D scene and
  its overlays (models, plate, section caps, holes, supports, labels) on the GPU through the
  wgpu viewport; do not rasterize scene content on the CPU or draw it through egui shapes when
  the GPU pipeline can draw it.
- Upload geometry once and place it with transforms or uniforms. Do not rebuild or re-upload
  vertex buffers, textures or pipelines every frame; rebuild only when their content changes.
- Animate and fade with shader parameters or uniforms rather than regenerated geometry.
- Keep heavy work (import, preparation, slicing, picking structures) off the UI thread, and
  avoid per-frame allocations, full-mesh scans and blocking calls in the frame loop.
- Measure before and after performance-relevant changes (frame time, upload and preparation
  time, memory) on representative models, and report the numbers with the change.

## Numerical conventions

Until superseded by a recorded decision, use millimeters, right-handed model/world coordinates,
`f64` for principal geometric calculations, explicit context-specific tolerances, and deterministic
ordering. Never introduce an unexplained global epsilon. See `docs/ARCHITECTURE.md` for complete
coordinate, winding, contour, and error conventions.

## Dependencies

Do not add dependencies casually. Before adoption, record native-Rust status, source availability,
FFI/build-script use, transitive dependency tree, maintenance, tests, robustness evidence,
performance and memory characteristics, license compatibility, security posture, and Windows/Linux
support. Geometry dependencies require heightened review. Do not add a promising library merely to
reserve it for future work.

## Testing and quality gates

Add automated tests with every functional implementation. Select unit, integration, property,
regression, fuzz, and benchmark coverage in proportion to risk. Before handing off a change, run
when the toolchain is available:

```console
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
```

Report commands that could not run and distinguish code failures from environment failures. Do not
infer Windows compatibility from WSL/Linux results; validate native Windows separately.
For repeatable native runs, follow [tools/windows-native/README.md](tools/windows-native/README.md)
and its hash-verified snapshot runner. Preserve initial failures; distinguish application-control
launch blocks from executed test failures. A focused retry does not replace a complete workspace pass.

## Error handling and diagnostics

- Library code must return structured, actionable errors for recoverable failures rather than
  panic.
- Panics are reserved for violated internal invariants and must be justified by tests.
- Preserve the underlying cause and relevant geometry/entity context without exposing private data.
- CLI errors belong on standard error and must result in a nonzero exit status.
- Do not hide numerical ambiguity, invalid input, data loss, or partial output behind warnings alone.

## Documentation conventions

Explain invariants, coordinate systems, units, tolerance meanings, ownership, determinism, failure
modes, and non-obvious tradeoffs. Document public Rust items. Prefer concise comments that explain
why over comments that restate code. Record durable decisions in `docs/TECHNOLOGY-DECISIONS.md` and
keep milestone status and scope in `ROADMAP.md`.

READMEs are for readers of the software, not records of its development (owner, 2026-11-10):

- The repository `README.md` says what the software is and does, how to build, run and check it,
  how the repository is laid out and how to contribute, and indexes the documentation. It holds
  no milestone status, roadmap steps, analyses, research, validation results or decision records:
  those belong in `ROADMAP.md` and `docs/`, and the README only links to them.
- A README in a subfolder (a tool, an asset folder) says what the folder holds and how to use it;
  its sources, findings and history go in `docs/`, linked from it.
- When a change adds such content, put it in `docs/` and, if readers need it, add a link to the
  README's documentation index.
- The README's "Supported printers" table must always match the built-in catalog and what has
  been verified (owner, 2026-11-10). It is generated by `tools/assets/make_catalog.py` between
  its markers; never edit it by hand. Whenever a printer is added, removed or renamed, its image
  orientation is verified (`ORIENTATION`), or karasu-slicer's files are printed on it for the
  first time (`PRINTED`, with the date and the record that shows it), update the generator and
  run it in the same change. A test checks that the table lists every catalog printer.

## Clean-room development

karasu-slicer is developed in clean room (owner, 2026-11-12). Other programs, commercial or open
source, may inform what karasu-slicer does, never how it is written. Every change follows these
steps, in this order:

`behavioural research -> behavioural requirements -> independent design -> Rust implementation -> validation`

1. **Behavioural research.** Allowed sources:
   - public specifications, standards, papers and textbooks;
   - manufacturers' published documentation: manuals, product pages, settings tables, data sheets;
   - another program's behaviour as an ordinary user sees it: its interface, settings, messages and
     the results it produces from inputs prepared for the purpose;
   - files another program exports from such inputs, read as data to learn a file format;
   - data files (never code) published under a licence compatible with the project's, read as data.

   Not allowed, for any other program, open source included and whatever its licence: reading its
   source code, decompiling, disassembling or debugging it, or inspecting its binaries; bypassing
   licensing, encryption, signatures or other access controls; modifying an installed program;
   examining unrelated private information.
2. **Behavioural requirements.** State what was observed in your own words: the inputs, the
   settings, the observable result, the program and its version, the conditions, and whether each
   point is an observation, an inference or a hypothesis.
3. **Independent design.** Derive the algorithm from the requirements, mathematics and public
   literature. Record the design and its reasoning in `docs/`, and decisions in
   `docs/TECHNOLOGY-DECISIONS.md`.
4. **Rust implementation.** Write the code from the design alone.
5. **Validation.** Compare results (files, images, measurements) with other programs' results,
   never with their code.

Never put into this repository, in code, tests, comments, documents or commit messages:

- source code, pseudocode, decompiled or disassembled code of any other software, open source or
  commercial, nor a translation or paraphrase of it;
- descriptions of another program's internals: its functions, classes, modules, libraries or data
  structures;
- another program's assets, presets or profiles.

A document may mention another program only for its observed behaviour or its published facts,
citing public sources by address, version and date, and files by hash. Raw evidence (screenshots,
exported files, downloaded documents) stays outside the repository. Legal assessments, such as
licence or patent questions, are not recorded in the repository. Third-party code enters the
product only as a reviewed dependency (see Dependencies), never copied into the repository.
Contributions follow the same method (`CONTRIBUTING.md`, `CLA.md`).

## Repository and Git safety

Inspect Git status before editing. Preserve user files and unrelated changes. Do not commit, push,
change remotes, or create remote repositories unless explicitly asked. Commit messages must be
English, descriptive, and imperative. Commit messages must not contain `Co-Authored-By:` trailers
or any other reference to AI agents or assistants. The project is licensed AGPL-3.0-or-later
(owner, 2026-11-11, TD-145; `LICENSE`); its name and logo are trademarks, not licensed with the
code (`TRADEMARKS.md`), and external contributions need the CLA (`CONTRIBUTING.md`). Never
change the licence, relicense code or add code under an incompatible licence without explicit
owner approval.

