#!/usr/bin/env python3
"""Extract the manufacturers' resin settings tables into the catalog generator's data files.

The two tables come from files downloaded by hand, which stay outside the repository:

- ELEGOO's settings spreadsheet (catalog source E2), an .xlsx file;
- Anycubic's resin settings page (catalog source A8), saved as .html.

    python3 -I tools/assets/extract_resin_tables.py --elegoo E2.xlsx --anycubic A8.html

It writes tools/assets/catalog-data/elegoo-e2.tsv and anycubic-a8.tsv, with every value as
published (ranges, two-stage values and "/" included), for the catalog's printers only. The
generator (make_catalog.py) reads those files and records which values it takes. The
spreadsheet's rows keep their row numbers so that each value can be found in the original.
Only the standard library is used; nothing in the downloaded files is executed.
"""
import argparse
import hashlib
import re
import sys
import zipfile
from html.parser import HTMLParser
from pathlib import Path
from xml.etree import ElementTree

DATA = Path(__file__).resolve().parent / "catalog-data"
SHEET_NS = {"s": "http://schemas.openxmlformats.org/spreadsheetml/2006/main"}

# ELEGOO's machine entries for the catalog printers. The Mars 4 Ultra and Saturn 3 Ultra also
# have fast-printing entries, which the catalog leaves out (owner, 2026-11-11).
ELEGOO_MACHINES = {
    "Mars 4": "elegoo-mars-4",
    "Mars 4 MAX": "elegoo-mars-4-max",
    "Mars 4 Ultra（normal resin+normal speed）": "elegoo-mars-4-ultra",
    "Mars 5": "elegoo-mars-5",
    "Mars 5 Ultra": "elegoo-mars-5-ultra",
    "Saturn 3": "elegoo-saturn-3",
    "Saturn 3 Ultra（normal resin+normal speed）": "elegoo-saturn-3-ultra",
    "Saturn 4": "elegoo-saturn-4",
    "Saturn 4 Ultra": "elegoo-saturn-4-ultra",
    "Saturn 4 Ultra 16K": "elegoo-saturn-4-ultra-16k",
    "Jupiter 2": "elegoo-jupiter-2",
    "Jupiter SE": "elegoo-jupiter-se",
}

ELEGOO_COLUMNS = ["row", "printer", "resin", "colour", "bottom_exposure_s", "exposure_s",
                  "layer_mm", "lift_mm", "lift_mm_min", "retract_mm_min", "rest_s"]

ANYCUBIC_FIELDS = ["Layer Thickness", "Exposure Time", "Light-Off Time", "Bottom Exposure Time",
                   "Bottom Layer", "Z Lift Distance", "Z Lift Speed", "Z Retract Speed",
                   "Anti-Aliasing Level"]

ANYCUBIC_COLUMNS = ["printer", "resin", "layer", "exposure", "light_off", "bottom_exposure",
                    "bottom_layers", "lift", "lift_speed", "retract_speed", "antialias"]


def clean(text):
    """Text with its runs of white space (no-break spaces included) made single spaces."""
    return " ".join(text.split())


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def xlsx_rows(path):
    """The first sheet's rows as {row number: [cell texts]}."""
    with zipfile.ZipFile(path) as book:
        shared = [
            "".join(t.text or "" for t in si.iter(f"{{{SHEET_NS['s']}}}t"))
            for si in ElementTree.fromstring(book.read("xl/sharedStrings.xml")).findall("s:si", SHEET_NS)
        ]
        sheet = ElementTree.fromstring(book.read("xl/worksheets/sheet1.xml"))
    rows = {}
    for row in sheet.iter(f"{{{SHEET_NS['s']}}}row"):
        cells = {}
        for cell in row.findall("s:c", SHEET_NS):
            column = re.match(r"[A-Z]+", cell.get("r")).group(0)
            value = cell.find("s:v", SHEET_NS)
            if value is None:
                inline = cell.find("s:is", SHEET_NS)
                text = "".join(t.text or "" for t in inline.iter()) if inline is not None else ""
            elif cell.get("t") == "s":
                text = shared[int(value.text)]
            else:
                text = value.text or ""
            cells[column] = text
        width = max((ord(c[-1]) - 64 for c in cells), default=0)
        rows[int(row.get("r"))] = [cells.get(chr(65 + i), "") for i in range(width)]
    return rows


def elegoo(path):
    rows = xlsx_rows(path)
    header = [clean(c) for c in rows[2]]
    if header[:4] != ["Machine Type", "Resin Type", "Notes", "Color"]:
        sys.exit(f"unexpected ELEGOO header: {header}")
    # Merged cells hold the machine and resin only in their first row.
    machine = resin = ""
    out = []
    for number in sorted(n for n in rows if n > 2):
        cells = [clean(c) for c in rows[number]] + [""] * 11
        machine = cells[0] or machine
        resin = cells[1] or resin
        printer = ELEGOO_MACHINES.get(machine)
        if printer is None or not cells[3]:
            continue
        out.append([str(number), printer, resin, *cells[3:11]])
    return out


class Tables(HTMLParser):
    """The page's headings and tables, in order."""

    def __init__(self):
        super().__init__()
        self.items, self.depth, self.row, self.cell, self.heading = [], 0, None, None, None

    def handle_starttag(self, tag, attrs):
        if tag == "table":
            self.depth += 1
            self.items.append(("table", []))
        elif tag == "tr" and self.depth:
            self.row = []
        elif tag in ("td", "th") and self.depth:
            self.cell = []
        elif tag in ("h1", "h2", "h3", "h4") and not self.depth:
            self.heading = []

    def handle_endtag(self, tag):
        if tag == "table":
            self.depth -= 1
        elif tag == "tr" and self.row is not None:
            self.items[-1][1].append(self.row)
            self.row = None
        elif tag in ("td", "th") and self.cell is not None and self.row is not None:
            self.row.append(clean("".join(self.cell)))
            self.cell = None
        elif tag in ("h1", "h2", "h3", "h4") and self.heading is not None:
            self.items.append(("heading", clean("".join(self.heading))))
            self.heading = None

    def handle_data(self, data):
        if self.cell is not None:
            self.cell.append(data)
        elif self.heading is not None:
            self.heading.append(data)


def anycubic(path, printers):
    parser = Tables()
    parser.feed(path.read_text(encoding="utf-8", errors="replace"))
    out, printer = [], None
    for kind, item in parser.items:
        if kind == "heading":
            match = re.match(r"Anycubic Resin Settings for (.+)", item)
            printer = match and "anycubic-" + match.group(1).strip().lower().replace(" ", "-")
            continue
        if printer not in printers or not item or item[0][0] != "Resin Type":
            continue
        resins = item[0][1:]
        values = {row[0]: row[1:] for row in item[1:]}
        missing = [f for f in ANYCUBIC_FIELDS if f not in values]
        if missing:
            sys.exit(f"{printer}: no {missing} in Anycubic's table")
        for i, resin in enumerate(resins):
            out.append([printer, resin] + [values[f][i] for f in ANYCUBIC_FIELDS])
        printer = None
    return out


def write(name, columns, rows, source, digest):
    lines = [f"# {source}, SHA-256 {digest}; extracted by tools/assets/extract_resin_tables.py",
             "\t".join(columns)] + ["\t".join(r) for r in rows]
    (DATA / name).write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"{name}: {len(rows)} rows")


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--elegoo", type=Path, required=True, help="ELEGOO's spreadsheet (E2)")
    parser.add_argument("--anycubic", type=Path, required=True, help="Anycubic's page (A8)")
    args = parser.parse_args()
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from make_catalog import PRINTERS  # noqa: E402 (the catalog's own printer list)
    printers = {f"{vendor}-{slug}" for vendor, slug, *_ in PRINTERS}
    DATA.mkdir(exist_ok=True)
    write("elegoo-e2.tsv", ELEGOO_COLUMNS, elegoo(args.elegoo),
          "ELEGOO's settings spreadsheet (E2)", sha256(args.elegoo))
    write("anycubic-a8.tsv", ANYCUBIC_COLUMNS, anycubic(args.anycubic, printers),
          "Anycubic's resin settings page (A8) as saved", sha256(args.anycubic))


if __name__ == "__main__":
    main()
