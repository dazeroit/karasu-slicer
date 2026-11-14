# Panel orientation check

Standard-library Python scripts that check each catalog printer's panel orientation against the
file its manufacturer's slicer writes for the same model. They read files and write only the test
model and karasu-slicer's exports. They extend `tools/goo-orientation` (TD-104, TD-143) to every
format the catalog's printers use; TD-153 records the first complete comparison.

- `make_f.py [OUT.stl]` writes the test model: an "F" 20 × 30 mm and 2 mm thick, which reads
  correctly seen from above (+X right, +Y toward the plate's back). The letter is chiral, so
  each flip or turn of the panel gives a different image. It is the same "F" that the printer
  profile's orientation choice shows.
- `export_karasu.py MODEL.stl OUT_DIR [KARASU_CLI]` exports the model from karasu-cli for every
  printer of the built-in catalog, centred and on the plate, named after the printer id
  (`elegoo-mars-5.goo`).
- `compare.py REFERENCE CANDIDATE [LAYER]` compares two files of the same model and printer;
  `compare.py --dirs REFERENCE_DIR CANDIDATE_DIR` pairs files by name and prints a Markdown table.
- `print_files.py` reads GOO V3, the Anycubic Photon Workshop family (PW0 or bit-plane images),
  PP1, the original Photon's `.photon` and plain CTB, whatever slicer wrote them. It takes one
  layer's pixels and what the file declares about its stored image (GOO mirror bytes, PP1
  `rotate_z`) and checks nothing else.

## Procedure

1. Write the model: `python3 -I tools/orientation-check/make_f.py F_orientation_test.stl`.
2. In the manufacturer's slicer (ELEGOO SatelLite, Anycubic Photon Workshop), for each printer:
   import the model without rotating it, leave it centred on the plate, add no supports, raft or
   hollowing, use 0.05 mm layers and no anti-aliasing where the slicer allows, and export the
   printer's file named after the printer id (`elegoo-mars-5.goo`). Note the slicer's version.
3. Export the same model from karasu-slicer:
   `python3 -I tools/orientation-check/export_karasu.py F_orientation_test.stl karasu/`.
4. Compare: `python3 -I tools/orientation-check/compare.py --dirs official/ karasu/`.

Keep the manufacturers' files outside the repository; record their slicer, version and SHA-256.

## Reading the result

One layer of each file becomes a silhouette (cells of a few pixels, each row filled between its
first and last lit cell). The candidate's, as stored, mirrored along columns (X), mirrored along
rows (Y) and turned 180 degrees, is compared with the reference's by intersection over union, in
place on the panel and with both centred on their centroids; each flip scores the better of the
two, and the verdict names the flip that scores at least 0.85 and 0.2 above the next.

- **same image**: karasu-slicer stores the manufacturer's image.
- **turned 180 degrees, same handedness**: the part keeps its handedness but faces the other side
  of the plate.
- **same image on an exchanged panel**: the part's image agrees, but the two files declare the
  panel with its sides exchanged (1440 × 2560 against 2560 × 1440); the profile's resolution and
  plate X and Y are to be exchanged.
- **turned 90 degrees (transposed, …)**: the image is transposed and flipped; the profile needs
  `swap_xy` and the mirror named.
- **MIRRORED**: the part would print mirrored; the profile is wrong.
- **unclear**: no flip stands out; check that both files hold the same model.

Files the readers name but do not read: GOO versions other than 3.0, PP1 layers stored as outlines
(`pwszImg`) and encrypted CTB, whose encryption is not bypassed.

A printer's profile is aligned with its manufacturer's slicer exactly (owner, 2026-11-14,
TD-153): the same stored image and the same declaration (GOO mirror bytes, PP1 `rotate_z`), so
that its files print as the manufacturer's do however the firmware reads the declaration.

Run the tests with `python3 -m unittest discover -s tools/orientation-check -p 'test_*.py'`.
