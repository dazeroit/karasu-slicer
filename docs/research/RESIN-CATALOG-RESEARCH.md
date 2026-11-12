# Resin catalog research: Anycubic, ELEGOO and SUNLU

Research of 2026-11-11 for widening the built-in catalog's resins (TD-146). Only the resin makers'
own sources were read: their websites, wikis, download hosts, PDFs and spreadsheets. No forum,
third-party profile or slicer software was used, and no manufacturer software was run. The
catalog's generated provenance page is [CATALOG-SOURCES.md](../CATALOG-SOURCES.md); the earlier
printer-profile research is [PRINTER-PROFILE-SOURCES-RESEARCH.md](PRINTER-PROFILE-SOURCES-RESEARCH.md).

Labels: **observed** means read in the source as cited; **inferred** means concluded from
observations, not stated by the source.

## 1. Owner decisions (2026-11-11)

1. One catalog resin per **product**, not per colour. Where a product's colours have different
   settings, the entry names the colour whose settings it holds (TD-118's colour field).
2. **SUNLU:** materials only, without print settings.
3. **Densities** in the catalog: the manufacturer's single published value.
4. **ELEGOO's fast-printing modes** (0.1 mm, the Mars 4 Ultra and Saturn 3 Ultra) are left out,
   and with them the Rapid Standard resin, which has rows only there.
5. Adding a printer from the catalog offers its resins as a checklist, with only the vendor's
   standard resin ticked.

Proposed and not objected to: Anycubic's resin page (A8) is the source of Anycubic settings,
marked "official"; its product pages are recorded where they disagree; legacy resins (no
longer sold) stay out; the Photon P1's Standard Resin keeps Photon Workshop's default (A11).

## 2. Sources

Catalog source keys (A8, E2, ...) are those of CATALOG-SOURCES.md.

### Anycubic

| Key | Source | Read |
| --- | --- | --- |
| A8 | <https://store.anycubic.com/blogs/news/resin-settings-for-anycubic-3d-printers>, 26 printer tables (HTML), saved page SHA-256 `6612ccc586fbad00bae00f94408fa05a3ce17fac9a312e670989d5e61a6a0725` | live, 2026-11-11 |
| A27 | <https://store.anycubic.com/collections/uv-resin/products.json>: the 16 resins sold, their colours and "1KG" bottles | live, 2026-11-11 |
| A28 | <https://store.anycubic.com/products/colored-uv-resin> (Standard Resin): density 1.13 g/cm³ | Internet Archive capture of 2026-10-14 |
| — | Product pages of the other 15 resins (`store.anycubic.com/products/...`, `eu.anycubic.com`, `anycubic.au`), each with a table of 16 printers | Internet Archive captures, 2025-06-23 to 2026-11-02 |
| — | Resin manuals V1.0 on Anycubic's CDN, tables as images: Tough (SHA-256 `3051a07d…88002`), ABS-Like+ (`dd9eed88…75b66`), Standard (`7a619c84…df1d4f`), Standard+ (`50e15f87…dddc4`), DLP Craftsman (`e5fb292e…66f2`), Water-Wash+ (`66da298a…a44c`) | 2026-11-11 |
| — | Technical data sheets linked from the product pages: generic ranges and physical data only | 2026-11-11 |

The store answered HTTP 429 to every request after the first few on 2026-11-11 and still did
after fifteen minutes; the product pages were therefore read from Internet Archive captures of
the official pages, as they stood at capture time. A8 and A27 were read live.

### ELEGOO

| Key | Source | Read |
| --- | --- | --- |
| E2 | The settings spreadsheet linked from <https://www.elegoo.com/blogs/3d-printer-user-guide/official-suggestions-for-elegoo-resin-on-elegoo-printers>, SHA-256 `7f67e7dc1c540981e9f9927bb54d7185c5deb0086e97fac51013e8dac5e97472`, unchanged since the catalog first used it: 2998 rows, 25 machine entries, 24 resins | 2026-11-11 |
| E10 | <https://wiki.elegoo.com/resin-settings>: the same table, identical row for row | 2026-11-11 |
| E3 | The printers' manuals (2026-09-03 versions where published): bottom layers and waits as printer defaults | 2026-11-11 |
| E9 | <https://wiki.elegoo.com/filaments-resins/resins/material-safety-data> and its 24 safety data sheets (dated 2026-01-07; ABS-Like Ultra 2026-03-11), section 9 read from page images | 2026-11-11 |
| — | <https://wiki.elegoo.com/filaments-resins/resins/faq>: "approximately 1.05–1.25 g/cm³", per-product data sheets not linked | 2026-11-11 |

ELEGOO's store pages answered HTTP 429 all day; colours sold and bottle sizes were not read.

### SUNLU

| Key | Source | Read |
| --- | --- | --- |
| SU1 | <https://www.sunlu.com/collections/2>: the 16 resins sold | 2026-11-11 |
| — | <https://www.sunlu.com/wiki/> resin pages and the product pages `https://www.sunlu.com/products/...`: ranges by screen type | 2026-11-11 |
| SU2 | Technical data sheets on `media.sunlu.com`, linked from the product pages (table below) | 2026-11-11 |

| Resin | Data sheet | SHA-256 | Liquid density (g/cm³) |
| --- | --- | --- | --- |
| Standard | `prod/20260331/69c33c9d-aec2-4e42-9d22-fede8a5215dd.pdf` | `643fae88…befc4f51` | not used (note 1) |
| Standard Plus | `prod/20260331/c61e9f62-bd8c-44fa-bd18-575a494a710a.pdf` | `bb0a0384…bd6cc` | 1.0900 ± 0.05 |
| ABS-Like | `prod/20260331/cdd6ab82-85d8-49ea-b432-12c6c9c5c005.pdf` | `f8ac1496…af67ea` | 1.0900 ± 0.05 |
| PA-Like ("Nylon-Like Resin") | `prod/20260331/32015e60-1bd7-47a1-8faf-8edcdcc3ec8f.pdf` | `bb20bc8a…a5f2` | 1.0188 ± 0.05 |
| Toughness (titled "High Temperature Resin", note 2) | `prod/20260331/dff54fde-adf4-4eda-902e-a6cf68a3426e.pdf` | `b3b2628e…21299` | 1.1300 ± 0.03 |
| Flexible | `prod/20260331/b22aa34c-9352-48a3-8f4f-15592dc25457.pdf` | `50366034…af96f` | 1.0742 ± 0.05 |
| PC-Like | `prod/20260916/70279771789538816956.pdf` | `95fc5ba6…6e1` | 1.0198 |
| 14K Standard | `prod/20260331/762ab522-c852-4b0f-8ed9-2b34b3cf6846.pdf` | `30c7cfa8…bfda01b` | 1.1300 ± 0.05 |
| 14K Red Wax | `prod/20260331/0bebe2d6-909e-41f6-a3ae-ef60fb4f3945.pdf` | `bd56f4d0…db8a0303` | 1.1300 ± 0.05 |
| 14K ABS-Like | `prod/20260331/b92b28f7-30fe-4621-92de-6e312652f5a7.pdf` | `355585c4…a7847` | 1.1214 ± 0.05 |
| Water-Wash Standard | `prod/20260331/3856d905-7b14-426e-90f7-0f7017cca4d9.pdf` | `1db3038f…18a6b` | 1.1200 ± 0.02 (printed "1. 1200") |
| Water-Wash ABS-Like | `prod/20260331/8db4c781-c713-4784-a413-b9c81decb46e.pdf` | `ed4d0428…b132c` | 1.1200 ± 0.02 (printed "1. 1200") |
| High Clear | `prod/20260422/844e2397-a18b-4d40-9b69-876e23ba961e.pdf` | `228968f2…2f3dc1` | 1.1214 ± 0.05 |
| CMYK+W Color | `prod/20260331/ae83073e-8d94-4380-9fdf-cb22f584c64e.pdf` | `30877344…a7ba1` | 1.1137 ± 0.05 |
| High Temperature Plus | `prod/20260331/a1d6f5af-9b5d-4bcf-89a2-1ad2727adede.pdf` | `7e299078…d8f80` | 1.235 ± 0.05 |
| Dental Model (titled "Dental Implant Resin", note 3) | `prod/20260331/6e323eee-29b9-4cc2-a545-1adce6e007c4.pdf` | `508f95b5…b2a66b` | not used (note 3) |

Full SHA-256 values: Standard `643fae88fab2eb86b5fc77fc7de20f5395193616e3d76e80c8f21b41befc4f51`,
Standard Plus `bb0a038420da2b1452db9e874b76b8bb51e7a4df902640ab641d2660eb3bd6cc`, ABS-Like
`f8ac14968fe6ef73a0723abb2cef84d82ea5a9887e9773e6446a366cb9af67ea`, PA-Like
`bb20bc8a0a814e286adf20da296a561a755b642a9709d2e0ea7247934225a5f2`, Toughness
`b3b2628ecee4a5e8b8822e41e5172108a0ed71777b1c3a118f197849e0521299`, Flexible
`50366034415fcf07df6ab9185585197f2e1c52c8a938a535eb1cafe48c9af96f`, PC-Like
`95fc5ba6cf8ad31518fb712c9f7b51e4608a8c426fb3a573432065654c6be6e1`, 14K Standard
`30c7cfa8776c16e54eb1043b22e5432a24171511c4493fb134050f244bfda01b`, 14K Red Wax
`bd56f4d0cabb36b5eb366d9ed240806cb57878b1cdc96173fd2771c9db8a0303`, 14K ABS-Like
`355585c46cca0f8f1c9e2f030e127e8b28c88a5153c17dc99910aa1fba6a7847`, Water-Wash Standard
`1db3038f49ec21ef65d32395a2f4a12ebd978b970b093d2a3c14a1b787c18a6b`, Water-Wash ABS-Like
`ed4d042872f12d43b8226ad6a413943fef5fb524f5e27708400db6f4dacb132c`, High Clear
`228968f25e9415e04a7df94f65b843785721456801216574558bd69e682f3dc1`, CMYK+W
`30877344ef717c6365845554f2dd2e206d21ab68f5c3c41960816e046b9ba7a1`, High Temperature Plus
`7e2990789abb8febc257e4c4e99bb65714bceeb30f96faada327c9d2a24f8d80`, Dental Model
`508f95b5dfcaaa1db7817074efd5a4d1c0afaf24e13c592a1ea3fe1318a2b66b`.

Notes:
1. Observed: the sheet linked from the Standard Resin's page is titled "Standard Resin", but its
   text says "Standard PLUS Photopolymer Resin" and its values are the Standard Plus sheet's
   (tensile strength 31 MPa, HDT 52 °C, where the Standard's page gives 45 MPa and 55 °C). Its
   1.09 g/cm³ is therefore not taken for the Standard Resin.
2. Observed: titled "High Temperature Resin", with the Toughness Resin's mechanical values; taken.
3. Observed: titled "Dental Implant Resin", with other mechanical values than the Dental Model
   Resin's page (tensile strength 25 against 45 MPa, Shore 75 against 83 D). Inferred: possibly
   another formulation. Not taken.

store.sunlu.com answered HTTP 429 to collection and product requests; sunlu.com answered.

## 3. What each maker publishes (observed)

**Anycubic.** One table per printer on A8, 16 or 17 resin columns each, single values: layer,
exposure, light-off time, bottom exposure, bottom layers, one Z lift distance, lift and retract
speeds (mm/s), anti-aliasing level. No transition layers anywhere. The seven newest tables (P1,
P1 Max, M7, M7 Pro, M7 Max, Mono 4, Mono 4 Ultra) use the current resin names; the older ones the
older names (Standard Resin+, Water-Wash Resin+, Plant-Based Resin, ...), with a few current
resins (Standard V2, ABS-Like Pro 2, Tough 2.0, Tough Ultra, High Clear, DLP Craftsman). The P1
table is new since the catalog's printer research. The product pages give one table per resin,
with 16 printers, and the density, mostly as a range. Only the 16K Standard Resin varies by
colour (Orange Red against the others). Anycubic publishes nothing for other makers' printers.
The first Photon and the Photon Zero have no table in any source.

**ELEGOO.** One table for every resin on every ELEGOO printer, per colour: bottom and normal
exposure (mostly ranges), layer height, two-stage lift (mm, from the plate up), lift and
retract speeds (mm/min), rest after retract. No bottom layers, transition layers or waits: the
manuals give those as printer defaults. The tilting printers (Mars 5 Ultra, Saturn 4 Ultra, 16K)
have "/" for Z motion. Every safety data sheet gives "Relative density 1.05 g/cm³ (25 ℃)"; no
per-product technical sheet was found. ELEGOO publishes nothing for other makers' printers.

**SUNLU.** No setting names a printer. Each resin has wide ranges split only by screen type
(colour or monochrome LCD) at 0.05 mm, for example a monochrome bottom exposure of 10–60 s; the
sources often disagree with each other (section 5). Each data sheet gives a liquid density as a
value with a tolerance. SUNLU's printer presets page is for filament only.

## 4. How the catalog takes them

Recorded in TD-146; in short:

- `tools/assets/extract_resin_tables.py` reads the saved A8 page and the E2 spreadsheet (both kept
  outside the repository, with the other downloaded files and the research notes, in
  `<evidence>/m6/resin-catalog-2026-11-11/`, listed with their SHA-256
  in its `SHA256SUMS`) and writes `tools/assets/catalog-data/anycubic-a8.tsv` (322 rows) and
  `elegoo-e2.tsv` (1630 rows, the catalog printers' normal modes, every colour) with every value
  as published. Both were checked against an independent conversion made during the research:
  no difference.
- `tools/assets/make_catalog.py` takes from them, writes the catalog and says in
  CATALOG-SOURCES.md what it took, what it left out and which colours differ.
- Result: 55 resins (16 Anycubic, 23 ELEGOO, 16 SUNLU) and 389 print settings (141 Anycubic
  "official", 248 ELEGOO of which 1 "official"), from 2 resins and 31 settings.

## 5. Conflicts and anomalies

### Anycubic: A8 itself (observed)

- The D2 table equals the M3 Plus table except anti-aliasing.
- The Mono X's anti-aliasing row equals the Photon Ultra's, "/" cells included where the Mono X's
  other rows have values; the V1.0 manuals give the Mono X anti-aliasing 1. The catalog uses 1.
- The M5s and M5s Pro "Standard Resin V2" columns give 0.1 mm layers with the exposures of their
  0.05 mm Standard Resin columns; the M5's gives "/" for the layer. Inferred: a copy error. The
  catalog leaves these three out.
- The Mono X2 and Mono X 6Ks High Clear exposure reads "4" with no unit (seconds, as the column).
- The Mono's High Clear anti-aliasing reads "1/". The catalog uses 1.
- A8 gives the Mono 4K 1 bottom layer in every column, where the product pages give 4 to 10 and
  every V1.0 manual 6; A8 gives the Mono 4 a 4 mm/s retract in every new column, where most
  product pages give 6 mm/s.

### Anycubic: product pages against A8, for the resins sold and the catalog printers

Each line: resin, printer: field, product page value against A8 value. Pages read in Internet
Archive captures (section 2); "EU" is eu.anycubic.com, which differs from the US page for
ABS-Like Resin Pro 2.

- Standard Resin: M5, M5s: lift speed 5 against 6 mm/s. Mono 4: retract 6 against 4 mm/s.
  Mono 4K: bottom layers 4 against 1. Mono 2: bottom 20 against 25 s. Mono X2: bottom layers
  4 against 5. Mono X 6Ks: lift 3 against 2 mm/s, retract 4 against 3 mm/s. M3 Max: light-off 2
  against 2.5 s, bottom 25 against 50 s. M3 Premium: bottom layers 4 against 2.
- Standard Resin V2: M5: layer 0.05 mm against "/". M5s, M5s Pro: layer 0.05 against 0.1 mm.
  Mono 4: retract 6 against 4 mm/s. Mono 4K: bottom layers 6 against 1.
- 16K Standard Resin (both colour groups): Mono 4: exposure 2.8 against 2.5 s, retract 6 against
  4 mm/s.
- Water-Wash Resin 2.0: Mono 4: exposure 2.4 against 2.8 s, lift 5 against 6 mm, lift and
  retract 8 against 4 mm/s.
- ABS-Like Resin Pro 2 (US page): Mono 4: retract 6 against 4 mm/s. Mono 4K: bottom layers 6
  against 1. Mono X 6K: bottom 23 against 25 s. M3 Premium: bottom layers 4 against 2.
- ABS-Like Resin Pro 2 (EU page): Mono 4: retract 6 against 4 mm/s. M5s Pro: light-off 1 against
  0.5 s. M5s: exposure 3 against 2.8 s, lift 6 against 5 mm/s. M5: bottom layers 6 against 5,
  lift 6 against 5 mm/s. Mono X 6Ks: bottom layers 6 against 5, lift 2 against 3 mm/s, retract 3
  against 4 mm/s. Mono X 6K: bottom 23 against 25 s. M3 Max: light-off 2 against 2.5 s, bottom
  30 against 35 s, lift 4 against 2 mm/s, retract 4 against 3 mm/s. M3 Premium: bottom layers 3
  against 2. Mono 2: bottom 28 against 25 s. Mono X2: bottom 25 against 20 s. Mono 4K: bottom
  layers 6 against 1.
- ABS-Like Resin 2.0: Mono 4: retract 6 against 4 mm/s.
- ABS-Like Resin 3.0: Mono 4: bottom layers 6 against 5, retract 6 against 4 mm/s.
- Water-Wash ABS-Like Resin 3.0: Mono 4: exposure 2.4 against 2.8 s, lift 5 against 6 mm, lift
  and retract 8 against 4 mm/s.
- BJD Wear-Resistant Resin: Mono 4: exposure 2.8 against 2.5 s, retract 6 against 4 mm/s.
- High Speed Resin 2.0: P1: exposure 1.5 against 1.1 s. Mono 4: retract 6 against 4 mm/s.
- Tough Resin 2.0: M5, M5s, M5s Pro, Mono X2: light-off 0.5 against 1 s. Mono 4: retract 6
  against 4 mm/s. Mono 4K: light-off 0.5 against 1 s, bottom layers 6 against 1. Mono X 6K:
  light-off 0.5 against 1 s, bottom 25 against 28 s. M3 Max: light-off 2.5 against 2 s.
  M3 Premium: light-off 0.5 against 1 s, bottom 25 against 30 s.
- Tough Resin Ultra: Mono 4: retract 6 against 4 mm/s. Mono 4K: bottom layers 6 against 1.
  Mono 2: bottom layers 5 against 6. Mono X 6Ks: light-off 1 against 0.5 s. M3 Premium: bottom
  layers 2 against 5.
- DLP Craftsman Resin: M5, M5s: exposure 2.8 against 3 s, bottom 25 against 30 s, lift 5 against
  6 mm/s. Mono 4: retract 6 against 4 mm/s. Mono X 6K: exposure 2 against 2.5 s. Mono X 6Ks:
  exposure 2.5 against 2 s, light-off 1 against 0.5 s. M3 Max: exposure 3 against 2 s.
  M3 Premium: bottom layers 4 against 2. The page has no Mono 4K row.
- Plant-Based UV Resin 2.0: Mono 4: exposure 2.4 against 2.8 s, retract 6 against 4 mm/s (only
  the Mono 4 and P1 tables of A8 name this resin).
- High Clear Resin (EU page of 2026-06-26): M5, M5s: exposure 4 against 5 s, bottom 25 against
  35 s, lift 5 against 6 mm/s. M5s Pro: bottom 30 against 35 s. Mono 4: retract 6 against
  4 mm/s. Mono 4K: bottom layers 6 against 1. Mono 2: exposure 4 against 4.5 s. Mono X 6K:
  bottom 28 against 23 s. Mono X 6Ks: bottom 20 against 25 s, lift 3 against 2 mm/s, retract 4
  against 3 mm/s. The US page's last capture (2025-06-23) also differs for the M3 Plus, M3, D2
  and Mono X 6K.
- 14K Texture Resin (EU and Australian pages): Mono 4: exposure 2.4 against 2.8 s, lift 5 against
  6 mm, lift and retract 8 against 4 mm/s. Its table equals the Water-Wash ABS-Like Resin 3.0's
  except an extra P1 Max row; inferred: possibly copied.

### Anycubic: V1.0 manuals against A8 (observed)

The manuals cover only older resins (Standard, Standard+, ABS-Like+, Tough, Water-Wash+, DLP
Craftsman). For the Standard Resin, which the catalog takes: Mono X anti-aliasing 1 against 4;
Mono 4K bottom layers 6 against 1; M3 Plus 1.5 s, light-off 0.1 s, bottom 23 s × 3 layers, lift
4 mm at 6 mm/s against A8's 2.8 s, 1 s, 30 s × 6, 5 mm at 3 mm/s.

### ELEGOO (observed)

- The Jupiter 2's manual gives a lift and retract of 60 + 280 mm/min; the table gives 60 + 390.
  The table is used.
- The Saturn 4 Ultra 16K's manual of 2026-09-03 gives 5 bottom layers, the value the catalog
  already used as its own choice.
- Inside the table: the Mars 4 Max's ABS-Like 3.0 Pro rows lift 2 + 3 mm where its other rows lift
  3 + 4; the Saturn 4's Water-Washable ABS-Like rows and its Standard Plant-based Grey row use
  75 + 230 mm/min where its others use 65 + 180. The catalog takes each row as published.
- On the Jupiter 2, Water-Washable ABS-Like gives the longer exposure to White; on every other
  printer to Black. On the Mars 4 Ultra, Standard 1.0 Black's bottom exposure (25–30 s) is below
  Translucent's and Maroon's (30–35 s).
- The Mars 4 Max's manual shows the Mars 4 DLP's values ("Take Mars 4 DLP as an example"), so its
  bottom layers are unknown and it has no print settings.
- No 8K resin rows for the Jupiter SE or Mars 4 Max; the Mars 5 spells "8K ABS-Like Resin 3.0".
- The manuals' slicer example says "Resin Density: 1.15 g/ml" for no named resin; the safety data
  sheets give 1.05 g/cm³ for each resin. The data sheets are used.

### SUNLU (observed)

- The Standard Resin's sources disagree on every range: monochrome bottom 10–50, 10–60 or
  20–60 s; normal 1.5–3, 1.5–3.5, 1.5–4 or 2–4 s.
- Toughness and Water-Wash ABS-Like: one comparison page gives each the other's ranges; inferred:
  swapped there.
- 14K Red Wax and 14K ABS-Like: bottom ranges differ between the product page, the wiki, the
  comparison page and the data sheet.
- Dental Model: normal exposure 2.5–3.5, 2.5–3 or 2–2.5 s by source.
- PA-Like, PC-Like, Flexible and High Clear: lift and retract speeds and lift heights differ by
  source; the Flexible's sheet prints a retract of "120-90".
- Legacy data sheets give 3–5 bottom layers; every current source gives 5–10.
- Data sheets mistitled (notes 1–3 above) and colour lists that differ from the product pages.

## 6. Open items

- Re-read Anycubic's product pages live when the store answers, to confirm the captured values
  and the 1.13 g/cm³ of the Standard Resin.
- Read ELEGOO's and SUNLU's store pages (colours sold, bottle sizes) when they answer.
- Per-colour variants where colours differ (CATALOG-SOURCES.md lists them), if users ask.
- SUNLU print settings by screen type, should SUNLU or a printer maker publish usable values.
