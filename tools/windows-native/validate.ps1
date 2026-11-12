# Native Windows validation. No policy, signing, exclusion or trust changes are performed.
[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$InputRoot,
    [string]$ExistingRun,
    [switch]$Online,
    [switch]$IncludeExamples,
    [switch]$IncludeApp
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($env:OS -ne 'Windows_NT') { throw 'Run this script with native Windows PowerShell.' }
if ($ExistingRun) {
    $runRoot = [IO.Path]::GetFullPath($ExistingRun)
    $source = Join-Path $runRoot 'source'
    if (!(Test-Path -LiteralPath $source -PathType Container)) { throw 'Existing source snapshot is missing.' }
} else {
    $runRoot = Join-Path $env:LOCALAPPDATA ('Temp\karasu-native-' + [guid]::NewGuid().ToString('N'))
    $source = Join-Path $runRoot 'source'
    New-Item -ItemType Directory -Path $source | Out-Null
}
if ($runRoot.StartsWith('\\')) { throw 'Build and execute on a local Windows filesystem, not UNC/WSL.' }
$evidence = Join-Path $runRoot ('evidence-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $evidence | Out-Null
$runRoot | Set-Content -LiteralPath (Join-Path $InputRoot 'native-root.txt')
$evidence | Set-Content -LiteralPath (Join-Path $InputRoot 'latest-evidence.txt')
foreach ($name in @('manifest.json','snapshot.json')) {
    Copy-Item -LiteralPath (Join-Path $InputRoot $name) -Destination $evidence
}
Copy-Item -LiteralPath $PSCommandPath -Destination (Join-Path $evidence 'validate.ps1')
$snapshot = Get-Content -Raw -LiteralPath (Join-Path $evidence 'snapshot.json') | ConvertFrom-Json
$manifest = Get-Content -Raw -LiteralPath (Join-Path $evidence 'manifest.json') | ConvertFrom-Json
if (!$ExistingRun) {
    $archive = Join-Path $InputRoot 'source.zip'
    if ((Get-FileHash -Algorithm SHA256 -LiteralPath $archive).Hash.ToLowerInvariant() -ne $snapshot.archive_sha256) { throw 'Snapshot archive hash mismatch.' }
    Expand-Archive -LiteralPath $archive -DestinationPath $source
}
function Verify-Source([string]$name) {
    $lines = foreach ($entry in $manifest) {
        if ([IO.Path]::IsPathRooted($entry.path) -or $entry.path.Split('/') -contains '..') { throw 'Invalid manifest path.' }
        $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $source $entry.path)).Hash.ToLowerInvariant()
        if ($actual -ne $entry.sha256) { throw ('Source hash mismatch: ' + $entry.path) }
        $actual + '  ' + $entry.path
    }
    $lines | Set-Content -LiteralPath (Join-Path $evidence ($name + '.sha256'))
    Write-Output ('SOURCE_INTEGRITY ' + $name + ' PASS files=' + $manifest.Count)
}
Verify-Source 'before'
$env:RUSTDOCFLAGS = '-D warnings'
$env:CARGO_NET_OFFLINE = if ($Online) { 'false' } else { 'true' }
# Explicit native tools; do not accidentally invoke a Linux Cargo shim.
$cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
$rustc = Join-Path $env:USERPROFILE '.cargo\bin\rustc.exe'
$script:results = @()
function Run-Native([string]$name, [string]$exe, [string[]]$arguments) {
    # Start-Process joins ArgumentList; explicitly quote each argument for paths with spaces.
    $quoted = foreach ($argument in $arguments) {
        if ($argument.Contains('"') -or $argument.EndsWith('\')) { throw 'Unsupported quoted/trailing-backslash command argument.' }
        '"' + $argument + '"'
    }
    $stdout = Join-Path $evidence ($name + '.stdout.log')
    $stderr = Join-Path $evidence ($name + '.stderr.log')
    Write-Output ('START ' + $name)
    $started = [DateTime]::UtcNow
    $code = -1
    $launchError = $null
    try {
        $process = Start-Process -FilePath $exe -ArgumentList $quoted -WorkingDirectory $source -NoNewWindow -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
        # Materialize the handle before WaitForExit for reliable Windows PowerShell exit codes.
        $handle = $process.Handle
        $process.WaitForExit()
        if ($null -eq $process.ExitCode) { throw 'Missing native exit code.' }
        $code = $process.ExitCode
    } catch {
        $launchError = $_.Exception.Message
        $launchError | Add-Content -LiteralPath $stderr
    }
    $script:results += [pscustomobject]@{name=$name;exit=$code;executable=$exe;arguments=$arguments;launch_error=$launchError;started_utc=$started.ToString('o');elapsed_seconds=([DateTime]::UtcNow-$started).TotalSeconds}
    ConvertTo-Json -InputObject @($script:results) -Depth 6 | Set-Content -LiteralPath (Join-Path $evidence 'results.json')
    Write-Output ('END ' + $name + ' exit=' + $code)
}
$startedValidation = Get-Date
Run-Native 'rustc-version' $rustc @('+stable','-Vv')
Run-Native 'cargo-version' $cargo @('+stable','-V')
Run-Native 'fmt' $cargo @('+stable','fmt','--all','--','--check')
Run-Native 'clippy' $cargo @('+stable','clippy','--workspace','--all-targets','--locked','--','-D','warnings')
Run-Native 'test-debug' $cargo @('+stable','test','--workspace','--locked')
Run-Native 'test-release' $cargo @('+stable','test','--workspace','--release','--locked')
Run-Native 'build' $cargo @('+stable','build','--workspace','--locked')
Run-Native 'rustdoc-private' $cargo @('+stable','doc','--workspace','--no-deps','--document-private-items','--locked')
Run-Native 'rustdoc-public' $cargo @('+stable','doc','--workspace','--no-deps','--locked')
if ($IncludeExamples) {
    Run-Native 'print-example-build' $cargo @('+stable','build','--release','-p','karasu-core','--example','print_reference','--locked')
    $example = Join-Path $source 'target\release\examples\print_reference.exe'
    Run-Native 'print-small' $example @((Join-Path $evidence 'sample'))
    Run-Native 'print-p1' $example @((Join-Path $evidence 'p1'),'--p1','3')
}
if ($IncludeApp) {
    # M6 desktop application: Release build, then smoke tests that open the window, render a few
    # frames and print the chosen graphics adapter (automatic selection, then the OpenGL fallback).
    Run-Native 'app-build' $cargo @('+stable','build','--release','-p','karasu-app','--locked')
    $app = Join-Path $source 'target\release\karasu.exe'
    Run-Native 'app-smoke-auto' $app @('--smoke-test')
    Run-Native 'app-smoke-gl' $app @('--smoke-test','--gpu-backend','gl')
    # The viewport with a tracked model: import, render and a measured camera orbit.
    $sphere = Join-Path $source 'fixtures\reference\stl\fx-03-sphere.stl'
    Run-Native 'app-smoke-model' $app @('--smoke-test',$sphere)
}
Verify-Source 'after'
# Collect only events concerning this validation root; never dump unrelated machine activity.
try {
    $events = @(Get-WinEvent -FilterHashtable @{LogName='Microsoft-Windows-CodeIntegrity/Operational';StartTime=$startedValidation;Id=3033,3076,3077,3089} -ErrorAction Stop | Where-Object { $_.Message -like ('*' + (Split-Path -Leaf $runRoot) + '*') })
    $records = foreach ($event in $events) {
        $xml = [xml]$event.ToXml()
        $fields = @{}
        foreach ($field in $xml.Event.EventData.Data) { $fields[$field.GetAttribute('Name')] = $field.InnerText }
        [pscustomobject]@{id=$event.Id;time=$event.TimeCreated.ToUniversalTime().ToString('o');data=$fields}
    }
    ConvertTo-Json -InputObject @($records) -Depth 6 | Set-Content -LiteralPath (Join-Path $evidence 'code-integrity.json')
} catch {
    $_.Exception.Message | Set-Content -LiteralPath (Join-Path $evidence 'code-integrity-unavailable.txt')
}
$failed = @($script:results | Where-Object { $_.exit -ne 0 })
Write-Output ('NATIVE_ROOT ' + $runRoot)
Write-Output ('EVIDENCE ' + $evidence)
Write-Output ('FAILED_COMMANDS ' + $failed.Count)
if ($failed.Count -ne 0) { exit 1 }
exit 0
