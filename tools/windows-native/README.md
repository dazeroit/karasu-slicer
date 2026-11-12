# Native Windows validation

Use native Windows Rust/MSVC on a local Windows filesystem. WSL Linux tests, Linux cross-compilation,
and successful creation of an `.exe` do not establish native test execution. The runner validates a
hash-verified snapshot, preserves each command's exit code/stdout/stderr and leaves source and host
security policy unchanged. It runs both complete workspace configurations, including doctests.

## Prerequisites

Windows stable Rust with the `x86_64-pc-windows-msvc` host, rustfmt, clippy, MSVC linker and Windows
SDK must already work. Use a normal user session. The native Cargo dependency cache must contain the
locked dependencies; the default run is offline. Missing toolchain/components/dependencies are
prerequisite failures, not code failures. The script does not install tools or select another target.
Check for custom `RUSTFLAGS`, Cargo runners/wrappers, target directories or target configuration before
validation and record any deliberate deviation. Use the default workspace profiles for acceptance.

Review `git status --short` first. The Python preparation script includes tracked working-tree files;
add `--include-untracked` only after reviewing nonignored new files. Missing tracked files and symlinks
fail explicitly. Inputs and generated artifacts live outside the repository. Use a new input path:

```console
python3 tools/windows-native/prepare.py /tmp/karasu-native-m5 --include-untracked
```

The directory contains `source.zip`, a SHA-256 manifest and snapshot metadata (commit, dirty status,
file count, archive hash). A dirty tree can be tested, but must not be described as an exact committed
revision. Source must remain unchanged while preparing; before/after validation hashes concern the
exported snapshot, not an independently changing live checkout.

From WSL, invoke Windows PowerShell explicitly. Adapt the distribution name and checkout path:

```console
/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe -NoProfile -ExecutionPolicy Bypass -File '\\wsl.localhost\Ubuntu\path\to\karasu-slicer\tools\windows-native\validate.ps1' -InputRoot '\\wsl.localhost\Ubuntu\tmp\karasu-native-m5' -IncludeExamples
```

`-ExecutionPolicy Bypass` applies only to this PowerShell process so the reviewed validation script
can run from WSL; it does not disable Windows App Control, Defender or Smart App Control. Do not
change persistent execution policy. An organization may prohibit this launch mode: honor that policy
and use its approved script-signing/execution process. The script itself performs no policy changes.

Alternatively, prepare/copy inputs and the runner from a native checkout, then invoke it in an
approved Windows PowerShell session:

```powershell
.\tools\windows-native\validate.ps1 -InputRoot 'C:\validation-input\karasu-m5' -IncludeExamples
```

Builds and executable artifacts go into a fresh `%LOCALAPPDATA%\Temp\karasu-native-<guid>\source`
directory. UNC is only the transport/input path. `native-root.txt` and `latest-evidence.txt` in the
input directory identify results. The runner quotes native command arguments, retains a process
handle before waiting (important for Windows PowerShell exit-code collection), and treats missing
exit codes or launch exceptions as failure. It never infers success from a completed log or a count
of test lines alone. No retry automatically overwrites original logs.

`-Online` permits native Cargo dependency downloads using the existing user configuration, if
explicitly needed. It does not install toolchains. Resolve an offline cache failure separately and
record it; do not call it a code regression. `--locked` prevents dependency version updates.

## Gates and result review

The default sequence records native rustc/Cargo versions, fmt, all-target warnings-denied clippy,
complete Debug and Release workspace tests, workspace build and warnings-denied public/private
rustdoc. `-IncludeExamples` additionally builds the Release `print_reference` example and exports
small and full-resolution P1 fixtures. `-IncludeApp` builds the Release desktop application
(the crate `karasu-app`, executable `karasu`, M6) and runs its `--smoke-test` three times: with the automatic adapter choice,
with the OpenGL fallback (`--gpu-backend gl`), and with the tracked FX-03 sphere opened in the
viewport (a measured camera orbit reporting frame times and triangles). Each run must open the
window from the interactive user session, render frames and print the chosen adapter as JSON on
stdout. All checks are attempted
so an early host blocker does not hide unrelated failures. A nonzero command keeps the overall run
failed.

Check `results.json`, stderr and stdout. Test counts differ by platform because of the existing
Unix-only path test; ignored tests are not passing tests. Verify `before.sha256`/`after.sha256` and
compare current implementation files with the snapshot before attributing results to a newer tree.
Preserve the evidence externally if the temporary directory might be removed. Raw logs may contain
user paths and should not be copied wholesale into public documentation.

Validate completed example output outside timing with `tools/m5-validate`. Compare every corresponding
artifact with the Linux baseline. CTB/GOO/Photon and reference files should match exactly for the same
fixture. PP1 records OS provenance: compare ZIP entry names and every member, permitting only the
`software_info.conf` `os` value to differ (`linux`/`windows`); all other metadata and mask/preview/scene
bytes must match. Do not normalize arbitrary JSON or ignore unexplained differences.

## Diagnosing process launch blocks

OS error **4551** means an application-control policy prevented execution. It is separate from a Rust
assertion failure, and PowerShell script execution policy is a different control. Read the relevant
`Microsoft-Windows-CodeIntegrity/Operational` events, filtered to this validation directory and time.
The runner collects matching 3033/3076/3077/3089 records when available; no events or denied log access
are recorded separately and do not prove that no policy is active. Event 3077 contains the enforced
policy name, blocked path and hashes. Correlated 3089 signature records can require matching the XML
ActivityID because their messages do not always contain a path; obtain those only when needed.

For the exact blocked executable, retain its full SHA-256, signature status, size and native command.
`Get-FileHash`, `Get-AuthenticodeSignature`, and `Get-Item -Stream *` are read-only diagnostics.
An absent `Zone.Identifier` does not imply App Control trust; deleting download markers does not
solve a policy rejection. Do not sign/patch/rename the binary, change code generation merely to
change its hash, disable protection, or create security exclusions to turn a failed run green.

One unchanged retry is useful for transient host assessment, retaining all failures. To rerun all
gates on the same snapshot and binaries with a new evidence directory:

```powershell
.\tools\windows-native\validate.ps1 -InputRoot 'C:\validation-input\karasu-m5' -ExistingRun 'C:\Users\user\AppData\Local\Temp\karasu-native-<guid>' -IncludeExamples
```

Use the same original input manifest. Never treat focused-only success as a complete Release pass;
rerun the whole workspace configuration. Repeated rejection of the same binary calls for diagnosis,
not unlimited retries. A fresh default build can verify clean reproducibility without reusing stale
artifacts, but is not a durable policy repair and its results apply to its newly recorded hashes.
If policy continues to block execution, report native acceptance as environment-blocked. The owner
must provide a development/CI Windows environment with an approved trust/signing policy before that
gate can close. This runner does not alter policy or provision signing identities.

Primary background: Microsoft's [App Control troubleshooting](https://learn.microsoft.com/en-us/windows/security/application-security/application-control/app-control-for-business/operations/appcontrol-debugging-and-troubleshooting)
and [Smart App Control overview](https://learn.microsoft.com/en-us/windows/apps/develop/smart-app-control/overview).
See [M5 validation](../../docs/M5-VALIDATION.md) for the actual runs and outcomes; observations on one
host are not a universal remedy for policy failures.

## PowerShell notes

Windows PowerShell 5.1 specifics matter: assign the JSON manifest array directly from ConvertFrom-Json
rather than wrapping its nonenumerated result in another array, and read XML event values through
InnerText so empty fields work under strict mode. The final collector was exercised against actual
3077/3033 events. The command runner was also smoke-tested with success, nonzero exit and missing
executable cases. Native command quoting targets Cargo/Rust/native executables, not cmd.exe shell
command strings, which require different quoting rules.

The M5 revalidation run with this procedure is recorded in
[docs/TOOL-RECORDS.md](../../docs/TOOL-RECORDS.md#native-windows-validation-observed-m5-revalidation).
