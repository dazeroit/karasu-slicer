# M6.4h — Mandatory preparation scenarios from the interface

**Status:** done on 2026-10-30. Decision: TD-111.
- The automated scenarios pass on Linux and native Windows, with byte-identical exports.
- The owner's interactive run on native Windows passed every item of the checklist below.
- Linux has no desktop session on this host, so the Linux run is the automated one.

The ROADMAP's M7 lists seven end-to-end preparation scenarios that must pass on native Windows and
Linux. M6.4h runs them from the desktop application. Each scenario is run in two ways:

- **Automated:** `crates/karasu-app/src/scenarios.rs` drives the interface's own state: the tools'
  clicks and hovers, the panels' edits, undo and redo, the Slice panel's job, and its export. It
  does not call the core directly. It runs with every `cargo test`, so the native Windows
  validation runs it too.
- **Interactive:** the owner follows the checklist below in the application.

## Scenario matrix

Every scenario uses a 20 mm cube written as an STL file, on the Anycubic Photon Mono X. It
exports in the printer's own format (`.pwmx`), so the bytes compared are the file that would be
printed. Every scenario that exports also checks that the STL file's bytes and the imported mesh
never change.

| ROADMAP scenario | Automated test | What it checks |
| --- | --- | --- |
| Import a solid STL, hollow it, add drain holes, slice and export | `a_solid_model_is_hollowed_drained_sliced_and_exported` | Hollowed through the Hollow panel's path; the hole placed with the Drain holes tool (hover, click, Apply). The guaranteed wall is at least 2 mm, the hole opens the cavity, drainage is complete, and no warning is shown. |
| Import a model, add, edit and remove supports, slice and export | `supports_are_added_edited_and_removed_then_sliced_and_exported` | Three supports placed by clicking the underside. One is selected by clicking its shaft and made Heavy; another is selected and deleted. Two supports are sliced, both standing on the plate. |
| Prepare and export a combined hollowed, drained and supported model | `a_hollowed_drained_and_supported_model_is_sliced_and_exported` | The drainage is complete and three supports are sliced, with no warning. |
| Edit preparation parameters and recompute without modifying the source | `preparation_edited_is_recomputed_and_the_source_never_changes` | After a wall edit, the slice is out of date and is not current. The new slice differs, with a wall between 1.5 and 2 mm. Setting the wall back gives a file byte-identical to the first. |
| Reopen or recompute saved preparation | `undoing_and_redoing_the_preparation_replays_it_exactly` | **Saving and reopening is not applicable:** project files come in M6.6. In-memory replay is checked instead: undoing all six steps (lift, hollowing, hole, three supports) gives the unprepared file again, and redoing them gives the prepared file, byte for byte. |
| Reproducible prepared geometry and output | `the_same_preparation_gives_the_same_file_every_time` | Two independent sessions give the same bytes. |
| (Revision consistency, PREP-S07) | `a_slice_finished_after_an_edit_is_never_taken_for_the_plate` | A slice that finishes after an edit made while it ran is never current and cannot stand for the plate. The next slice reflects the edit. |
| Invalid geometry | `an_open_model_is_refused_for_hollowing_with_its_fixes` | An open cube imported without repairs: hollowing refuses it with an error that offers fixes. Nothing can be exported. |
| Closed cavities | `an_enclosed_cavity_slices_with_a_warning_never_a_plain_success` | A cavity with no hole slices, and the slice is reported with a warning, never as a plain success. |
| Insufficient clearance | `a_drain_hole_that_misses_the_cavity_leaves_it_enclosed_and_says_so` | A 1 mm hole 0.8 mm from the edge stays inside the wall. It is placed, and the summary says it reaches no cavity. The slice warns that the cavity is enclosed. |
| Support connection failures | `a_support_without_room_prints_with_a_warning_naming_it`, `supports_that_no_longer_reach_the_plate_refuse_the_slice_and_say_why` | A support whose tip is longer than the lift allows: it prints with a warning naming it. With the model lowered under all its supports, the slice is refused, and the refusal names each support that could not be built and why. |

## What the scenarios found and fixed

1. **A cavity that traps resin was reported as a plain success.** The slice summary said
   "Drainage: none; the cavity is enclosed…", but only inside the success message's detail. A
   finished slice now also shows the warning "A cavity has no drain hole opening it: it will trap
   uncured resin" whenever the summary says a cavity traps resin.
2. **A refused slice did not say why its supports failed.** When the model is above the plate
   and no support stands on it, the refusal only said so. It now lists each support that could
   not be built, with its finding (for example, the base join is not below the shaft).

## Evidence

**Linux (WSL2), 2026-10-30:** the 12 scenarios pass, in 7.4 s in a debug build.
`KARASU_SCENARIOS_OUT` kept the exports, and `tools/m5-validate/validate_workshop.py` read all 13
without error. The files that are equal by design are equal:

| Export | SHA-256 |
| --- | --- |
| combined, edited-2mm, edited-2mm-again, replayed-prepared, replayed-redone, reproduced, reproduced-b | `2e802a359d865456d593bbe1da2cb3d39eda297f4abee2e5b215f9cd5373e84c` |
| edited-1.5mm, stale-after | `a1d81c9d1294d96787104665890ff4e58f26f2c1ce8b131c4b1b2c3722fa19fd` |
| hollowed | `7ae68d9de31950be09e37644bbd349b9f063cd3fb5bafc826225d26dd0a06afe` |
| supported | `b71af513a86ece40c871bf7d9372bbac7870de19e1fb98cbbd0d45725cab4623` |
| replayed-plain, replayed-undone | `b60ad12e1f4685d4f78bde112383c224f22dd3c97bbdac58d94dc1a60a4667c5` |

**Native Windows, 2026-10-30:** passed. Snapshot of the working tree on 0c1181c, with these changes,
629 files, `validate.ps1 -IncludeApp`, rustc 1.99.0 MSVC, with no failed command:
- the Debug and Release workspace tests each pass 1,493, with 0 failed and 9 ignored; the 12
  scenarios pass in both;
- the three smoke runs pass: Vulkan 12.9 ms, OpenGL 13.0 ms, FX-03 16.5 ms mean frame time.

The scenarios were then run again in Release with `KARASU_SCENARIOS_OUT` (2.4 s). All 13
exports are byte-identical to Linux's, which were built in Debug.

**Owner's interactive run, native Windows, 2026-10-30:** every item passed, on
`HillGiant_32mm.stl` with a Photon P1 (PP1 exports).
- Items 1–5 behaved as expected.
- Item 6: the two exports of the same preparation are byte-identical (`dcda10e1…`). The export
  after setting the wall back equals the first export (`9300d0ec…`).
- Item 7:
  - The enclosed-cavity warning was shown. HillGiant's hollow has 16 cavity regions, and the holes
    opened only one, so every slice warned, as designed.
  - Lowered to 2 mm, only one branch lost its junction. The slice ended with a warning naming it.
  - `Base_01.stl` was not refused with repairs on opening off: its openings are zero-area cracks,
    which the plate closes by design.
  - A cube with one triangle removed was refused by Hollow ("The model cannot be prepared") and
    by Slice, and could not be exported. Its import shows only an Info diagnosis, since plain
    slicing closes small gaps per layer.

## Interactive checklist (owner)

Run this in the application on native Windows, and on Linux where a desktop is available. Use any
closed model; a cube or a miniature both work. For each item, record whether it passed, with a
screenshot if it did not.

1. **Hollow, drain, slice, export.**
   - Open the model.
   - Hollowing › Hollow: set a 2 mm wall and Apply.
   - Drain holes: place one hole on a side and Apply.
   - Export › Slice and export: Slice, then Export….
   - *Expected:* the slice succeeds with no warning; the section shows the cavity and the hole;
     the file is written.
2. **Supports.**
   - Lift the model 5 mm with Supports › Raft › Lift.
   - Supports: place three supports under it.
   - Select one and choose Heavy; select another and delete it.
   - Slice and export.
   - *Expected:* two supports in the slice summary; no warning.
3. **Combined.** Hollow, drain and support the same model, then slice and export.
4. **Edit and recompute.**
   - After a slice, change the wall.
   - *Expected:* the slice shows as out of date, and Export… is disabled until you slice again.
   - Slice again, then set the old wall back and slice.
   - *Expected:* the result matches the first slice (same layers, volume and time).
5. **Undo and redo.** Undo every step back to the plain model and slice; then redo every step and
   slice. *Expected:* the facts match the plain slice, then the prepared one.
6. **Reproducible.**
   - Close the application, open the same model, and repeat item 3 with the same values.
   - Export to another file.
   - *Expected:* `Get-FileHash` gives the same SHA-256 for both exports.
7. **Failures.**
   - Hollow a model with no drain hole and slice. *Expected:* a warning that the cavity traps
     resin.
   - Lower a supported model onto its supports (Raft › Lift to 2 mm) and slice. *Expected:*
     each support that no longer fits is named in Messages. If others still stand, the slice
     ends with a warning; if none stands on the plate, the slice is refused.
   - In Settings, set the repair on opening to "None: diagnose only". Open a model with a
     missing face, such as a cube with one triangle removed, and hollow it.
     *Expected:* an error that offers repair fixes. Cracks of zero area, as in `Base_01.stl`, are
     not such holes: the plate closes them by design (TD-092), and the model slices.

Physical printing is out of scope: none of this proves that a cavity drains or that supports
hold.
