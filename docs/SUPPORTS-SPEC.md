# Manual supports on any model (TD-062)

**Status:** implemented by TD-062 on 2026-10-19.

**Research:** supports reference research.

**Scope:**
- Individually editable manual supports on any closed model, solid or hollowed.
- Each support has:
  - a tip on the model;
  - an optionally bent shaft;
  - a foot on the plate, on the model or on another support.
- Braces between supports, and an optional raft.

**Out of scope:** automatic placement (later work), and the existing convex certified KSUP1 plans
of M5.5-D, which remain unchanged for preparation sessions.

## 1. Model and conventions

- **Units and angles:** coordinates are world millimetres after placement and preparation, the
  same frame as `--drain-hole`. Angles are in degrees. All values are finite `f64`.
- **The model's frame (TD-108):** the interface writes an object's document in the object's
  model frame instead, and the pipeline places it with the object's placement before building
  it (`SupportDocument::placed`). Tips, bends, feet on the model and explicit axes follow every
  move, turn, scale and mirror; a foot on the plate at `[x, y]` is the model frame's point
  `(x, y, 0)`, carried and set on the plate; heights on shafts, the raft and the parameters are
  world values. Such a document takes no binding (refused if present).
- **Binding:** a support document describes supports for one placed and prepared model. It may
  record the model's SHA-256 and placement; when it does, a mismatch is an error. Whether bound or
  not, every anchor is checked against the actual surface, so a stale document fails on its
  contacts instead of printing misplaced supports.
- **The model surface:** the mesh that is sliced:
  - the placed model;
  - the hollowed shell, outer surface and cavity, when hollowing is used.

  Its triangles are outward, pointing away from material, so cavity triangles face into the
  cavity. A contact is **interior** when its anchor triangle belongs to the cavity, otherwise
  **exterior**.

## 2. Document (`karasu-supports`, version 1)

The document is JSON, and unknown fields are rejected:

```json
{
  "format": "karasu-supports",
  "version": 1,
  "binding": { "model_sha256": "…", "translation_mm": [x, y, z] },
  "preset": "medium",
  "defaults": { "contact_diameter_mm": 0.35 },
  "clearance_mm": 0.3,
  "anchor_tolerance_mm": 0.05,
  "max_axis_deviation_deg": 60,
  "raft": { "thickness_mm": 1.0, "margin_mm": 1.0, "slope_deg": 60,
            "fillet_diameter_mm": 2.0, "fillet_height_mm": 0.8, "groups_join_mm": 10 },
  "supports": [
    {
      "id": 1,
      "tip": { "point": [x, y, z], "axis": "normal" },
      "bends": [[x, y, z]],
      "base": { "platform": [x, y] },
      "parameters": { "shaft_diameter_mm": 1.2 },
      "accept_collision": false
    }
  ],
  "braces": [
    { "id": 100, "from": { "support": 1, "z_mm": 6 }, "to": { "support": 2, "z_mm": 9 },
      "diameter_mm": 0.8 }
  ]
}
```

**Top-level fields:**
- `binding`, `defaults`, `raft` and `braces` are optional.
- **`binding`** records what the supports were placed for:
  - `model_sha256`: the model file's digest; another model is refused.
  - `translation_mm`: the placement's translation. Alone, the placement must match it exactly.
  - **The pose (TD-072):** `rotate_deg`, `scale` and `mirror` (as for placement; defaults: no
    rotation, unit scale, no mirroring), with `translation_mm` required. The supports then follow
    the model through any placement that differs only by a move on the plate and a turn about
    the vertical axis:
    - The whole document moves rigidly by that motion: tips, bends, bases and axes. Junction and
      brace heights, the raft and the parameters are unchanged.
    - A different rotation about X or Y, scale or mirroring is refused. Shafts are vertical and
      stand on the plate, so they cannot follow a tilt.
    - So is a height change beyond `anchor_tolerance_mm`. Within it, every anchor is still
      checked on the surface.
    - The summary reports the turn and where the model's bounding-box centre moved.
- `preset` is one of `light`, `medium` (the default) and `heavy`.
- **Parameter resolution:** each support's parameters are the preset, then `defaults`, then its
  own `parameters`. Each support therefore owns a complete parameter set, and editing one support
  never changes another.

**Tip axis (`tip.axis`):**
- `normal`: the surface's outward normal at the contact (TD-124): the area-weighted mean of the
  normals of the triangles within the contact's footprint radius that face the anchor
  triangle's way. The axis limit is measured from it too, and a model foot's from the mean over
  its own contact.
- `straight`: towards the next path point (the first bend or the base join).
- `[x, y, z]`: an explicit direction, normalised.

**Base:**
- `{"platform": [x, y]}`: on the plate.
- `{"vertical": true}`: on the plate below the last path point.
- `{"model": [x, y, z], "axis": …}`: on the model surface, with the same axis options pointing
  out of the surface.
- `{"support": id, "z_mm": z}`: on another support's shaft at height `z`.

**Kind (`kind`, optional; owner, 2026-10-30):** `standard` (the default) or `mini`. A mini
support is a thin rod of one diameter with a ball at each end: no tip cone, no foot, no pad. It
holds a spot where a standard support finds too little room below, or no way down to the plate,
from the model below or from another support's shaft. Its base is `model` or `support`; on the
plate (`platform` or `vertical`) it is refused, since it would have no pad to hold it. Its tip
axis and its foot axis take the same options as a standard support's. Any other value is refused.

**Origin (`origin`, optional):** what placed the support: `placed` (the default) or `islands`
(placed under an island by the interface's Islands tool, which can find and delete its own;
owner, 2026-10-29). It changes nothing in the support's shape or checks; any other value is
refused.

**Identifiers and references:**
- IDs are nonzero and unique across supports and braces.
- References must exist, and support-to-support bases must not form cycles.
- An unresolvable reference is an error, never silently dropped.

## 3. Parameters

Hard ranges are errors. Soft ranges produce warnings.

| Parameter | Hard range | Soft range | Light | Medium | Heavy |
| --- | --- | --- | --- | --- | --- |
| `contact_shape` | `cone`, `sphere` | – | cone | cone | cone |
| `contact_diameter_mm` | 0.05–5 | ≥ 0.2 | 0.3 | 0.5 | 0.6 |
| `contact_fit_min_mm` | 0.05–5, or absent | ≥ 0.2 | absent | absent | absent |
| `contact_sphere_diameter_mm` | 0.05–5 | – | 0.5 | 0.6 | 0.8 |
| `tip_body_diameter_mm` | 0.1–10 | – | 0.8 | 1.0 | 1.0 |
| `tip_length_mm` | 0.2–30 | ≥ 1 | 2.0 | 2.5 | 2.5 |
| `penetration_mm` | 0–5 | ≤ 1 | 0.1 | 0.1 | 0.1 |
| `shaft_diameter_mm` | 0.1–20 | – | 0.8 | 1.0 | 1.2 |
| `shaft_bottom_diameter_mm` | 0.1–20, or absent | – | absent | absent | absent |
| `sides` | 3–64 | ≥ 6 | 16 | 16 | 16 |
| `pad_diameter_mm` | 0.5–50 | – | 4.0 | 5.0 | 7.0 |
| `pad_thickness_mm` | 0.1–5 | ≤ 2 | 0.4 | 0.5 | 0.6 |
| `cone_diameter_mm` | 0.1–50 | – | 2.0 | 2.5 | 3.5 |
| `cone_height_mm` | 0–20 | – | 0.8 | 1.0 | 1.2 |
| `foot_contact_diameter_mm` | 0.05–5 | ≥ 0.2 | 0.3 | 0.4 | 0.6 |
| `foot_body_diameter_mm` | 0.1–10 | – | 0.8 | 1.0 | 1.4 |
| `foot_length_mm` | 0.2–30 | – | 1.5 | 2.0 | 2.5 |
| `foot_penetration_mm` | 0–5 | ≤ 1 | 0.15 | 0.2 | 0.3 |
| `mini_diameter_mm` | 0.1–2 | ≥ 0.2 | 0.3 | 0.4 | 0.5 |
| `mini_penetration_mm` | 0–1 | – | 0.05 | 0.1 | 0.15 |

**Coupled rules (errors):**
- the contact diameter must not exceed the tip body diameter;
- the foot contact diameter must not exceed the foot body diameter;
- the cone diameter must not exceed the pad diameter;
- the contact sphere diameter must be at least the contact diameter;
- the smallest fitted contact, `contact_fit_min_mm`, must not exceed the contact diameter;
- the mini penetration must be less than half of the mini diameter (a ball's centre stays out of
  the surface). Zero is accepted: the ball then touches the surface.

**Document settings:**

| Setting | Range | Default |
| --- | --- | --- |
| `clearance_mm` | 0–5 | 0.3 |
| `anchor_tolerance_mm` | 0.001–1 | 0.05 |
| `max_axis_deviation_deg` | 0–89 | 60 |
| Raft `thickness_mm` | 0.2–5 | – |
| Raft `margin_mm` | 0–20 | – |
| Raft `slope_deg` (side-wall angle from horizontal) | 30–90 | – |
| Raft `fillet_diameter_mm` (default 2) | 0.2–10 | – |
| Raft `fillet_height_mm` (default 0.8) | 0–5 | – |
| Raft `groups_join_mm` (absent: one raft) | 0.5–200 | – |
| Raft `shape` (absent: `slab`; TD-144) | `slab`, `skate`, `web` | `slab` |
| Raft `bar_width_mm` (web only; absent: 1) | 0.2–10 | 1 |
| Raft `bar_height_mm` (web only; absent: 0.5) | 0.2–5 | 0.5 |
| Raft `bar_longest_mm` (web only; absent: 20) | 1–200 | 20 |
| Brace `diameter_mm` (absent: automatic) | 0.1–10 | – |
| Brace `max_diameter_mm` (automatic only; absent: no limit) | 0.1–10 | – |

Karasu chose these presets within the ranges observed in the references. They are not copies.
TD-136 set the contacts, depths, tips and shafts from two test prints on the Photon P1
(`docs/research/SUPPORT-PRESETS-RESEARCH.md`, E-SUP-02 and E-SUP-03): every contact sunk 0.1 mm,
Medium's contact 0.5 mm, Heavy's 0.6 mm on a 1.0 mm tip, 2.5 mm long, and a 1.2 mm shaft. They
are geometric parameters proven on flat test tiles of one resin, not general print settings.
The feet, pads, cones and mini supports keep their first values.

**The width at the surface** (`SupportParameters::surface_width_mm`, TD-136): a cone contact's
diameter is its end's, `penetration_mm` inside the model; where it meets a level surface it is
`d + (body - d) * min(penetration / tip_length, 1)` wide, the mark it leaves (Light 0.33, Medium
0.52, Heavy 0.62 mm); a ball's is its footprint's. The support panel shows it beside the
contact.

## 4. Geometry

**Shared rules:**
- Every part is a closed, outward, consistently oriented convex triangle mesh.
- Rings and spheres use the platform-independent degree trigonometry of TD-061, so geometry is
  bit-identical on every platform.
- Tessellation depends only on the support's own `sides`.
- One geometry serves previews, slicing and any export.

**Primitives:**
- **Ring** (centre `c`, unit axis `u`, radius `r`, `n` sides):
  - Reference axis: the world axis least aligned with `u` (ties in X, Y, Z order).
  - Frame: `e = unit(u × reference)` and `f = u × e`.
  - Vertices: `c + r(cos θₖ e + sin θₖ f)` with `θₖ = 360·k/n`.
- **Frustum:** two rings joined, with flat caps. A zero radius gives a cone apex.
- **Sphere:** a latitude–longitude sphere with `n` meridians and `max(2, n/2)` bands.

**One support.** Let `P` be the contact point, `a` the unit tip axis, `p` the penetration and `L`
the tip length.
- **Tip join:** `J = P + a·L`.
- **Contact:**
  - Cone: a frustum from `P − a·p` (contact radius) to `J` (tip body radius).
  - Sphere: a sphere of the contact sphere radius `ρ` centred at `S = P + a·(ρ − p)`, so it
    reaches depth `p` below the surface, plus the frustum from `S` (contact radius) to `J`.
- **Path:** `J`, then the bends, then the base join `B`.
- **Shaft:** one frustum per path segment. The radius runs linearly by arc length from the shaft
  radius at `J` to the bottom radius at `B`.
- **Joints:** spheres at `J`, at every bend and at `B`, each with the larger adjacent radius.
- **Base:**
  - **Plate, without a raft:**
    - Pad: a cylinder from 0 to `pad_thickness` at the base XY.
    - Cone: a frustum from the pad top (cone radius) to `B = (x, y, pad_thickness +
      cone_height)` (shaft bottom radius).
  - **Plate, with a raft** (owner, 2026-10-29: the raft is the feet): no pad. The floor `z₀` is
    the raft top. The fillet is a frustum from `(x, y, z₀ − thickness/2)`, inside the raft
    (radius the larger of the fillet's and the shaft bottom's), to `B = (x, y, z₀ +
    fillet_height)` (shaft bottom radius); with no height it joins the shaft straight into the
    raft.
  - For `vertical`, the base XY is that of the last path point.
  - **Model:** mirrors the tip on the anchor `M`, with outward normal `m`, foot axis `b`, and the
    foot parameters. The foot join `F = M + b·foot_length` is `B`. A route that stands a
    support on the model (TD-132) gives `b` explicitly: straight up when that is within the
    axis limit of the mean normal, otherwise the mean normal turned up by the limit less
    0.5°, so the ball at the shaft's bottom rests on the foot instead of hanging below it.
  - **Support:** `B` is the point at height `z` on the parent's path. The parent segment there
    must not be horizontal. The parent is built first.
- **Mini support** (`kind: mini`). Let `r` be half the mini diameter, `p` the mini penetration
  and `c` the clearance.
  - **Ends:** the tip's ball, of radius `r`, centred at `T = P + a·(r − p)`, so it reaches depth
    `p` below the surface. On the model, the foot's ball mirrors it at the anchor `M` with the
    foot axis `b`: `F = M + b·(r − p)`. On a support, the end is the junction point `B` on the
    parent's path, with a ball of radius `r` there.
  - **Path:** `T`, the bends, then `F` (or `B`). The foot's end must lie below the tip's:
    otherwise it is an invalid path and nothing is built.
  - **Rod:** one cylinder of radius `r` per path segment, with a ball at every bend.
  - **Its parts:** within `2(r + c)` of an end on the model, measured along the path, the rod is
    that end's part (Contact at the tip, Foot at the foot), so what shows or picks a tip finds
    it; the rest is Shaft.
  - Its path is a shaft as any other, for supports standing on it and for braces. The Braces
    tool's planned braces leave mini supports out.
- **Braces:** a cylinder between the two support path points at their heights, plus joint
  spheres. Without `diameter_mm` a brace is automatic (owner, 2026-10-29): its diameter is the
  thinner of the two shafts' where it meets them (a tapered shaft's diameter at that height),
  worked out at each build, so it follows its supports' values; with `max_diameter_mm` (owner,
  2026-11-06, TD-136), at most that (`Brace::radius_mm`). A brace with a `diameter_mm` of its
  own takes no `max_diameter_mm`; a document written before TD-136 has none, and its automatic
  braces build as they did.
- **Planned braces** (`plan_braces`, the Braces tool's automatic braces): braces a document
  could add between neighbouring supports, from a plan (maximum distance in plan, lowest
  height above the floor, step, pattern and diameter, or none for automatic braces). Nothing is
  added: the caller adds the kept ones. Each planned brace reports its diameter as built; an
  automatic plan keeps clear of the shafts' end joints by the wider shaft's diameter, at most
  the plan's largest.
  - **Neighbours** (owner, 2026-10-29: a brace joins two supports next to each other, never
    past a third): each built support stands at the plan position of its path's lowest point.
    Two supports no farther apart than the distance are a pair only when no third support is
    nearer to both than they are to each other (the relative neighbourhood graph): in a row,
    A–B and B–C, never A–C; on a grid, the cells' sides, no diagonal; in a triangle, its two
    shorter sides. Such pairs never cross in plan; as a guard for exact ties, pairs are also
    taken nearest first, ties by their IDs, and kept only when their plan segment crosses no
    pair kept before (a support lying on it counts as crossing). A grid of cells the distance's
    size finds the near supports and kept pairs.
  - **Levels:** the floor plus the lowest height, then every step above it, the same for every
    pair. A pair takes the levels from a brace's diameter above the higher shaft bottom to a
    diameter below the lower tip join. Horizontal: a level brace per level. Zigzag: one
    diagonal per step between consecutive levels, rising from the first support of the pair
    (the lower ID) on even steps and from the second on odd ones. Cross: both diagonals.
  - **Skipped, never dropped:** a brace whose cylinder or end spheres are not proven the
    document's clearance from the model, or that enters a drain hole's channel through the
    model (TD-123), or on a pair the document
    already braces less than half a step from its heights, is returned with the reason and ID
    zero. Kept braces take the IDs after the document's highest, in order.
  - Deterministic: the result depends only on the document, the surface, the drains and the
    plan.
- **Raft:** one raft under every plate foot, or with `groups_join_mm` one under each group of
  feet whose base points lie less than that apart, directly or through others (found with a
  grid of that size, so the work follows the feet). Each is the 3D convex hull of its feet's
  fillet rings, taken twice:
  - at `z = thickness`, each fillet radius plus the margin;
  - at `z = 0`, the same radius plus `thickness / tan(slope)`.

  The shafts stand on the raft top through their fillets. A raft with no plate support is an
  error.

### Contacts fitted to the surface (TD-116)

With `contact_fit_min_mm`, a standard support's contact narrows to fit the surface. It never
widens. The build takes the first diameter, from `contact_diameter_mm` down by 0.05 mm steps to
`contact_fit_min_mm` (the last step is that value itself), whose footprint has room. The contact
ball and the penetration narrow by the same factor; every other value stays. Mini supports and
model feet are not fitted.

- **Footprint radius `r`:** half the contact diameter for a cone; for a ball of radius `ρ` sunk
  by `d`, the circle where it meets the tangent plane, `√(d(2ρ − d))` (or `ρ` when `d ≥ ρ`).
- **Room**, at the tip `p` with outward anchor normal `n`, with `e = max(d / 2, 2τ)`:
  - the point `e` below `p` is inside the model: from it to `p + 2τn`, the first surface crossed
    is left, not entered;
  - from depth `e` to `d + τ` below `p`, the column crosses no surface;
  - 16 spokes of length `r`, perpendicular to `n` (the frame of section 4), from the column at
    depth `e` and at depth `d`, cross no surface.
- **Meaning:** the whole footprint keeps at least half its penetration, and at least `2τ`, in
  the model. A sharp edge, a fin narrower than the footprint, a tip too close to a border or a
  wall thinner than the penetration narrows the contact; a concave corner does not. The room is
  the surface's, measured along its normal, whatever the tip's axis.
- **No room at the smallest:** the smallest is built, with a **warning** on the contact.
- **Reported:** each built support gives its contact's diameter as built.

## 5. Validation

Validation never repairs, clamps or drops anything. It produces findings. Each finding names the
support or brace, the part, the kind, the severity and the numbers involved.

A support with an error is still built whenever its shape is defined, so it can be seen and
corrected: an axis beyond the limit, piercing, a collision, a drain hole or the build volume
leave its parts in the scene. Only an anchor off the surface, an invalid path (coincident
points, a plate base above the tip, an invalid parent) and an over-long contact sphere leave
no shape to build. Whether a document with errors is sliced is the caller's decision.

**Errors (the document cannot be sliced):**
- **Document:** schema, ranges and references.
- **Anchor off the surface:** an anchor (tip or model foot) whose nearest surface triangle is
  farther than `anchor_tolerance_mm`.
- **Axis:** a tip or foot axis that deviates from the anchor normal by more than
  `max_axis_deviation_deg`.
- **Mini supports and the model** (owner, 2026-10-30: a mini support rests among the model's
  parts, as in other slicers): no part of a mini support keeps the clearance, and touching or
  entering the model is no error, since the resin there is one. The Islands and Automatic tools place
  none that touches the model away from its balls (TD-126); placed by hand, it may. Where its axis crosses a surface
  triangle (checked from `2τ` past each end's ball centre on the model), a **warning** says so,
  once per support.
- **Piercing:** the penetration segment, from depth `2τ` to `p + τ` below the anchor along the
  axis, crosses any surface triangle (`τ` is the anchor tolerance). The support would pierce a
  thin wall or reach a cavity. The segment is empty, and nothing checked, only when `p ≤ τ`
  (TD-136: before, the check started at `p > 2τ` and so left out every preset's 0.1 mm).
- **Collision, not accepted:**
  - A part comes closer to the model surface than `clearance_mm`.
  - Exempt are the support's own contact part and the model-foot frustum, and every part of a
    mini support. Everything else is checked: joints (including `J`), shaft, base, braces and
    raft.
  - **The reach (TD-124):** a contact's or model foot's cone must still not enter the model away
    from where it touches. From where its side rises `2τ` above the contact's tangent plane to its
    join, the cone must meet no surface triangle (a proof by separating axes, without
    clearance); against triangles within three footprint radii of the contact, the cone narrowed
    by the contact's penetration. A surface rising beside the contact, such as the other side of
    a crease, or a fold the cone grazes farther away, then makes a collision of the contact
    part; bumps around the contact shallower than the penetration do not.
  - A collision is accepted only through the support's `accept_collision`, or a global
    acceptance by the caller. It then remains a reported finding.
- **Foot in a drain hole (TD-131):** a base standing on the model at a point strictly inside a
  drain hole's cylinder, where the hole cuts the model away: nothing is under it.
- **Build volume:** a part outside the printer's XY area or Z height, or below `z = 0`.
- **Plate base above the tip:** a plate base whose join `B` is not below the next path point
  above it.

**Warnings:**
- values in a soft range;
- a contact fitted to the surface with no room even at its smallest (section 4);
- a shaft segment more than 45° from vertical;
- collisions that were accepted;
- a support part entering a drain hole's channel, which it may partly plug (TD-123; a warning
  since TD-131, as a hole often makes an island that needs a support where it starts). The
  channel is the hole's cylinder over each stretch of its axis inside the model's material:
  from where the axis enters the surface to where it leaves it, or to the hole's end. The
  entry's margin before the surface, a cavity the hole crosses and the margin past a cavity wall
  are no channel: a support there is whole and plugs nothing. Parts are tested by their tapered
  capsule, so a cone's thin contact end counts as thin. Braces are planned clear of channels,
  as a brace holds no island, and automatic supports place no tip strictly inside a hole's
  cylinder, where the hole cuts the surface away.

**Collision proof:**
- **Candidates:** a uniform grid of model triangles gives the candidates for each part's bounding
  box, enlarged by the clearance.
- **Test:** a part is clear of a triangle when separating-axis projections prove a gap of at
  least `clearance_mm + δ`, with `δ = 10⁻⁶ mm` absorbing rounding. The candidate axes are:
  - the part's face normals;
  - the triangle normal;
  - cross products of part and triangle edge directions;
  - the direction between closest points of the part's axis segment and the triangle.
- **Conservative:** a projected gap is a lower bound of the true distance, so a proof is sound.
  Failure to prove is reported as a collision.

## 6. Slicing

- **Combined mesh:** the sliced mesh is the model surface plus every support part and the raft,
  all closed outward shells, in world coordinates.
- **Union:**
  - Layers use the `union` fill (positive winding), so overlapping supports, penetrating tips and
    the model merge into one material.
  - Interior supports stand in the cavity, where the shell's winding is zero, and become
    material there.
  - Supports require `--fill union`.
- **Policies (declared):**
  - Antialiasing, minimum gray and XY compensation apply to the whole layer, supports included.
  - Drain cuts subtract from the model's material only (TD-123): a support crossing a hole keeps
    its material, and the check above keeps supports out of the hole's channel.
- **Placement:**
  - With supports, the model may be lifted: its lowest point must be at Z ≥ 0.
  - The layer stack starts at Z = 0, where the raft or the plate pads rest exactly.
  - A model above the plate with neither plate supports nor a raft is an error.
- **Previews:** previews show the model with its supports.

## 7. CLI

- `--supports <file.json>`: slices with the document's supports and lists them in the summary:
  - counts by base kind and by interior or exterior contact;
  - braces;
  - raft;
  - parts and triangles;
  - findings.
- `--accept-support-collisions`: accepts every collision finding globally, recorded in the
  summary.
- `--lift <mm>`: with `--drop`, raises the model so its lowest point is at Z = lift.

## 8. Tests

- **Geometry:**
  - every part is closed, outward and convex;
  - frustum and sphere volumes converge to the analytic values;
  - tessellation is identical regardless of how many other supports exist;
  - the output is bit-identical across runs.
- **Anchors:**
  - on-surface, off-surface and interior/exterior classification;
  - axis deviation;
  - piercing a thin wall.
- **Collision:**
  - a shaft through the model is found;
  - a shaft just outside the clearance is proven clear;
  - acceptance is recorded;
  - drain-hole intersection is refused.
- **Document:**
  - presets, defaults and per-support overrides resolve independently;
  - range, coupling, unknown-field, duplicate-ID, missing-reference and cycle errors.
- **Slicing and CLI:**
  - per-layer support material equals the analytic section of a vertical support;
  - a support under a cantilever, one standing on the model, one inside a hollowed cavity, a
    branch on a support, a brace, and a raft;
- **Mini supports** (TD-113): on the model, with and without penetration (the balls reaching
  it exactly), and on another support's shaft, with no cone, foot or pad; their braces left out
  of a plan; a rod beside the model without a finding, and one through a thin wall with a warning
  only; a foot above the tip, an anchor off the surface, a base on the plate and a penetration
  past the radius refused; the foot found straight below a tip, beside a part too, and none over
  the plate; a hollowed cube sliced with one from the command line.
- **Planned braces:** a square of supports braced round its sides only, built without
  findings; a row braced between next supports only, a triangle along its two shorter sides,
  a grid along its cells' sides; the patterns' counts; braces too close to the
  model, across a drain hole or on a pair already braced, skipped with their reason; a plan out
  of range refused;
  - previews include supports;
  - Linux and Windows outputs are byte-identical.
