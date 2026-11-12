# M5.5-A validation

## Scope

Executable immutable plan/history contracts and the A design are implemented. Hollowing, cut/support
geometry, solid certification, scene evaluation and B–E integration acceptance remain unimplemented.
No dependency or reference implementation is imported. Source digests are caller assertions; passing
parameter tests does not certify an asset or solid. The specification defines the mathematical design,
provisional backend domain and independent geometry acceptance fixtures for subsequent increments.

## Linux

| Check | Result |
| --- | --- |
| Complete workspace tests | **736 passed, zero failed, two ignored** |
| Focused preparation integration suite, Debug and Release | **18 passed each** |
| Final fmt, strict workspace/all-target clippy, workspace build | PASS |
| Warnings-denied private-item rustdoc | PASS |

The first quality pass identified two assertion-formatting differences in the new tests. Those were
corrected; the final formatting check passes. Full functional tests passed before this whitespace-only
correction, and the final focused suite, clippy and build pass afterward. Initial logs remain retained.
Evidence is under `/tmp/karasu-m55-validation`; no generated geometry or printer execution is claimed.

The 18 integration tests cover independent canonical record header/source/affine decoding, malformed
numeric/range data, explicit order/IDs, exact upstream support bindings, source/placement/sampling/
model-prefix invalidation, support-only edits, fixed-world cuts, signed-zero canonicalization,
overflow-safe direction calculations, reconstruction identity, exact/one-over budgets, transactional
cancellation, undo/redo, redo branching and generation ABA protection.

## Native Windows

Use the checked-in [snapshot procedure](../tools/windows-native/README.md). Final input is
`/tmp/karasu-m55-native-final`: a 210-file manifest and SHA-256 archive
`31369329e5548c646024a1b9c136e17e75622eef3c0776776878c974ea80a6f1`.
The snapshot includes the uncommitted A implementation based on `b2306e5`, not a newly committed
revision. Final application Rust and tests match it; later changes reconcile documentation only.

Native root:
`%LOCALAPPDATA%\Temp\karasu-native-a7f41d44eb424297946a6f255201740d`.
Primary evidence: `evidence-024b570a939244158cafc09c41395ea8`; focused checks:
`focused-preparation`; complete unchanged Debug retry: `debug-workspace-retry`.

| Native check | Result |
| --- | --- |
| Snapshot integrity before/after, and after focused/retry runs | PASS, all 210 files |
| Final fmt, strict all-target clippy, workspace build | PASS |
| Full workspace Release suite, including doctests | **735 passed, zero failed, two ignored** |
| Full workspace Debug suite, unchanged complete retry including doctests | **735 passed, zero failed, two ignored** |
| Focused preparation suite, Debug and Release | **18 passed each** |
| Warnings-denied public/private rustdoc | PASS |

The single-test difference from Linux is existing Unix-only coverage. The initial final-snapshot
Debug attempt stopped after 257 passing tests because Windows Application Control rejected the
preexisting `layer_schedule` executable before it ran (4551). Its unchanged complete workspace retry
passes all 735 tests. Initial Code Integrity/log evidence is retained and is not counted as success.
The first earlier 210-file snapshot also retains its formatting failure and dependency build-script
policy blocks under `/tmp/karasu-m55-native` and native root
`%LOCALAPPDATA%\Temp\karasu-native-3989d05e186b40d2b3dd43315cd6dbfb`.

No policy, binary, signing identity, trust marker or code-generation setting was altered to evade
Windows enforcement. Successful unchanged/native runs establish functional acceptance of the final
snapshot, not durable host-policy repair. Future 4551 events still require separate recording.

## Consequences and limits

M5.5-A's implemented contracts/design pass their Linux and native Windows gates. M5.5-B–E geometry
and prepared-scene export remain pending. This does not certify global mesh validity, hole/cavity
connectivity, support strength, geometric error or physical printing. Subsequent B admission must
prove its chosen oracle/reconstruction domain and budgets before calling geometry complete.

The native complete Release run also exercises all existing M5 profile/job/export tests, resolving
M5's outstanding software Release gate for the unchanged exporter implementation. M5 independent
reader, artifact and performance evidence remains in [M5 validation](M5-VALIDATION.md); no new physical
printer or encrypted-format support is inferred. The current milestone does not regenerate M5
large export benchmark artifacts or claim a new throughput baseline.
