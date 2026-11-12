# M1 Reference Fixture Dataset

## Purpose and provenance

This dataset is generated independently by the native Rust utility in
`tools/reference-fixtures/`. It contains no geometry or resources from Lychee Slicer or CHITUBOX.
The fixtures provide stable inputs for controlled experiments and later native-Rust validation; they
do not encode expected commercial-slicer results.

All coordinates are millimeters in a right-handed coordinate system with positive Z upward. Binary
STL stores coordinates and normals as little-endian IEEE 754 `f32` values. Fixture construction uses
`f64`, then explicitly quantizes during STL serialization. Triangle and vertex ordering is stable,
the 80-byte header contains no timestamp or host path, and the two-byte attribute field is zero.

The machine-readable authority for filenames, serialized bounds, counts, topology observations,
file sizes, and SHA-256 hashes is `fixtures/reference/manifest.json`.

## Fixture ground truth

`Vertices` counts unique indexed source vertices; binary STL repeats three vertex records per
triangle. Mesh volumes below are computed from the serialized, oriented triangle surfaces. A value
is deliberately absent for invalid meshes because signed triangle sums are not a unique printable
volume.

| Fixture | File | Serialized bounds in mm | Vertices | Triangles | Components | Topology | Volume / selected section |
| --- | --- | --- | ---: | ---: | ---: | --- | --- |
| FX-01 | `fx-01-cube.stl` | `[-10,-10,0]` to `[10,10,20]` | 8 | 12 | 1 | Closed, watertight, outward | Exact volume 8,000 mm³; horizontal interior section 400 mm² |
| FX-02 | `fx-02-cylinder.stl` | `[-10,-10,0]` to `[10,10,20]` | 258 | 512 | 1 | Closed, watertight, outward | Mesh volume 6,280.662213576 mm³; interior section is the regular 128-gon area `6,400 sin(π/64)` mm², not exactly `100π` |
| FX-03 | `fx-03-sphere.stl` | `[-10,-10,0]` to `[10,10,20]` | 642 | 1,280 | 1 | Closed level-3 icosphere, outward | Mesh volume 4,152.740796369 mm³; ideal-sphere values `4,000π/3` mm³ and area `100π` mm² at Z=10 are comparison references, not mesh equalities |
| FX-04 | `fx-04-hollow-box.stl` | `[-10,-10,0]` to `[10,10,20]` | 16 | 24 | 2 | Outer shell outward; 16 mm cavity shell inward | Exact material volume 3,904 mm³; section is 400 mm² at Z=1 and 144 mm² at Z=10 |
| FX-05 | `fx-05-rotated-prism.stl` | `[-9.186400414,-15.869561195,0]` to `[9.186400414,15.869561195,35.429767609]` | 8 | 12 | 1 | Closed, watertight, outward | Analytic volume 6,000 mm³; serialized mesh volume differs by about `2.4e-5` mm³ from `f32` quantization |
| FX-06 | `fx-06-multiple-volumes.stl` | `[-12.5,-5,0]` to `[12.5,5,10]` | 16 | 24 | 2 | Two closed outward cube shells | Exact combined volume 2,000 mm³; horizontal interior section 200 mm² |
| FX-07 | `fx-07-internal-cavity.stl` | `[-10,-10,0]` to `[10,10,20]` | 170 | 332 | 2 | Cube outward; inward level-2 icosphere cavity | Mesh material volume 7,494.119420481 mm³; ideal spherical-cavity section at Z=10 is only a comparison reference |
| FX-08 | `fx-08-degenerate-faces.stl` | `[-10,-10,0]` to `[10,10,20]` | 9 | 15 | 1 | FX-01 plus one repeated-vertex triangle, one collinear triangle, and one duplicate face | No unique printable volume asserted |
| FX-09 | `fx-09-reversed-face.stl` | `[-10,-10,0]` to `[10,10,20]` | 8 | 12 | 1 | Closed edge incidence with exactly one reversed triangle and three inconsistent directed edges | Geometric point set encloses the FX-01 cube, but orientation-dependent volume is intentionally not asserted |
| FX-10A | `fx-10a-open.stl` | `[-10,-10,0]` to `[10,10,20]` | 8 | 10 | 1 | Top two triangles removed; four boundary edges | No closed volume |
| FX-10B | `fx-10b-non-manifold.stl` | `[-10,-10,0]` to `[10,10,20]` | 9 | 13 | 1 | One added face creates one three-face edge and two boundary edges | No unique printable volume asserted |
| FX-11 | `fx-11-high-complexity.stl` | `[-30,-30,0]` to `[30,30,60]` | 163,842 | 327,680 | 1 | Closed level-7 icosphere, outward | Mesh volume 113,093.511687739 mm³; ideal-sphere volume `36,000π` mm³ is a comparison reference |

FX-10A and FX-10B preserve the two variants specified under the single authoritative `FX-10`
identifier of the reference test matrix. The manifest records `fixture_id: "FX-10"` plus variant
`A` or `B` rather than inventing new top-level fixture IDs.

FX-05 resolves an underspecified construction detail explicitly: the unrotated `10 × 20 × 30` mm
prism is centered at the origin, rotated 23 degrees around X and then 17 degrees around Z, and finally
translated only along Z until its minimum Z is zero.

## Generation and storage

From the repository root:

```console
cargo run -p reference-fixtures -- --output-dir fixtures/reference
sha256sum fixtures/reference/stl/*.stl
```

The generator writes only its stable filenames and `manifest.json`; it does not delete other files
from the output directory. Repeating the command must reproduce identical bytes and hashes.

The eleven small STL files through FX-10B, the manifest, generator source, and this specification are
suitable for Git. FX-11 is 16,384,084 bytes and is specifically ignored because it is reproducible
and unnecessarily large for source control. Generate it locally before performance work. Its hash
remains in the manifest so regeneration can be verified. Allow substantial temporary memory for its
indexed topology validation; a formal resource benchmark has not yet been performed.

FX-12 (`fx-12-real-model.stl`) is a real model supplied by the owner on 2026-10-18 for future
large-model tests. It is not generated, not part of the generator manifest, and ignored by Git,
so tests that use it must skip explicitly when the file is absent. Identity: binary STL,
133,751,984 bytes, 2,675,038 triangles, header exported by CHITUBOX, SHA-256
`decf78c163e833133ee8c6c671bba4a88006b74a617f740647196125e9c1f518`.

Inspection under the TD-053 limits:
- 1,337,327 indexed vertices; source bounds about 42.1 × 44.0 × 49.5 mm, minimum Z 0.4884 mm;
- one edge- and vertex-connected component with 4,012,553 edges, no boundary edges and no winding
  conflicts;
- two duplicate triangles, which make four non-manifold edges and six non-manifold vertices.
  Winding-union slicing admits it. Strict slicing refuses it unless topology defects are accepted,
  and hollowing refuses it explicitly.

`tests/large_models.rs` (ignored by default) checks its identity, import, edge summary and ten
union layers.

## Validation scope

Automated tests verify binary STL record size/count, finite coordinates, source bounds, deterministic
serialization, fixed filenames and triangle counts, closed-edge incidence and consistent winding for
valid fixtures, positive oriented volume for valid solids, and the named defects in FX-08 through
FX-10. Tests do not slice, repair, rasterize, or infer commercial application behavior.
