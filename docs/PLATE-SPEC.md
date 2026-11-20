# Multiple models per plate (TD-065)

**Status:** implemented on 2026-10-19 (priority 5 of the
engine assessment, second part).

**Research and owner decisions:** research.

**Code:**
- `karasu_core::plate`: the document, the shelf packer and the overlap check;
- the CLI's `--plate`, `--copies`, `--arrange` and `--spacing` (`slice/plate.rs`).

## Document (`karasu-plate`, version 1 or 2)

```json
{"format": "karasu-plate", "version": 1, "spacing_mm": 2,
 "objects": [
   {"name": "bust", "model": "bust.stl", "units": "mm", "translate_mm": [105.84, 59.185, 5],
    "arrange": false, "supports": "bust-supports.json"},
   {"model": "ring.3mf", "drop": true, "rotate_deg": [0, 0, 30], "copies": 6},
   {"model": "vase.stl", "units": "mm", "drop": true, "copies": 2,
    "hollow": {"wall_mm": 2, "drain_holes": [{"entry_mm": [0, 0, -1], "axis": "+z",
                                              "length_mm": 4, "diameter_mm": 3}]}}]}
```

**Document limits:** unknown fields are refused, as is any document over 1 MiB.

**Plate:**
- `spacing_mm`: 0–100, default **2** (owner decision);
- 1–256 objects, with at most 4096 instances in all.

**Paths:** model and support paths are relative to the plate document.

**Object fields:** the same meanings as the single-model options.

| Field | Single-model option | Default |
| --- | --- | --- |
| `model` | `--model` | required |
| `name` | none | the model path |
| `units` | `--units` | none |
| `rotate_deg` | `--rotate` | none |
| `scale` | `--scale` | none |
| `mirror` | `--mirror` | none |
| `translate_mm` | `--translate` | `[0, 0, 0]` |
| `center` | `--center` | false |
| `drop` | `--drop` | false |
| `lift_mm` | `--lift` | none |
| `hollow` (`wall_mm`, `cell_mm`, `simplify_mm`, `drain_holes`) | `--hollow`, `--hollow-cell`, `--hollow-simplify`, `--drain-hole`, `--surface-drain-hole` | none |
| `drain_holes` (version 2) | `--drain-hole`, `--surface-drain-hole` | none |
| `supports` | `--supports` | none |
| `accept_support_collisions` | `--accept-support-collisions` | false |
| `arrange` | none | **true** |
| `copies` | `--copies` | 1, up to 1000 |

**Rules:**
- The single-model rules apply:
  - `translate_mm` excludes `center` and `drop`;
  - `lift_mm` needs `drop`, and a positive lift needs supports;
  - a surface hole from the cavity, or without `depth_mm`, needs `hollow` (TD-089);
  - `accept_support_collisions` needs supports.
- `copies > 1` needs `arrange`.
- `center` needs `"arrange": false`.

**Drain holes in the document** (TD-066, TD-089):
- an explicit cylinder `{"entry_mm", "axis": "+z" | "direction": [dx, dy, dz], "length_mm",
  "diameter_mm"}`;
- or a surface hole `{"surface_mm", "diameter_mm", "outside_mm"?, "inside_mm"?}` (margins
  default to 1 mm).

**Version 2 (TD-089)** keeps everything above and adds:
- `drain_holes` at the object level, without `hollow`: holes are independent of hollowing;
- for a surface hole, `"surface": "outer"` (the default) or `"cavity"`, the cavity's surface
  the hole goes out from, and `depth_mm`, a fixed depth from the surface instead of reaching the
  cavity;
- for a surface hole, `"tilt": {"angle_deg", "azimuth_deg"}` (M6.4c c5): the axis leans from the
  surface normal by `angle_deg`, 0 to 60, towards `azimuth_deg`, -360 to 360, measured around the
  normal from the reference tangent of `HoleTilt` (world +Z projected onto the surface, or world
  +X where the normal is near vertical). A zero angle is a perpendicular hole.

The hollowing's own holes come first, then the object's. Holes are written in the design
placement's world coordinates, as before. Version 1 documents are still read; a version 1
document with version 2 fields is refused. Hollowing a mirrored object is accepted since TD-089.

`simplify_mm` is a tolerance in mm or `"auto"`
([specification](HOLLOW-SIMPLIFY-DRAIN-SPEC.md)).

**Per-object vs global:** what belongs to the plate stays global (MM-R2), from the command line:
layer height, exposure, layer plans, antialiasing, minimum gray, compensation, fill, hole closure
and repair. The objects carry no such fields.

## Design placement and instances

Every object is prepared **once** at its *design placement*:
- **Pipeline:** import, repair, topology checks, placement, hollowing, drain holes and supports,
  exactly as a single model.
- **Coordinates:** support documents and drain holes are written in world coordinates of that
  placement, so a single-model support file works unchanged in a plate.

Each **instance** is the prepared geometry moved rigidly in XY by an offset:
- **Fixed objects** (`"arrange": false`): no offset. Their placement must lie on the plate, as for
  a single model.
- **Arranged objects:** the offset comes from the packer. Their design placement is checked only
  in Z: resting on the plate, or lifted on supports.

## Footprints and arrangement

**Footprint** of an object: the XY rectangle of everything it prints at its design placement.
- **Contents:** the sliced surface (model or hollow shell, plus supports and raft). Drain-hole
  cylinders are not part of it, as each cuts only its own model (TD-166, below).
- **Shape:** footprints are axis-aligned rectangles; with `--arrange-footprint outline`, convex
  outlines (TD-165, below).
- **Turns (issue #48):** with `--arrange-turn`, an arranged instance may turn about Z about its
  object's pivot, the middle of its design footprint: the object by the turn that makes its
  outline's rectangle smallest by 1 % or more, or that makes it fit the plate (TD-162), and each
  instance a quarter turn more as the packer chooses. Its vertices, drain rings, footprint,
  outline and drain cutters' boxes turn with it, then move; quarter turns are exact. Without
  turns nothing changes: an instance only moves, bit for bit as before.

**Packer (`plate::arrange`):** deterministic, from the plate's middle outwards (owner,
2026-11-10; next-fit shelves from the front-left corner before).
- **Order:** by decreasing area, then depth, then width, then instance order.
- **Placement:** each footprint goes where its centre is nearest the plate's middle, among the
  corner that centres it on the plate and those that put it beside a footprint already placed or
  a fixed one: one spacing beyond one of its sides, flush with that side's ends or centred on
  it. A candidate closer than the spacing to anything placed or fixed is skipped. The group
  grows outwards from the middle in a grid; fixed footprints anywhere are kept clear, and the
  others still start from the middle.
- **Fallback:** when some footprints do not fit, next-fit shelves are tried too (by decreasing
  depth; each shelf fills from the left, moves past fixed footprints, and starts again above
  them when they leave an empty shelf no room; centred without fixed objects), and taken when
  they leave fewer out. A plate never holds fewer than shelves fit.
- **Area:** the plate inset by one pixel pitch (the larger axis), so rounding in the rigid move
  can never put geometry off the panel.
- **Cost:** measured on Linux in release, 100 footprints in 4 ms, 400 in 82 ms
  (`arrange_is_timed_on_many_footprints`).
- **Arithmetic:** plain `f64` in a fixed order, so results are bitwise identical on every
  platform.

**By outline (TD-165, `--arrange-footprint outline`):** each arranged instance packs its convex
outline seen from above with `plate::outline::arrange_outlines`, clear of the fixed objects'
outlines, from the middle, then from the front left corner when not all fit, never fewer than
rectangles. With `--arrange-turn` each instance is offered its turns' quarter turns, so pointed
models turn half round to nest.

**Instances that do not fit** fail the command with a count and the first ten instances; nothing
is placed off the plate.

**The summary** gives, per instance, `Instance k translation (mm)` (its object's design
translation, turned and moved with it) and, for a turned instance only, `Instance k turn about Z
(deg)`.

## Uniting and slicing

**One body:**
- **Mesh:** all instances are concatenated into one world mesh (objects in document order,
  copies in order) and sliced on one schedule.
- **Union:** under the positive winding rule, overlapping instances print as their union.
- **Top:** the schedule runs to the highest point of any object.

**Overlap:** XY footprint overlap is a diagnostic: `Overlapping footprints` lists the pairs, which
are united. Rectangles are compared first; where two overlap, the convex outlines of the objects'
sliced surfaces decide (TD-165), so copies clear of each other whose rectangles overlap, as
Arrange by outline places them, are not listed.

**Drain cutters:**
- They move with their instance and are removed from every layer analytically, from the models'
  material only: every instance's supports keep theirs (TD-123).
- **Each cuts only its own model (TD-166):**
  - **Reach:** a cutter reaches another instance when its XY box meets that instance's
    footprint, touching included. The box of a cylinder with axis from `p` to `q`, unit
    direction `a` and radius `r` spans `min(p_k, q_k) − r·√(1 − a_k²)` to
    `max(p_k, q_k) + r·√(1 − a_k²)` along each axis `k`, grown by the cutters' 1 µm building
    tolerance.
  - **Cut groups:** group 0 holds every instance but those whose cutters reach another. Each of
    those takes, in instance order, the lowest group from 1 that holds no instance it reaches or
    that reaches it.
  - **Sampling:** each group's cutters remove material from that group's models alone; the
    groups are united per sample, before gray levels, so the plate is still sampled as one body.
    A plate with no reach is one group and slices exactly as before.
  - **Strict layers** (`--fill strict`) cannot keep groups apart: a reach is refused, naming the
    hole and the instance it reaches.

**Previews:** previews show the united plate.

**Adaptive layer plans** read the slopes of every placed model instance, never the supports.

**Single model:** without `--plate`, `--copies` or `--arrange`, a single model keeps its exact
placement transform; its outputs are unchanged.

## CLI

- **Options:**
  - `--plate <plate.json>` replaces `--model`/`--assume-mm`. The per-object options are refused
    with it.
  - `--copies <n>` (1–1000) and `--arrange` arrange a single model's instances; `--spacing <mm>`
    (0–100, default 2) needs one of them, and `--center` is refused with them.
- **Summary:**
  - plate-wide lines: `Objects`, layers, settings and totals;
  - then, per object:
    - name, model path and source;
    - `Instances: n (arranged|fixed), footprint w x d mm`;
    - one `Instance k translation (mm)` per copy;
    - the transform, diagnosis, supports and hollowing;
  - then `Overlapping footprints`;
  - and, only when a cutter reaches another instance, `Drain holes reaching other models: n,
    each cutting only its own model:` with the first ten.

## Not in this version

- Rotation during packing, convex-hull footprints and edge margins chosen by the user.
- Per-object exposure (deferred by research: no clone offers it, and the commercial modes are paid
  and format dependent).
- Linked instances that share supports edits.
- Several plates per job.
