use std::fs;
use std::path::Path;

use crate::geometry::{box_mesh, cylinder, icosphere, merge_meshes, rotated_prism, Mesh, Vec3};
use crate::sha256;
use crate::stl;

// Dataset schema/identifiers remain stable across application branding changes.
const MANIFEST_NAME: &str = "manifest.json";

pub(crate) struct GenerationSummary {
    pub(crate) stl_count: usize,
    pub(crate) manifest_name: &'static str,
}

struct Fixture {
    id: &'static str,
    variant: Option<&'static str>,
    filename: &'static str,
    category: &'static str,
    parameters_json: &'static str,
    expected_topology: &'static str,
    experiments: &'static [&'static str],
    mesh: Mesh,
}

pub(crate) fn write_dataset(output_dir: &Path) -> Result<GenerationSummary, String> {
    let stl_dir = output_dir.join("stl");
    fs::create_dir_all(&stl_dir)
        .map_err(|error| format!("cannot create STL directory {}: {error}", stl_dir.display()))?;

    let fixtures = fixtures();
    let mut generated = Vec::with_capacity(fixtures.len());
    for fixture in &fixtures {
        if fixture.mesh.vertices.is_empty() || fixture.mesh.triangles.is_empty() {
            return Err(format!("{} contains no geometry", fixture.filename));
        }
        if !fixture.mesh.all_coordinates_finite() {
            return Err(format!(
                "{} contains a non-finite coordinate",
                fixture.filename
            ));
        }
        let bytes = stl::binary_stl(&fixture.mesh, fixture.id)?;
        let path = stl_dir.join(fixture.filename);
        fs::write(&path, &bytes)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        generated.push((fixture, bytes));
    }

    let manifest = manifest_json(&generated);
    let manifest_path = output_dir.join(MANIFEST_NAME);
    fs::write(&manifest_path, manifest.as_bytes())
        .map_err(|error| format!("cannot write {}: {error}", manifest_path.display()))?;

    Ok(GenerationSummary {
        stl_count: generated.len(),
        manifest_name: MANIFEST_NAME,
    })
}

#[allow(clippy::too_many_lines)]
fn fixtures() -> Vec<Fixture> {
    let cube = box_mesh(
        Vec3::new(-10.0, -10.0, 0.0),
        Vec3::new(10.0, 10.0, 20.0),
        false,
    );
    let outer_box = cube.clone();
    let inner_box = box_mesh(Vec3::new(-8.0, -8.0, 2.0), Vec3::new(8.0, 8.0, 18.0), true);
    let multiple_volumes = merge_meshes(&[
        box_mesh(
            Vec3::new(-12.5, -5.0, 0.0),
            Vec3::new(-2.5, 5.0, 10.0),
            false,
        ),
        box_mesh(Vec3::new(2.5, -5.0, 0.0), Vec3::new(12.5, 5.0, 10.0), false),
    ]);
    let internal_cavity = merge_meshes(&[
        outer_box.clone(),
        icosphere(5.0, Vec3::new(0.0, 0.0, 10.0), 2, true),
    ]);

    let mut degenerate_faces = cube.clone();
    degenerate_faces.vertices.push(Vec3::new(0.0, -10.0, 0.0));
    degenerate_faces.triangles.push([0, 0, 1]);
    degenerate_faces.triangles.push([0, 8, 1]);
    degenerate_faces.triangles.push(cube.triangles[0]);

    let mut reversed_face = cube.clone();
    reversed_face.triangles[0].swap(1, 2);

    let mut open_cube = cube.clone();
    open_cube.triangles.drain(2..4);

    let mut non_manifold = cube.clone();
    non_manifold.vertices.push(Vec3::new(0.0, -10.0, 10.0));
    non_manifold.triangles.push([0, 1, 8]);

    vec![
        Fixture {
            id: "FX-01",
            variant: None,
            filename: "fx-01-cube.stl",
            category: "valid closed solid",
            parameters_json: r#"{"size_mm":[20.0,20.0,20.0]}"#,
            expected_topology: "one closed, watertight, outward-oriented genus-0 shell",
            experiments: &["EXP-01", "EXP-03"],
            mesh: cube.clone(),
        },
        Fixture {
            id: "FX-02",
            variant: None,
            filename: "fx-02-cylinder.stl",
            category: "valid triangulated solid",
            parameters_json: r#"{"radius_mm":10.0,"height_mm":20.0,"segments":128}"#,
            expected_topology: "one closed, watertight, outward-oriented genus-0 shell",
            experiments: &["EXP-01"],
            mesh: cylinder(10.0, 20.0, 128),
        },
        Fixture {
            id: "FX-03",
            variant: None,
            filename: "fx-03-sphere.stl",
            category: "valid triangulated solid",
            parameters_json: r#"{"radius_mm":10.0,"center_mm":[0.0,0.0,10.0],"icosphere_subdivisions":3}"#,
            expected_topology: "one closed, watertight, outward-oriented genus-0 shell",
            experiments: &[],
            mesh: icosphere(10.0, Vec3::new(0.0, 0.0, 10.0), 3, false),
        },
        Fixture {
            id: "FX-04",
            variant: None,
            filename: "fx-04-hollow-box.stl",
            category: "valid solid with enclosed cavity",
            parameters_json: r#"{"outer_size_mm":[20.0,20.0,20.0],"inner_size_mm":[16.0,16.0,16.0],"wall_mm":2.0}"#,
            expected_topology: "two closed watertight shells: outer outward and cavity inward",
            experiments: &[],
            mesh: merge_meshes(&[outer_box, inner_box]),
        },
        Fixture {
            id: "FX-05",
            variant: None,
            filename: "fx-05-rotated-prism.stl",
            category: "valid transformed solid",
            parameters_json: r#"{"size_mm":[10.0,20.0,30.0],"rotation_degrees":{"x":23.0,"z":17.0},"operation_order":["rotate_x","rotate_z","translate_min_z_to_zero"]}"#,
            expected_topology: "one closed, watertight, outward-oriented genus-0 shell",
            experiments: &["EXP-01", "EXP-02"],
            mesh: rotated_prism(),
        },
        Fixture {
            id: "FX-06",
            variant: None,
            filename: "fx-06-multiple-volumes.stl",
            category: "valid disconnected solids",
            parameters_json: r#"{"cube_size_mm":10.0,"x_ranges_mm":[[-12.5,-2.5],[2.5,12.5]],"gap_mm":5.0}"#,
            expected_topology: "two disconnected closed, watertight, outward-oriented shells",
            experiments: &[],
            mesh: multiple_volumes,
        },
        Fixture {
            id: "FX-07",
            variant: None,
            filename: "fx-07-internal-cavity.stl",
            category: "valid solid with curved enclosed cavity",
            parameters_json: r#"{"outer_size_mm":[20.0,20.0,20.0],"cavity_radius_mm":5.0,"cavity_center_mm":[0.0,0.0,10.0],"icosphere_subdivisions":2}"#,
            expected_topology: "two closed watertight shells: outer outward and cavity inward",
            experiments: &[],
            mesh: internal_cavity,
        },
        Fixture {
            id: "FX-08",
            variant: None,
            filename: "fx-08-degenerate-faces.stl",
            category: "intentionally defective mesh",
            parameters_json: r#"{"base":"FX-01","inserted_defects":["repeated-vertex triangle","collinear triangle","duplicate face"]}"#,
            expected_topology:
                "closed cube surface plus two zero-area triangles and one duplicate face",
            experiments: &["EXP-04"],
            mesh: degenerate_faces,
        },
        Fixture {
            id: "FX-09",
            variant: None,
            filename: "fx-09-reversed-face.stl",
            category: "intentionally defective mesh",
            parameters_json: r#"{"base":"FX-01","reversed_triangle_count":1}"#,
            expected_topology: "closed edge incidence with exactly one reversed face",
            experiments: &["EXP-04"],
            mesh: reversed_face,
        },
        Fixture {
            id: "FX-10",
            variant: Some("A"),
            filename: "fx-10a-open.stl",
            category: "intentionally defective mesh",
            parameters_json: r#"{"base":"FX-01","removed_faces":"two top triangles"}"#,
            expected_topology: "open cube with one four-edge boundary loop",
            experiments: &["EXP-04"],
            mesh: open_cube,
        },
        Fixture {
            id: "FX-10",
            variant: Some("B"),
            filename: "fx-10b-non-manifold.stl",
            category: "intentionally defective mesh",
            parameters_json: r#"{"base":"FX-01","added_face":"third face incident to edge 0-1"}"#,
            expected_topology: "cube plus one face producing a non-manifold three-face edge",
            experiments: &["EXP-04"],
            mesh: non_manifold,
        },
        Fixture {
            id: "FX-11",
            variant: None,
            filename: "fx-11-high-complexity.stl",
            category: "valid performance fixture",
            parameters_json: r#"{"radius_mm":30.0,"center_mm":[0.0,0.0,30.0],"icosphere_subdivisions":7}"#,
            expected_topology: "one closed, watertight, outward-oriented genus-0 shell",
            experiments: &[],
            mesh: icosphere(30.0, Vec3::new(0.0, 0.0, 30.0), 7, false),
        },
    ]
}

#[allow(clippy::too_many_lines)]
fn manifest_json(generated: &[(&Fixture, Vec<u8>)]) -> String {
    let mut output = String::from(
        "{\n  \"schema_version\": 1,\n  \"dataset\": \"resin-slicer-m1-reference-fixtures\",\n",
    );
    append(&mut output, format_args!(
        "  \"generator\": {{\"package\": \"reference-fixtures\", \"version\": \"{}\", \"source\": \"tools/reference-fixtures\"}},\n",
        env!("CARGO_PKG_VERSION")
    ));
    output.push_str(
        "  \"units\": \"millimeters\",\n  \"coordinate_system\": {\"handedness\": \"right-handed\", \"up_axis\": \"Z\", \"stl_coordinates\": \"IEEE 754 binary32 little-endian\"},\n  \"fixtures\": [\n",
    );

    for (index, (fixture, bytes)) in generated.iter().enumerate() {
        let serialized_mesh = quantized_mesh(&fixture.mesh);
        let bounds = serialized_mesh.bounds();
        let topology = fixture.mesh.topology();
        let variant = fixture
            .variant
            .map_or_else(|| "null".to_owned(), json_string);
        let experiments = fixture
            .experiments
            .iter()
            .map(|experiment| json_string(experiment))
            .collect::<Vec<_>>()
            .join(", ");
        output.push_str("    {\n");
        append(
            &mut output,
            format_args!("      \"fixture_id\": {},\n", json_string(fixture.id)),
        );
        append(&mut output, format_args!("      \"variant\": {variant},\n"));
        append(
            &mut output,
            format_args!(
                "      \"filename\": {},\n",
                json_string(&format!("stl/{}", fixture.filename))
            ),
        );
        append(
            &mut output,
            format_args!(
                "      \"geometry_category\": {},\n",
                json_string(fixture.category)
            ),
        );
        append(
            &mut output,
            format_args!("      \"parameters\": {},\n", fixture.parameters_json),
        );
        append(
            &mut output,
            format_args!(
                "      \"bounding_box_mm\": {{\"min\": {}, \"max\": {}}},\n",
                vector_json(bounds.min),
                vector_json(bounds.max)
            ),
        );
        append(&mut output, format_args!(
            "      \"unique_vertex_count\": {},\n      \"stl_vertex_record_count\": {},\n      \"triangle_count\": {},\n",
            fixture.mesh.vertices.len(),
            fixture.mesh.triangles.len() * 3,
            fixture.mesh.triangles.len()
        ));
        append(
            &mut output,
            format_args!(
                "      \"expected_topology\": {},\n",
                json_string(fixture.expected_topology)
            ),
        );
        let oriented_volume = if fixture.category.starts_with("valid") {
            json_number(serialized_mesh.signed_volume())
        } else {
            "null".to_owned()
        };
        append(
            &mut output,
            format_args!("      \"oriented_mesh_volume_mm3\": {oriented_volume},\n"),
        );
        append(&mut output, format_args!(
            "      \"observed_topology\": {{\"connected_components\": {}, \"boundary_edges\": {}, \"non_manifold_edges\": {}, \"inconsistent_oriented_edges\": {}, \"degenerate_triangles\": {}, \"duplicate_triangles\": {}}},\n",
            topology.connected_components,
            topology.boundary_edges,
            topology.non_manifold_edges,
            topology.inconsistent_oriented_edges,
            topology.degenerate_triangles,
            topology.duplicate_triangles
        ));
        append(
            &mut output,
            format_args!(
            "      \"file_size_bytes\": {},\n      \"sha256\": {},\n      \"experiments\": [{}]\n",
            bytes.len(),
            json_string(&sha256::hex_digest(bytes)),
            experiments
        ),
        );
        output.push_str(if index + 1 == generated.len() {
            "    }\n"
        } else {
            "    },\n"
        });
    }
    output.push_str("  ]\n}\n");
    output
}

fn quantized_mesh(mesh: &Mesh) -> Mesh {
    Mesh {
        vertices: mesh
            .vertices
            .iter()
            .map(|vertex| {
                Vec3::new(
                    f64::from(stl::quantize_coordinate(vertex.x)),
                    f64::from(stl::quantize_coordinate(vertex.y)),
                    f64::from(stl::quantize_coordinate(vertex.z)),
                )
            })
            .collect(),
        triangles: mesh.triangles.clone(),
    }
}

fn vector_json(vector: Vec3) -> String {
    format!(
        "[{},{},{}]",
        json_number(vector.x),
        json_number(vector.y),
        json_number(vector.z)
    )
}

fn json_number(value: f64) -> String {
    let normalized = if value == 0.0 { 0.0 } else { value };
    if normalized.fract() == 0.0 {
        format!("{normalized:.1}")
    } else {
        let formatted = format!("{normalized:.9}");
        let trimmed = formatted.trim_end_matches('0');
        if trimmed.ends_with('.') {
            format!("{trimmed}0")
        } else {
            trimmed.to_owned()
        }
    }
}

fn json_string(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn append(output: &mut String, arguments: std::fmt::Arguments<'_>) {
    use std::fmt::Write;
    output
        .write_fmt(arguments)
        .expect("writing JSON to a String cannot fail");
}

#[cfg(test)]
mod tests {
    use super::fixtures;
    use crate::stl;

    #[test]
    fn names_and_triangle_counts_match_the_reference_matrix() {
        let fixtures = fixtures();
        let actual = fixtures
            .iter()
            .map(|fixture| {
                (
                    fixture.id,
                    fixture.variant,
                    fixture.filename,
                    fixture.mesh.triangles.len(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            actual,
            vec![
                ("FX-01", None, "fx-01-cube.stl", 12),
                ("FX-02", None, "fx-02-cylinder.stl", 512),
                ("FX-03", None, "fx-03-sphere.stl", 1_280),
                ("FX-04", None, "fx-04-hollow-box.stl", 24),
                ("FX-05", None, "fx-05-rotated-prism.stl", 12),
                ("FX-06", None, "fx-06-multiple-volumes.stl", 24),
                ("FX-07", None, "fx-07-internal-cavity.stl", 332),
                ("FX-08", None, "fx-08-degenerate-faces.stl", 15),
                ("FX-09", None, "fx-09-reversed-face.stl", 12),
                ("FX-10", Some("A"), "fx-10a-open.stl", 10),
                ("FX-10", Some("B"), "fx-10b-non-manifold.stl", 13),
                ("FX-11", None, "fx-11-high-complexity.stl", 327_680),
            ]
        );
    }

    #[test]
    fn all_coordinates_and_serializations_are_deterministic() {
        for fixture in fixtures() {
            assert!(
                fixture.mesh.all_coordinates_finite(),
                "{}",
                fixture.filename
            );
            assert_eq!(
                stl::binary_stl(&fixture.mesh, fixture.id).expect("first serialization"),
                stl::binary_stl(&fixture.mesh, fixture.id).expect("second serialization"),
                "{}",
                fixture.filename
            );
        }
    }

    #[test]
    fn valid_fixtures_are_closed_and_consistently_oriented() {
        for fixture in fixtures().into_iter().filter(|fixture| {
            matches!(
                fixture.id,
                "FX-01" | "FX-02" | "FX-03" | "FX-04" | "FX-05" | "FX-06" | "FX-07" | "FX-11"
            )
        }) {
            let topology = fixture.mesh.topology();
            assert_eq!(topology.boundary_edges, 0, "{}", fixture.filename);
            assert_eq!(topology.non_manifold_edges, 0, "{}", fixture.filename);
            assert_eq!(
                topology.inconsistent_oriented_edges, 0,
                "{}",
                fixture.filename
            );
            assert_eq!(topology.degenerate_triangles, 0, "{}", fixture.filename);
            assert_eq!(topology.duplicate_triangles, 0, "{}", fixture.filename);
            assert!(fixture.mesh.signed_volume() > 0.0, "{}", fixture.filename);
        }
    }

    #[test]
    fn deliberately_inserted_defects_are_present() {
        let fixtures = fixtures();
        let fx08 = fixtures
            .iter()
            .find(|fixture| fixture.id == "FX-08")
            .expect("FX-08");
        let topology = fx08.mesh.topology();
        assert_eq!(topology.degenerate_triangles, 2);
        assert_eq!(topology.duplicate_triangles, 1);

        let fx09 = fixtures
            .iter()
            .find(|fixture| fixture.id == "FX-09")
            .expect("FX-09");
        assert!(fx09.mesh.topology().inconsistent_oriented_edges > 0);

        let open_variant = fixtures
            .iter()
            .find(|fixture| fixture.id == "FX-10" && fixture.variant == Some("A"))
            .expect("FX-10A");
        assert_eq!(open_variant.mesh.topology().boundary_edges, 4);

        let non_manifold_variant = fixtures
            .iter()
            .find(|fixture| fixture.id == "FX-10" && fixture.variant == Some("B"))
            .expect("FX-10B");
        assert_eq!(non_manifold_variant.mesh.topology().non_manifold_edges, 1);
    }

    #[test]
    fn exact_axis_aligned_bounds_match_the_specification() {
        let fixtures = fixtures();
        for (filename, expected_min, expected_max) in [
            ("fx-01-cube.stl", [-10.0, -10.0, 0.0], [10.0, 10.0, 20.0]),
            (
                "fx-02-cylinder.stl",
                [-10.0, -10.0, 0.0],
                [10.0, 10.0, 20.0],
            ),
            ("fx-03-sphere.stl", [-10.0, -10.0, 0.0], [10.0, 10.0, 20.0]),
            (
                "fx-04-hollow-box.stl",
                [-10.0, -10.0, 0.0],
                [10.0, 10.0, 20.0],
            ),
            (
                "fx-06-multiple-volumes.stl",
                [-12.5, -5.0, 0.0],
                [12.5, 5.0, 10.0],
            ),
            (
                "fx-07-internal-cavity.stl",
                [-10.0, -10.0, 0.0],
                [10.0, 10.0, 20.0],
            ),
            (
                "fx-08-degenerate-faces.stl",
                [-10.0, -10.0, 0.0],
                [10.0, 10.0, 20.0],
            ),
            (
                "fx-09-reversed-face.stl",
                [-10.0, -10.0, 0.0],
                [10.0, 10.0, 20.0],
            ),
            ("fx-10a-open.stl", [-10.0, -10.0, 0.0], [10.0, 10.0, 20.0]),
            (
                "fx-10b-non-manifold.stl",
                [-10.0, -10.0, 0.0],
                [10.0, 10.0, 20.0],
            ),
            (
                "fx-11-high-complexity.stl",
                [-30.0, -30.0, 0.0],
                [30.0, 30.0, 60.0],
            ),
        ] {
            let bounds = fixtures
                .iter()
                .find(|fixture| fixture.filename == filename)
                .expect("fixture")
                .mesh
                .bounds();
            for (actual, expected) in [bounds.min.x, bounds.min.y, bounds.min.z]
                .into_iter()
                .zip(expected_min)
            {
                assert!((actual - expected).abs() < 1.0e-12, "{filename}");
            }
            for (actual, expected) in [bounds.max.x, bounds.max.y, bounds.max.z]
                .into_iter()
                .zip(expected_max)
            {
                assert!((actual - expected).abs() < 1.0e-12, "{filename}");
            }
        }
    }

    #[test]
    fn rotated_prism_bounds_follow_the_declared_transform() {
        let fixture = fixtures()
            .into_iter()
            .find(|fixture| fixture.id == "FX-05")
            .expect("FX-05");
        let bounds = fixture.mesh.bounds();
        let x_angle = 23.0_f64.to_radians();
        let z_angle = 17.0_f64.to_radians();
        let rotated_y_extent = 10.0 * x_angle.cos() + 15.0 * x_angle.sin();
        let width_radius = 5.0 * z_angle.cos() + rotated_y_extent * z_angle.sin();
        let depth_radius = 5.0 * z_angle.sin() + rotated_y_extent * z_angle.cos();
        let vertical_span = 20.0 * x_angle.sin() + 30.0 * x_angle.cos();

        assert!((bounds.min.x + width_radius).abs() < 1.0e-12);
        assert!((bounds.max.x - width_radius).abs() < 1.0e-12);
        assert!((bounds.min.y + depth_radius).abs() < 1.0e-12);
        assert!((bounds.max.y - depth_radius).abs() < 1.0e-12);
        assert!(bounds.min.z.abs() < 1.0e-12);
        assert!((bounds.max.z - vertical_span).abs() < 1.0e-12);
    }
}
