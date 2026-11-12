# M5 validation and supported claim envelope

## Scope and evidence

Validation covers immutable jobs, strict versioned printer/resin persistence, reference PNG/JSONL,
CTB v2 plain, GOO v3, firmware-motion Photon v1 and single-material P1 PP1 v3. The implementation
uses independent safe Rust algorithms informed by public protocols and reference behaviour.
Specifications are in the adjacent M5 specification records. Raw evidence remains outside the
repository.
These paths are local session evidence, not durable checked-in fixtures or a promised installation.

## Linux quality and independent readers

Final workspace tests: **718 passed, zero failed, two ignored**. Formatting, warnings-denied
workspace/all-target clippy, workspace build, release example build and warnings-denied private-item
rustdoc pass. The twelve independent Python reader unit tests pass. Reader checks cover container
framing, CRC/checksums, dimensions, metadata, scene records and decoded masks; malformed/truncated
inputs are rejected. Public unmodified uv3dp reads generated CTB/Photon and the independent public
GOO crate reads GOO; pinned sources and artifact hashes are in the format research record.

The independent PP1 reader validates all three generated P1-resolution masks against reference PNG,
including exposure/Z/controller data, preview PNGs and encoded coverage volume. Each decoded mask
has SHA-256 `8f9792bf7028cc49c9b5b65611946a7e8218826a4b9275a95ae277914fdbee34`.
The PP1 artifact SHA-256 is `2d9ce3fea0eecfc83305251953e7556fd40643681c05e8c91f2119d7a3feb03c`.
The official public P1 sample is independently checked for metadata and its first complete layer;
commercial scene semantics are explicitly separated from Karasu raster scene statistics.

## Hostile profile JSON

The ASan profile JSON campaign completed **2,660,659 executions in 26 seconds**, with no findings
and 527 MiB peak memory against a 1,024 MiB cap. Leak detection was disabled because of the host
ptrace limitation; this is not a leak-clean result. Fixed and retained corpus replay passes. This
campaign targets JSON/profile admission, not all codecs or the complete application.

## Streaming measurements

The declared preliminary acceptance budget is <=5 seconds per layer on the specified P1 fixture,
<=512 MiB whole-process RSS and no retained mask stack. Three fresh-process runs were executed
serially after a separate initial concurrent smoke run. Host scheduling, CPU frequency and filesystem
cache are uncontrolled. P1/reference independent decoding ran outside timing but could compete for
host resources; these are functional budget observations, not isolated throughput comparisons.

| Case | Layers / resolution | Whole-process elapsed runs | Peak RSS runs (KiB) |
| --- | --- | --- | --- |
| Small, all four formats plus reference | 3 / 64x40 | 0.02, 0.02, 0.02 s | 3272, 3272, 3276 |
| Many, all four formats plus reference | 2000 / 64x40 | 5.09, 4.71, 5.21 s | 3792, 3788, 3808 |
| P1 PP1 plus reference | 3 / 13312x5120 | 3.46, 4.62, 7.47 s | 69772, 69772, 69768 |

Even the slowest complete P1 process is below 2.5 seconds per layer, including both PP1 and
reference regeneration/output. The P1 source mask is 65 MiPixels; all three layers are independently
regenerated and streamed. Metadata remains proportional to layer count and explicitly capped.
The PP1 file is 238,762 bytes; its three stored-DEFLATE reference PNGs and metadata total
204,546,213 bytes. Settings, orientation and geometry are uncalibrated test fixtures.
Repeated completed artifacts are compared by exact SHA-256, outside the timed interval.

## Native Windows

A fresh 199-file native source snapshot passed SHA-256 integrity checks before and after validation.
Native Windows Debug passes **717 tests, zero failed, two ignored**. Native fmt, strict clippy,
workspace build, private/public rustdoc, release profile/job suites, private codec tests, release
example build and small/P1 example exports pass. Final Linux rerun remains 718/0/2.

In the initial run, the native Release workspace suite and focused `print_export` suite were **environment-blocked**:
Windows Application Control rejects `print_export-9d38e2476adfaf6d.exe` before execution with OS
error 4551. An unchanged focused and full retry reproduce the blocker. Original and retry logs are
retained; no security policy, executable or trust metadata was changed. This is not a passing Release
suite and not a Rust assertion failure. M5 native Release acceptance remains open.

Native evidence root:
`%LOCALAPPDATA%\Temp\karasu-m5-validation-741ac0b5d86f46eabe223343b4070a51`.
The source snapshot precedes final documentation reconciliation; the only subsequent Rust source
change clarifies an ExportLimits doc comment. Executable implementation and tests are unchanged.

All CTB/GOO/Photon and reference files are byte-identical across Linux/Windows. PP1 differs only in
`software_info.conf` OS provenance (`linux` versus `windows`); every other ZIP member is identical,
including masks, controller records, scenes and previews. This intentional provenance difference
changes ZIP lengths/offsets/CRCs and whole-file hashes. PP1 is deterministic on each platform, not
byte-identical across platform identities. Independent native small-PP1 validation also passes.

### Revalidation and repeatable procedure (2026-10-17)

The unchanged original Release retry still fails before `print_export` executes. Read-only Code
Integrity inspection confirms enforcement event 3077, policy `VerifiedAndReputableDesktop`, status
`0xc0e90002`, requested signing level 2 and validated level 1. The unsigned test executable has full
SHA-256 `33b6297be6c43e064c016c13c932d73721ff376935ce9b3ef2f8077278c87633`.
Its only file stream is ordinary data; there is no Zone.Identifier download marker.

A separate clean build uses the checked-in [native Windows procedure](../tools/windows-native/README.md),
a fresh local Windows directory and a 203-file hash-verified snapshot based on commit `c16fb19`.
Only the new validation tooling is untracked in this snapshot; application Rust matches the commit.
Native toolchain: Rust 1.99.0 (`b940084d7`), host `x86_64-pc-windows-msvc`, LLVM 23.1.1.

| Fresh native check | Result |
| --- | --- |
| Source integrity before/after | PASS, 203 files |
| fmt, strict all-target clippy, workspace Debug build | PASS |
| Complete workspace Debug suite, including doctests | **717 passed, zero failed, two ignored** |
| Warnings-denied public/private rustdoc | PASS |
| Workspace Release suite | Environment-blocked during dependency build; no complete suite executed |
| Release print example build | Environment-blocked by the same dependency |
| Small and P1 example execution | Not executed: Release executable was not built |

The clean Release failure is another enforced 3077 event, now rejecting the newly built unsigned
`proc-macro2` build-script executable. Its full SHA-256 is
`c97f13415491180e0788bb4488365aa91e9d8e04aa27c3a93a27d3136d2cd41b`.
The same policy/status/signing levels apply. This confirms the blocker is not restricted to the
previous export test artifact. No application source, security policy, signature, trust metadata or
compiler settings were changed to avoid the rejection. A clean local directory alone does not solve
host policy. This does not indicate a Rust assertion or dependency source failure.

Fresh native evidence root:
`%LOCALAPPDATA%\Temp\karasu-native-1091cc69e40f474c9471b6061281fd18`,
under `evidence-862879725c9c49d98ccd8e330ceec593`. Source inputs and Linux rerun logs are in
`/tmp/karasu-native-m5-revalidation-final`. The first collector recorded an XML empty-field parsing
error; its original failure is retained. The corrected XML InnerText collector successfully extracted
matching enforcement events to `code-integrity-events.json` at the native run root. The final runner
contains that correction. Separate runner smoke checks verify exit 7, missing-executable failure and
exit 0; a native PowerShell parser check covers the final script. Documentation changes after the
snapshot do not imply another application build.

Linux fmt/clippy/test/build rerun passes, with **718/0/2**. Native Release acceptance remains open.
Use a development/CI Windows environment whose owner-approved application-control/signing policy
permits both locally generated Cargo build scripts and test executables, then run all gates again.
No universal policy workaround or automatic trust change was found or applied. A focused success,
Debug-only pass or fresh directory cannot substitute for the complete Release gate. Physical printer
validation remains separate.

## Limits

No physical print, device/network submission, cloud service, dual-material registration or resin
calibration is validated. P1 orientation and firmware acceptance need representative hardware tests.
CTB is the plain v2 reader-tested subset; encrypted-only firmware is not claimed. Legacy Photon
explicitly delegates motion to firmware. Exact grayscale conversion rejects unrepresentable values;
loss requires explicit RoundToFormat. Nominal time excludes firmware acceleration/sensors/heating;
coverage volume is not measured resin use.

Machine publication never replaces an existing destination. It links atomically on the same
filesystem and otherwise (FAT32/exFAT) copies into an exclusively created destination (TD-049).
Reference directory publication requires cooperative ownership of the parent to avoid a concurrent
empty-directory creation race. Sync does not establish universal power-loss durability on every
filesystem. Individual profile JSON is bounded; no total-library size bound is claimed.

## Subsequent native gate closure during M5.5-A (2026-10-17)

The final [M5.5-A snapshot](M55-VALIDATION.md) completes both entire native Windows configurations:
**735 passed, zero failed, two ignored** in Debug (unchanged complete retry) and Release. All existing
M5 profile/job/export tests are included and machine encoder/exporter implementation files are
unchanged from the M5 baseline. This resolves the previously open M5 software Release acceptance
gate for the tested snapshot. Initial and repeated blocked attempts above remain historical evidence;
no application-control policy repair or physical-device acceptance is inferred. Independent reader,
format subset, quantization, atomicity, provenance and physical-print limitations remain unchanged.
