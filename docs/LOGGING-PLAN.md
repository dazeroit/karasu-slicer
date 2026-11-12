# Persistent logs: implementation plan (TD-096)

**Status:** done, 2026-10-23 (g1 to g4; results in [Progress](#progress)); the live window and
the diagnostic report followed on 2026-10-26 (section 5). Decision: [TD-096](TECHNOLOGY-DECISIONS.md).

## 1. Goal

A user who meets a problem can send us the log of the session it happened in, and we can debug
from it. Today:
- The application's messages live in memory and are lost when it closes.
- Its standard error goes nowhere on Windows, since the Release build has no console.
- A panic leaves no trace. An out-of-bounds GPU buffer in `Base_01.stl` at 0.2 mm closed the
  application, and the cause had to be inferred.

The same records also feed a live, terminal-like window in the interface later. That window is
not built now, but the design serves it from the start.

## 2. Design

- **Where:** `karasu_project::logging`, shared by the application and the command line. The
  geometry core stays without logging: its results already carry what matters.
- **Facade:** the [`log`](https://crates.io/crates/log) crate. Our own code logs through it, and
  so do egui, wgpu and winit, already in the tree. A wgpu validation error, like the one behind
  the 0.2 mm crash, is then logged without extra work. Our logger implements `log::Log`; no other
  crate is added.
- **Records:** one line each, in English, never localized:
  - the time in UTC (ISO 8601, milliseconds) and the time since the session started;
  - the level;
  - the area (`app`, `pipeline`, `hollow`, `holes`, `slice`, `verify`, `gpu`, and the third
    party's own targets);
  - the message.
- **Sinks:**
  1. **Session file:**
     - *Location:* `logs/` in the application's cache root: `%LOCALAPPDATA%\karasu-slicer\logs`
       on Windows, `$XDG_CACHE_HOME/karasu-slicer/logs` (or `~/.cache/…`) on Linux. Only
       `slices/` is emptied at start.
     - *Naming:* one file per session, `karasu-<UTC start>-<pid>.log`.
     - *Retention:* the 10 newest sessions are kept, older ones removed at start.
     - *Size:* each file stops at 20 MB, with a final line saying so; later records still reach
       the ring.
     - *Writing:* buffered, flushed on every warning, error and panic, and otherwise at least
       once a second by a background thread, so the interface never waits on the disk.
  2. **Ring:** the last 10,000 records in memory, each with a sequence number. The live window
     will read only the records after the last one it showed: no per-frame allocation and no
     lock held while drawing.
  3. **Command line:** `--log <file>` writes the same records to that file, at most 20 MB,
     without rotation. Without it nothing is written to disk. Standard error keeps its progress
     and errors as today.
- **Levels:**
  - our code records `info` and above by default;
  - third-party crates record `warn` and above, which keeps wgpu's validation errors;
  - `KARASU_LOG` (`error` to `trace`) raises or lowers our level, for a debugging session.
- **Panics:** a hook records the message, the place and a captured backtrace at `error`, flushes
  the file, then runs the default hook. The hook is installed before the window opens.
- **Never in the frame loop:** nothing is logged per frame. Records come from operations (an
  import, a preparation, a slice), not from drawing.
- **Determinism:** logging never changes a result or an output file.
- **Privacy:** the logs hold the paths of opened files and the hardware's name, and stay on the
  computer. Sending them is the user's choice. A later "save a diagnostic report" action can
  hide the paths.

## 3. What is recorded

| Area | Records |
| --- | --- |
| Session | version and commit, operating system, CPU threads, installed memory and the memory budget (TD-094), chosen GPU adapter, backend and driver |
| Models | import (file, size, format, unit, triangles, time), diagnosis and repairs (preset, what changed, time) |
| Preparation | each job's phases with durations; hollowing (wall, cell, offset, passes, guaranteed wall, cavity triangles, time); hole probe built or reused; drain holes cut or refused, with the reason |
| Slicing | request (printer, resin, format, layers), phases with durations, totals, the summary, and the output path |
| Verification | the walls, breaches, pockets, verdict and time (TD-095) |
| Interface | project opened or saved, export, undo or redo of a plate edit (one line each) |
| Problems | every message shown as a warning or an error, refusals with their remedies, GPU and windowing warnings, panics |

## 4. Phases

| Phase | Content | Acceptance |
| --- | --- | --- |
| **g1 Logger** | `karasu_project::logging`: the `log` facade, session file with retention and cap, ring, panic hook, `KARASU_LOG` | Unit tests for format, rotation, cap, ring order, levels, panic record |
| **g2 Application** | Installed at start; session facts; pipeline, preparation, slicing and verification records; messages mirrored as records | A session's log describes an import, a hollow, a hole, a slice and a verification; a panic in a test build leaves its record |
| **g3 Command line** | `--log <file>` | Same records; outputs unchanged (comparison suite) |
| **g4 Validation** | Cost on `Base_01.stl` (slicing time with and without logging), native Windows, documents | Overhead under 1 % of slicing time; Windows passes |

## Progress

- **g1 (2026-10-23), logger:** `karasu_project::logging`:
  - `Logger` implements `log::Log`; `install` sets it up for the process with the panic hook
    and a background flush every second;
  - `Destination` is the session folder, a file, or memory only;
  - `Ring::since` gives the records after a sequence number;
  - the defaults are `SESSIONS_KEPT`, `MAX_FILE_BYTES`, `RING_RECORDS` and `KARASU_LOG`.

  Unit tests cover the UTC times and file names, the one-line record format, the levels of our
  areas and of other targets, the ring's order and bound, the pruning of old sessions (other
  files untouched), the file's cap with its last line, and buffering until a flush. An
  integration test, in its own process, installs the logger and checks:
  - records through the facade and other libraries' warnings are kept, and filtered records are
    not;
  - a second installation is refused;
  - a caught panic is recorded with its backtrace and flushed to the file.

- **g2 (2026-10-23), application:** the application installs the logger at start: session
  files in the cache root's `logs/`, or memory only when that fails (said on standard error).
  - *Session facts:* version and build, system and threads, then the installed memory and the
    memory budget, and the graphics adapter, backend and driver once chosen.
  - *Pipeline:* every phase of 0.1 s or more is logged with its duration (progress within a
    phase is one phase; shorter phases at `debug`). The pipeline also logs:
    - the slice's request and its end (layers, output and time), then its summary;
    - each preparation's objects and time, with per-object failures;
    - imports;
    - each hollow (wall, cell, offset, passes, guaranteed wall, cavity triangles);
    - drain-hole indexes built, and holes refused or unavailable;
    - the wall verification's findings and verdict.
  - *Interface:* every message shown is recorded with its level and detail, and opened files are
    logged.
  - *Test:* a test installs the logger in memory and verifies a hollowed, drained cube from the
    layer panel. The ring holds every step: the file opened and imported, the slice asked for,
    the hollow, the drain-hole index, the verification, the phase times, the end, the summary,
    and the "Walls verified" message.

- **g3 (2026-10-23), command line:** `slice --log <file>`.
  - *What it does:* it installs the logger on that file (replaced, capped at 20 MB). It logs the
    session's facts and the command line, then the pipeline's records. A failed job's error is
    logged before the exit.
  - *Errors:* a log that cannot be created fails the job (exit 1). The option may be given only
    once.
  - *Test:* the output is byte-identical with and without `--log`, the log holds the job, and a
    refused job's error is logged.

- **g4 (2026-10-23), validation:**
  - *Cost:* `Base_01.stl` sliced at 1.3 mm, three runs each way, alternated:
    - without a log: 7.16, 7.08 and 7.04 s;
    - with `--log`: 6.92 and 7.05 s, and a first run at 9.86 s (a cold start, not repeated).

    The overhead is within the noise, under the plan's 1 %. The job's log is 3.9 KB.
  - *Native Windows* (`validate.ps1 -IncludeApp`): every gate passed, with 1,287 tests in Debug
    and in Release.
    - The application's three smoke runs left three session files in
      `%LOCALAPPDATA%\karasu-slicer\logs`. They hold the version, the system, the adapter (Vulkan,
      and the OpenGL fallback), the memory and its budget, the file opened and the message shown.
    - An `egui_wgpu` warning was recorded too, so other libraries' records are collected.

## 5. The live window and the diagnostics (done 2026-10-26, `karasu-app::log_window`)

- **Live log window** (Windows › Log, a tool window): the ring's records as they come, one row
  per line, each record's first line with its time since the start, level and area; filtered by
  level (everything, warnings and errors, errors), area and text; "Follow" keeps the newest in
  view; "Copy" copies what is shown; "Clear the view" hides the records so far (the file keeps
  them). While open it asks the ring only for the records after the last it has, four times a
  second, and draws only the rows in view; closed, it reads nothing.
- **Diagnostics:** "Open the logs folder", and "Save a diagnostic report…": the application,
  the system, the memory and the graphics adapter, then the session's log file (or the records
  in memory without a file). "Hide my home folder", on by default, writes `~` for it, where
  the user's name appears in paths.
