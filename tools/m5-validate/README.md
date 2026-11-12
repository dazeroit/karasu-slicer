# Independent M5 protocol validator

Run `python3 -m unittest discover -s tools/m5-validate -v` for public-layout
vectors and corruption checks. Inspect a generated output with
`python3 tools/m5-validate/validate_machine.py output.goo` (also CTB V2 plain
and Photon V1). The script imports no Karasu helpers and implements a separate
struct-based reader, written independently of karasu-slicer's writers.

This is a bounded development check for known small trusted test outputs,
not a general-purpose untrusted file reader. It verifies field bounds, RLE
pixel counts, zero encryption seed, and GOO checksum/framing/footer. It decodes GOO RGB565
thumbnails and CTB/Photon RLE15 previews (descriptor sizes, run markers, exact pixel counts) and
summarizes them; `validate_with_previews(path, decode_layers=False)` returns the decoded images
quickly for visual inspection. `validate_machine.py` also reads plain CTB v3, v4 and v5 (TD-063): `LayerDefEx` against the
table row, address pages, notice, v4 block and v5 resin strings; encrypted CTB is rejected.
`validate_workshop.py` reads the Anycubic Photon Workshop family (revisions 1–518, PW0 or `.pws`
bit-planes) and checks every section, record and lit-pixel count. `validate_sl1.py` reads Prusa
SL1/SL1S archives: members, sorted `config.ini`, grayscale layer PNGs, RGBA thumbnails and the
layer-count and material totals.
`validate_reference.py` checks an optional `minimum_gray` (TD-060) and that no nonzero
pixel lies more than half a level below it. `validate_pp1.py` reports each preview PNG's size and channel
count (3 for rendered previews, 1 for the gray projection). It emits
small masks in JSON for comparison. The check does not establish physical
printer acceptance, calibration, firmware-specific orientation, or encryption
support. CTB V2 plain is deliberately unencrypted and has no broad firmware
compatibility claim.
