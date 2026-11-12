# Support test plate

`make_support_test.py` writes the test plate of the
[support presets research](../../docs/research/SUPPORT-PRESETS-RESEARCH.md) as a karasu project:
eighteen numbered tiles, each with one support combination, and a record sheet to fill in after
the print.

```console
python3 -I tools/support-test/make_support_test.py <template.karasu> support_test.karasu sheet.md
karasu-cli slice --project support_test.karasu --output support_test.pp1
```

The template is any project whose first plate has the printer, resin and print setting to print
with; its plate settings are copied too, its objects are not. The script needs no package
beyond Python's standard library and writes models and supports by hand: every tile is a closed
box and its digits separate closed boxes standing on its top.

**The tiles** (15 × 15 × 2 mm, lifted 8 mm over a raft, 26 mm apart, in three rows):
- 1 to 12, grid A: contact 0.3, 0.4, 0.5 and 0.6 mm (columns) against depth 0.1, 0.2 and 0.3 mm
  (rows, 0.1 mm at the back), on Medium otherwise; 36 supports 2.5 mm apart to the raft.
- 13 to 18, grid B, the two right-hand columns: Medium's contact braced (13), 3.5 mm apart (14),
  3.5 mm apart and braced (15), branching from trunks (16); Heavy branching and braced as the
  venom torso v4 (17), Heavy alone (18).

**The second plate** (`--round 2`, after the first's print), twenty tiles in three rows of
seven: ten combinations, tiles 1 to 10 and again 11 to 20 elsewhere on the plate. The contact's
width at the surface about the first plate's threshold (0.45, 0.48 and 0.52 mm, sunk 0.1 mm),
0.5 mm sunk 0.2 mm, Heavy's 0.6 mm contact sunk 0.1 mm on a 1.0 mm and on its own 1.4 mm body,
16 wide contacts 4.7 mm apart (a 3.5 mm spacing spread from edge to edge), sparse braces, and
tiles tilted by 30 degrees with 0.5 mm and with the Medium of the time. Every tile's tips run from edge to edge, the outer ones 0.4 mm in, and a
tilted tile's lowest edge, which prints first, stands on tips 0.2 mm in.

Plates generated now follow today's presets; which commits generated the printed plates, and how
the presets changed since, is recorded in
[docs/TOOL-RECORDS.md](../../docs/TOOL-RECORDS.md#support-test-plate-the-printed-plates).
