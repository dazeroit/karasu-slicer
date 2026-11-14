"""Export a model from karasu-cli for every printer of the built-in catalog, in each of its formats.

usage: export_karasu.py MODEL.stl OUT_DIR [KARASU_CLI]

Each file is named after the printer id (`elegoo-mars-5.goo`), followed by `--` and the format id
when the printer has more than one format (`anycubic-photon--anycubic-pws-v1.pws`). The model is
centred on the plate and dropped onto it, with the printer's first catalog print setting at
0.05 mm layers (or its first setting). A printer without a print setting (no resin table, such as
the original Photon) borrows the Photon S's first setting under its own id, with one-stage motion
and no separate rests so that every legacy format can carry it: the panel's orientation depends on
the printer profile alone. KARASU_CLI defaults to the workspace's release build.
"""
import glob
import json
import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
CATALOG = REPO / 'assets' / 'catalog'


def extension(format_id):
    if format_id == 'goo-v3':
        return 'goo'
    if format_id == 'photon-v1-firmware-motion':
        return 'photon'
    return re.fullmatch(r'anycubic-([a-z0-9]+)-v\d+', format_id).group(1)


def borrowed_setting(printer_id, out):
    donors = sorted((CATALOG / 'print-settings' / 'anycubic' / 'photon-s').glob('*.json'))
    donor = json.loads(donors[0].read_text())
    donor['profile']['printer_id'] = printer_id
    donor['profile']['id'] = printer_id + '-orientation-check'
    for stage in ('bottom', 'normal'):
        st = donor['profile']['settings'][stage]
        st['wait_before_exposure_s'] = st['wait_after_exposure_s'] = st['wait_after_lift_s'] = 0.0
        st['lift'], st['retract'] = st['lift'][:1], st['retract'][:1]
    path = out / f'{printer_id}.setting.json'
    path.write_text(json.dumps(donor))
    return path


def main(argv):
    if len(argv) not in (2, 3):
        print(__doc__)
        return 2
    model, out = Path(argv[0]), Path(argv[1])
    cli = argv[2] if len(argv) == 3 else str(REPO / 'target' / 'release' / 'karasu-cli')
    out.mkdir(parents=True, exist_ok=True)
    materials = {json.loads(Path(f).read_text())['profile']['id']: f
                 for f in glob.glob(str(CATALOG / 'materials' / '*' / '*.json'))}
    failed = 0
    for printer_file in sorted((CATALOG / 'printers').glob('*/*.json')):
        printer = json.loads(printer_file.read_text())['profile']
        vendor, name = printer_file.parent.name, printer_file.stem
        settings = sorted((CATALOG / 'print-settings' / vendor / name).glob('*.json'))
        if not settings:
            settings = [borrowed_setting(printer['id'], out)]
        heights = {s: json.loads(s.read_text())['profile']['settings']['layer_height_mm']
                   for s in settings}
        setting = next((s for s in settings if heights[s] == 0.05), settings[0])
        resin = materials.get(json.loads(setting.read_text())['profile']['resin_id'])
        formats = printer['format_ids']
        for format_id in formats:
            suffix = '' if len(formats) == 1 else '--' + format_id
            dest = out / f"{printer['id']}{suffix}.{extension(format_id)}"
            command = [cli, 'slice', '--assume-mm', str(model), '--printer', str(printer_file),
                       '--print-setting', str(setting), '--format', format_id, '--center',
                       '--drop', '--output', str(dest)]
            if resin:
                command[6:6] = ['--resin', resin]
            result = subprocess.run(command, capture_output=True, text=True)
            status = 'ok' if result.returncode == 0 else (
                'FAILED: ' + (result.stderr.strip().splitlines() or ['?'])[-1])
            failed += result.returncode != 0
            print(f"{printer['id']:34s} {format_id:28s} {heights[setting]:.3f} mm  {status}")
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
