# Suction-cup detection (M6.4j)

**Status:** core analysis and command line implemented on 2026-10-31 (TD-114). The Drain holes
tool's part (section 8) was built the same day from the owner's mockup; it awaits the owner's
trial.
Research: [suction cups](research/SUCTION-CUP-RESEARCH.md).

## 1. What a suction cup is

The printer builds the print from the plate, one layer at a time, and peels each layer from the
vat's film. While layer `k` is printed, only layers `0..=k` exist.

At layer `k`, an empty region (resin, as the masks see it) is a **suction cup** when, through
layers `0..=k`, it does not reach the outside:
- material surrounds it in its layer;
- material caps it towards the plate, or the plate itself does under the first layer.

Peeling layer `k` then pulls on the resin it encloses.

A cup spans the layers it is open to the film at. It ends in one of two ways:
- **Then open:** it reaches the outside, through a vent or past the rim of an open shape;
- **Closes:** the layers close over it, and its resin stays trapped. TD-101's drainage already
  reports that resin as a cavity region of the finished part. The cup says when it pulls while
  printing.

Examples, from the plate up:
- A hollow box on the plate, or on supports, with no hole is a cup through its cavity, and it
  closes.
- A ring standing on the plate is a cup up to its rim: the plate caps it. On supports it is
  open below and is no cup.
- A bowl open towards the vat is a cup up to its rim. One open towards the plate is none.
- A hole on the plate side of a cavity vents it. A hole on the vat side does not, until the
  layers reach it.

## 2. Vents

An opening narrower than a vent does not relieve a cup (owner, 2026-10-31: "a pixel would not
do; it would still be a suction cup physically"). The **minimum vent** is an area in mm², the
owner's setting: an empty passage counts as a vent only where a circle of that area fits through
it.

- **Test.** The empty pixels of each layer are eroded by the circle's radius less one pixel (the
  largest pitch), the rasterization's allowance. A pixel survives when no material pixel centre
  lies within that radius of its centre, on the lattice of the two pitches. The survivors are
  joined again across rows and layers. A cup that the whole empty pixels would vent, but the
  eroded ones would not, has openings too small.
- **Zero, or under a pixel.** A minimum vent whose radius is less than a pixel erodes nothing:
  any opening of a whole empty pixel vents.
- **Default:** 1 mm² (a circle about 1.13 mm across) on the command line. The interface's
  default is decided with its mockup.
- **Shape.** A round hole of the minimum area passes, within the pixel's allowance. A square of
  the same area does not: its inscribed circle is smaller. A slot narrower than the circle does
  not vent, however long.

## 3. Input and conventions

- The **exported masks**, with supports and raft: they can cap a cup, and only openings left in
  the print vent it. Each layer comes with its plane's height and its slab height (for volumes).
- A pixel is **material** when its gray reaches `material_gray`. The pipeline passes 1: a pixel
  cured at all seals, and no opening is credited that the print would not leave open.
- **Connectivity:**
  - empty pixels join by an edge within a layer (4-neighbours, the dual of the material's
    8-neighbours in TD-107);
  - the same pixel of consecutive layers joins vertically;
  - empty pixels on the panel's border are the outside, as is everything above the last layer;
  - below the first layer is the plate, which caps.
- **Units:** pixel coordinates in the report (column, row; a pixel's centre at half units);
  areas in mm² and volumes in mm³ from the pitches and slab heights. The pipeline converts the
  cap to world millimetres.

## 4. Algorithm

The analysis sweeps the layers from the plate up, twice at once:
- once on the whole empty pixels;
- once on the eroded ones, when the minimum vent erodes anything.

1. **Each layer**, on every core a batch at a time:
   - its material runs;
   - their complement (the empty runs);
   - with a vent, the complement of the material grown by the vent's disc.
2. **Labelling.** A layer's empty runs are joined across rows by a union-find into components,
   numbered by their first run. Each component becomes one node of the sweep's union-find:
   - node 0 is the outside, joined by every component that touches the border;
   - components join the components of the layer under that they overlap by a pixel.
3. **Pieces.** After layer `k` is joined, every component not joined to the outside is a cup
   piece of layer `k`:
   - In the whole sweep, such a piece has no opening.
   - In the eroded sweep, a component inside a whole-sweep piece belongs to that piece.
     Otherwise it is a piece of its own, with openings too small. Its statistics count the
     pixels its runs reach grown again by the disc: the region the vent's circle sweeps.
4. **Cups.** Pieces are joined into cups:
   - those of one layer sharing a node (they enclose one resin, joined below);
   - those overlapping a piece of the layer under in either sweep.

   Since unions only ever merge, a piece recorded at layer `k` stays a fact of that layer even
   when a later layer vents its resin.
5. **Report**, per cup:
   - its first and last layer;
   - its opening (`None`, or `TooSmall` when any piece is eroded);
   - whether it closes. It closes when, after its last layer, none of its last pieces reaches
     the outside, and its last layer is not the print's last;
   - its widest layer and pixels, its pixels and volume over its layers;
   - its cap: the centroid of its first layer's pixels;
   - its bounds;
   - its **parts** (below).

   Cups are sorted by first layer, then the cap's column and row.
6. **Parts** (owner, 2026-10-31).
   - **Why.** A cavity with several lowest points, such as the hollow legs and arms of a figure
     turned over, is one cup once its parts meet above. Each part still pulls on its own resin
     from its cap up to where it meets an older one, and needs its own vent. Reported as one cup
     with one cap, a vent at that cap relieved only its own part, and the others then appeared as
     new cups: on the owner's turned `HillGiant_32mm.stl`, 7 cups became 10.
   - **How parts form.** A piece joined to no piece under it starts a part. Parts that meet go
     on as the oldest (the lowest index of a union-find). Each part keeps:
     - its first piece's centroid and bounds;
     - its last layer as a part of its own;
     - its widest pixels and volume meanwhile.
   - **Where its vent goes.** Its lowest point is often a narrow tip. A vent there opens into
     the part only through the tip, a passage too small by the minimum vent's own rule. So each
     part also keeps the first layer where a circle of the minimum vent fits in it, and the
     centroid of where it fits (the eroded pixels inside it): `vent_layer` and `vent`.
   - **Limit.** Parts are found on the whole empty pixels. A part joined to the rest only
     through narrow passages inside the cavity shows as a cup of its own (openings too small)
     once the rest is vented, and is found by searching again.

**Determinism.** Layers are joined in order. Components are numbered by their first run, and
union roots are the lowest index. The parallel step writes each layer's runs from that layer
alone. The report is the same for any thread count.

**Cost.** Work follows the runs. Two layers' labels are held per sweep, plus one node per empty
component of each layer and one record per piece. On the owner's hollowed `HillGiant_32mm.stl`
(774 layers at 13312 × 5120 px):
- 0.75 s on 16 cores with a 1 mm² vent, against 3.13 s before the batch was made parallel;
- 0.23 s without erosion.

**Limits.**
- A cup is joined to a later piece through the eroded sweep only where their eroded pixels
  overlap. A cup narrower than the vent's circle at one layer, and vented through a narrow
  opening at the next, may be reported as two cups.
- The region an eroded piece sweeps leaves out the cup's corners the circle cannot reach.
- `MAX_PIECES` (50 million) bounds the pieces. Past it the analysis fails rather than
  truncating.

## 5. What is not hidden

Every cup is reported, by TD-107's rule. A sculpted model has hundreds of pixel-scale dimples and
crevices: 1319 cups on the hollowed HillGiant, 9 of them at least 1 mm² wide and 0.5 mm tall.
The interface needs a display filter on the widest area and the height, with the rest counted;
its defaults are decided with its mockup.

## 6. Command line

`slice ... --suction-cups [--min-vent <mm2>]` adds the cups to the summary. It reports and never
fails the job:

```text
Suction cups (vents of at least 1 mm^2): <count>, the largest <area> mm^2
Suction cup: layers <first>-<last> (cap at x <x>, y <y>, z <z> mm), <area> mm^2, <volume> mm^3, <opening>, <end>, <n> cap(s)
Suction cup part: layers <first>-<last> (cap at x <x>, y <y>, z <z> mm), <area> mm^2, <volume> mm^3
Suction cups not listed: <count>
```

- **Which cups are listed:** at most 20, the widest first (the summary's limit, like the
  unsupported regions').
- **Cap:** the centroid at the bottom of the cup's first layer.
- **Parts:** each of a cup's parts after the first follows it, with its own layers, cap, widest
  area and volume.
- **`<opening>`:** `no opening` or `openings too small`.
- **`<end>`:** `closes over trapped resin` or `then open`.
- **Refused usage:** `--min-vent` without `--suction-cups`, and a negative or non-finite area.

## 7. Validation

- **Unit tests** (`karasu_core::analysis::suction`):
  - a closed box: one cup through its cavity that closes, with its exact area, volume, cap and
    bounds;
  - a ring on the plate and a lifted one;
  - a side vent wider and narrower than the minimum;
  - a floor vent with and without a minimum;
  - two chimneys over one chamber, and two separate cups sorted;
  - refused parameters.
- **Command line** (`suction_cups_report_the_resin_a_ring_on_the_plate_encloses`):
  - a ring on the plate;
  - a slotted ring, vented with no minimum and too narrow at 20 mm²;
  - the misused options.
- **Reference:** the ten shapes of the research agree layer for layer with UVtools 7.0.3, except
  where the minimum vent differs by design.
- **Interface** (`hollowing_ui::cups`): a hollowed cube resting on the plate is searched; its
  closed cavity is one cup that closes, with its area, cap and height, and it is shown. Chosen,
  it is framed at the layer where its vent goes. Its vent goes sideways through a wall, a
  radius above the cap, 4.5 mm deep.
- **Physical prints:** still to do. A hollow test piece printed without a vent, with one on the
  vat side and with one on the plate side must show the cups before any claim that the analysis
  predicts print failures.

## 8. The Drain holes tool

From the owner's mockup ("Suction cups in the Drain holes tool", 2026-10-31); see
`docs/UI-DESIGN.md`, "Drain holes panel".

- **The search** slices the selected model alone, as it will be exported (supports, raft and
  drain holes included), in the plate's format, on a thread of its own, with the panel's minimum
  vent. It reflects the plate as it was; once the plate changes, its cups are out of date until
  found again.
- **What is shown.** The core reports every cup. The panel and the markers show those at least
  as wide (widest layer) and as tall as the display filter (1 mm² and 0.5 mm by default). The
  rest are counted and listed under All.
- **Parts in the panel.** The chosen cup says how many vents it needs: its own part's and those
  of its other parts as wide and as tall as the display filter. A stepper goes through them,
  framing each just above its vent point. Markers stand at the vent points: the chosen cup's
  parts each with one, the chosen part larger.
- **The vent** (Add a vent here, for the chosen part; Add a vent at each lowest point, for every
  part the cup needs a vent at) is a new surface hole on the outside, with an explicit depth. It
  starts from the part's vent point, where a vent opens into it wide enough.
  - **Ways out.** Each way is followed to where it leaves the model, past any material on the
    way (hits entering the model, or on other models, are passed). From a cavity, a way goes
    through the cavity's wall. From a crevice or a dish of the surface, open to the vat, it goes
    through the crevice's wall.
    - Straight down, from 0.01 mm above the vent point.
    - Down at 45°, and level, each along 16 directions round the vertical, from a radius above
      the vent point (half the part's height left, if less).
  - **Choice** (revised after the owner's second trial, 2026-11-02). Straight down was taken
    however long. Under a crevice of the turned `HillGiant_32mm.stl` it crossed the whole model,
    and the hole missed the crevice.
    - A way that ends within 0.05 mm of the lowest point of a model resting on the plate is left
      out: the plate closes it.
    - The shortest way goes first, straight down counting three quarters of its length.
    - Ways whose face leans more than 60° from them go last.
  - **The hole** is a surface hole at the way's exit, its depth reaching 0.5 mm past the vent
    point into the part.
    - Its axis follows the area-weighted normal of the faces within its radius, as drainage
      resolves it (`surface_hole_outward` in the core). It is turned onto the way by the exact
      tilt (`axis_tilt`).
    - A single face's normal, used at first, leaned far from that average on the sculpted
      surface, and the hole missed.
    - A way whose hole would have to turn past 60° is passed for the next.
  - **Then** it is checked like a placed hole. A vent relieves only what lies above it: the
    narrow tip under a vent point stays a small cup, usually under the filter.
- **Section.** Choosing a cup or a part cuts the section in the layer where its vent goes,
  near its bottom, where it is plainly closed (owner, 2026-11-02). At its widest layer, often
  its last, a cup whose openings are too narrow already showed them and looked open.
- **Limit.** The vent point is a centroid and may lie outside a curved part; its vent may then
  miss it, and the hole's check, or a new search, says so.
