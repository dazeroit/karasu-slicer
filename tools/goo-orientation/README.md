# GOO orientation check

Standard-library Python scripts that compare the layer images of two GOO V3 files, to check a
printer profile's panel orientation against a reference slicer's file for the same model
(TD-104, TD-143). They read files; they write nothing but the PNG asked of `goo_layer.py`.

- `goo_header.py FILE.goo...` prints each file's header: the writing software, the printer,
  the anti-aliasing, the layer count, the panel's pixels and size, and the mirroring the
  header declares for the stored image.
- `goo_layer.py FILE.goo LAYER OUT.png [SCALE]` writes one layer's lit region as stored, as a
  grey PNG reduced `SCALE` times (row 0 is the stored image's first row).
- `goo_compare.py REFERENCE.goo CANDIDATE.goo LAYER...` gives, for each layer, the overlap
  (intersection over union of lit 4 × 4 cells) of the candidate's stored image with the
  reference's, as stored, mirrored along columns or rows, and turned 180 degrees.

Export the reference's model with karasu-slicer at the same place (a centred model with
`--center --drop`), the same layer height and the profile to check, then compare layers where
the model is not symmetric:

```console
karasu-cli slice --assume-mm ROOK.stl --printer printer.json --resin resin.json \
  --print-setting setting.json --format goo-v3 --center --drop --output karasu.goo
python3 -I tools/goo-orientation/goo_header.py ROOK.goo karasu.goo
python3 -I tools/goo-orientation/goo_compare.py ROOK.goo karasu.goo 100 400 600 900
```

A profile matches when "as stored" is near 1 on every compared layer. Keep reference files
outside the repository: they are the manufacturers', not karasu-slicer's.
