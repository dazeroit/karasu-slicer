//! Imports the independently owned corpus; no commercial installation or raw evidence is needed.

use karasu_core::geometry::{Point3, Vector3};
use karasu_core::mesh::stl::{import_stl_bytes, import_stl_path, StlOptions, StlUnitPolicy};
use karasu_core::mesh::{Degeneracy, MeshDiagnostic};
use std::{fs, path::PathBuf, time::Instant};

struct Fixture<'a> {
    id: &'a str,
    variant: Option<&'a str>,
    filename: &'a str,
    triangle_count: usize,
    vertex_count: usize,
    min: [f64; 3],
    max: [f64; 3],
    degenerates: usize,
    duplicates: usize,
}

const MANIFEST: &str = include_str!("../../../fixtures/reference/manifest.json");

// These helpers read only the known, versioned manifest's primitive fields, not arbitrary JSON.
// Keeping them test-local avoids adding a production JSON parser or a development dependency.
fn field<'a>(text: &'a str, name: &str) -> &'a str {
    text.split_once(&format!("\"{name}\":"))
        .unwrap()
        .1
        .trim_start()
}

fn string(text: &str) -> &str {
    text.strip_prefix('"').unwrap().split('"').next().unwrap()
}

fn count(text: &str, name: &str) -> usize {
    field(text, name)
        .split([',', '}'])
        .next()
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

fn triple(text: &str, name: &str) -> [f64; 3] {
    let values: Vec<f64> = field(text, name)
        .strip_prefix('[')
        .unwrap()
        .split(']')
        .next()
        .unwrap()
        .split(',')
        .map(|v| v.trim().parse().unwrap())
        .collect();
    values.try_into().unwrap()
}

fn fixtures() -> Vec<Fixture<'static>> {
    assert_eq!(count(MANIFEST, "schema_version"), 1);
    MANIFEST
        .split("\"fixture_id\":")
        .skip(1)
        .map(|entry| {
            let variant = field(entry, "variant");
            Fixture {
                id: string(entry.trim_start()),
                variant: variant.starts_with('"').then(|| string(variant)),
                filename: string(field(entry, "filename")),
                triangle_count: count(entry, "triangle_count"),
                vertex_count: count(entry, "unique_vertex_count"),
                min: triple(entry, "min"),
                max: triple(entry, "max"),
                degenerates: count(entry, "degenerate_triangles"),
                duplicates: count(entry, "duplicate_triangles"),
            }
        })
        .collect()
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/reference")
}
fn policy() -> StlOptions {
    StlOptions::new(StlUnitPolicy::AssumeMillimeters)
}

fn fixture(id: &str, variant: Option<&str>) -> Fixture<'static> {
    fixtures()
        .into_iter()
        .find(|f| f.id == id && f.variant == variant)
        .unwrap()
}

fn raw_vertices(bytes: &[u8]) -> Vec<[Point3; 3]> {
    bytes[84..]
        .as_chunks::<50>()
        .0
        .iter()
        .map(|record| {
            [12, 24, 36].map(|offset| {
                let coordinate = |offset| {
                    f64::from(f32::from_le_bytes([
                        record[offset],
                        record[offset + 1],
                        record[offset + 2],
                        record[offset + 3],
                    ]))
                };
                Point3::new(
                    coordinate(offset),
                    coordinate(offset + 4),
                    coordinate(offset + 8),
                )
            })
        })
        .collect()
}

#[test]
fn all_eleven_small_fixtures_match_manifest_counts_bounds_and_defects() {
    let small: Vec<_> = fixtures().into_iter().filter(|f| f.id != "FX-11").collect();
    assert_eq!(small.len(), 11);
    for entry in small {
        let result = import_stl_path(root().join(entry.filename), policy()).unwrap();
        let stats = result.mesh.statistics();
        assert_eq!(
            stats.vertex_count, entry.vertex_count,
            "{} {:?}",
            entry.id, entry.variant
        );
        assert_eq!(stats.triangle_count, entry.triangle_count);
        let bounds = stats.source_bounds.unwrap();
        for (actual, expected) in bounds
            .min()
            .coordinates()
            .into_iter()
            .zip(entry.min)
            .chain(bounds.max().coordinates().into_iter().zip(entry.max))
        {
            // Manifest endpoints are rounded to nine decimal places; source f32 values are
            // promoted exactly. This allowance is for manifest text rounding, not mesh welding.
            assert!(
                (actual - expected).abs() <= 5.1e-10,
                "{}: {actual} versus {expected}",
                entry.id
            );
        }
        assert_eq!(
            result
                .diagnostics
                .iter()
                .filter(|d| matches!(d, MeshDiagnostic::DegenerateTriangle { .. }))
                .count(),
            entry.degenerates
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .filter(|d| matches!(d, MeshDiagnostic::DuplicateTriangle { .. }))
                .count(),
            entry.duplicates
        );
    }
}

#[test]
fn all_source_corners_and_winding_are_preserved_without_transform_or_repair() {
    for entry in fixtures().into_iter().filter(|f| f.id != "FX-11") {
        let bytes = fs::read(root().join(entry.filename)).unwrap();
        let result = import_stl_bytes(&bytes, policy()).unwrap();
        for (indices, source) in result.mesh.triangles().iter().zip(raw_vertices(&bytes)) {
            let actual = indices.map(|i| result.mesh.vertices()[i as usize]);
            assert_eq!(actual, source, "{} {:?}", entry.id, entry.variant);
        }
        assert_eq!(import_stl_bytes(&bytes, policy()).unwrap(), result);
    }
}

#[test]
fn cube_and_cylinder_have_exact_twenty_mm_source_extents() {
    for id in ["FX-01", "FX-02"] {
        let entry = fixture(id, None);
        let result = import_stl_path(root().join(entry.filename), policy()).unwrap();
        assert_eq!(
            result.mesh.source_bounds().unwrap().dimensions(),
            Vector3::new(20.0, 20.0, 20.0)
        );
        assert_eq!(result.diagnostics, []); // The historical Lychee warning is not a validity oracle.
    }
}

#[test]
fn rotated_prism_keeps_baked_shape_at_unmodified_source_coordinates() {
    let entry = fixture("FX-05", None);
    let result = import_stl_path(root().join(entry.filename), policy()).unwrap();
    let bounds = result.mesh.source_bounds().unwrap();
    assert_eq!(
        bounds.dimensions(),
        Vector3::new(
            18.372_800_827_026_367,
            31.739_122_390_747_07,
            35.429_767_608_642_58
        )
    );
    assert_eq!(bounds.min().z.to_bits(), 0.0_f64.to_bits());
    assert!(bounds.min().x < 0.0 && bounds.min().y < 0.0);
    assert_eq!(result.diagnostics, []);
}

#[test]
fn defective_fixtures_keep_all_faces_and_do_not_claim_topological_validity() {
    let entry = fixture("FX-08", None);
    let result = import_stl_path(root().join(entry.filename), policy()).unwrap();
    assert_eq!(result.mesh.statistics().triangle_count, 15);
    assert!(result
        .diagnostics
        .contains(&MeshDiagnostic::DegenerateTriangle {
            triangle: 12,
            kind: Degeneracy::RepeatedVertex
        }));
    assert!(result
        .diagnostics
        .contains(&MeshDiagnostic::DegenerateTriangle {
            triangle: 13,
            kind: Degeneracy::ZeroComputedArea
        }));
    assert!(result
        .diagnostics
        .contains(&MeshDiagnostic::DuplicateTriangle {
            triangle: 14,
            first: 0
        }));
    for (id, variant, count) in [
        ("FX-09", None, 12),
        ("FX-10", Some("A"), 10),
        ("FX-10", Some("B"), 13),
    ] {
        let entry = fixture(id, variant);
        let result = import_stl_path(root().join(entry.filename), policy()).unwrap();
        assert_eq!(result.mesh.statistics().triangle_count, count);
        assert_eq!(result.diagnostics, []); // M2.1 does not perform edge-incidence analysis.
    }
}

#[test]
#[ignore = "requires the independently generated local FX-11 file; never regenerates it"]
fn optional_fx11_bounded_import_measurement() {
    let entry = fixture("FX-11", None);
    let path = root().join(entry.filename);
    assert!(
        path.is_file(),
        "FX-11 must already exist before this optional measurement"
    );
    let start = Instant::now();
    let result = import_stl_path(path, policy()).unwrap();
    let elapsed = start.elapsed();
    assert_eq!(
        result.mesh.statistics().triangle_count,
        entry.triangle_count
    );
    assert_eq!(result.mesh.statistics().vertex_count, entry.vertex_count);
    assert_eq!(result.diagnostics, []);
    eprintln!("FX-11: {} triangles / {} vertices; import+diagnostics={elapsed:?}; facet metadata={} bytes/face", entry.triangle_count, entry.vertex_count, std::mem::size_of_val(&result.metadata.facets[0]));
}
