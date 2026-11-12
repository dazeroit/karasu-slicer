# M3 Native Windows Validation at 382fb62

## Result and identity

On 2026-10-17 the owner's request to diagnose and attempt native Windows validation completed
successfully against committed source `382fb62cd835e48d561b6f944ed9dac9f6f3fd7e`.
Windows 11 Pro, build 26200 (native `cmd.exe ver`: 10.0.26200.9711), runs stable
Rust/Cargo 1.99.0 with host/target `x86_64-pc-windows-msvc`. Rustc is
`b940084d7eb6a299eb4bfeb8e34901bc051e7ac4`, LLVM 23.1.1. Visual Studio 2022 Community
and the native MSVC toolchain are installed. No dependency/toolchain/security-policy change or
production fix was required.

| Native command | Result |
| --- | --- |
| `cargo +stable fmt --all -- --check` | PASS |
| `cargo +stable clippy --workspace --all-targets -- -D warnings` | PASS |
| `cargo +stable test --workspace` | **600 passed, 0 failed, 2 ignored** |
| `cargo +stable build --workspace` | PASS |
| `cargo +stable doc --workspace --no-deps --document-private-items` | PASS, warnings denied |
| `cargo +stable doc --workspace --no-deps` | PASS, warnings denied |
| Release `represented_coordinates` | 8 passed |
| Release `layer_composition` | 4 passed |
| Release `layer_engine` | 12 passed |
| Release `layer_schedule` | 26 passed |
| Release `constructive_material` | 8 passed |
| Release `region_hierarchy` | 5 passed |
| Release private `slicing::construct::exact::tests` | 8 passed |

Every command exits zero. The Linux baseline is 601 passed/0 failed/2 ignored; Windows omits the
existing Unix-only non-Unicode-path CLI test. Release counts are separate reruns, not additional
workspace test counts. Cargo operates offline; rustdoc warnings are denied through `RUSTDOCFLAGS`.

## Actual environmental obstacles

The previous conclusion that an absent visible `/proc/sys/fs/binfmt_misc/WSLInterop` entry meant
Windows was unreachable was incomplete. Mounted Windows executables were present. A sandboxed
native launch failed with `WSL ... UtilBindVsockAnyPort:307: socket failed 1`. The same `cmd.exe /d /c ver`
command succeeded with authorized execution outside the sandbox. This distinguishes a sandbox
restriction from a missing Windows installation or unusable MSVC toolchain.

Launching the prepared PowerShell `.ps1` file then failed with `UnauthorizedAccess` because script
execution was disabled. All listed execution-policy scopes were `Undefined`; no policy setting was
changed. The reviewed commands ran directly through `powershell.exe -NoProfile -NonInteractive
-Command`, using native `Start-Process`, actual process handles/exit codes, and separate stdout and
stderr files. No `ExecutionPolicy Bypass` setting or Windows application-control change was used.

Historical campaigns also retained application-control OS error 4551 while spawning CLI/doctest
executables. That failure did **not** recur in this campaign. Its earlier cause remains unresolved;
this successful run does not erase earlier failures or promise that host policy cannot block a
future generated executable.

## Export integrity and evidence

`git archive` exported the exact commit into a ZIP, without Git metadata or build artifacts.
Archive SHA-256: `e4230e50aa617b50581329677ccaba0fb00edcabee41facf521dfcc467fd57d9`.
A native Windows SHA-256 check verified all **150 files** before and after every validation command.
Commands ran in an isolated native temporary source tree:

`%LOCALAPPDATA%\Temp\resin-slicer-382fb62-f0b6072e1bb6466caf56f33c84afc3c3\source`

The adjacent `evidence` directory retains the archive, per-file manifest, original driver,
before/after hashes, per-command stdout/stderr and `results.json` containing actual exits and timing.
A Linux-side copy is `/tmp/resin-native-382fb62/`, including `evidence-sha256.json`.
The failed script-file launch and sandbox diagnostic remain documented above; neither is recorded
as a successful native quality gate. Documentation added after this run is not misrepresented as
part of the validated commit.

This closes native functional acceptance for the current M3 implementation, including schedule,
shared strict containment, owned engine, source material composition and represented coordinates.
It does not establish native fuzz/performance acceptance, agreed large-input budgets, whole-M3
closure or commercial-default agreement. Remaining corpus/budget/reference gates are unchanged.
