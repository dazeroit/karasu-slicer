//! Native-process CLI contracts using owned canonical fixtures and isolated temporary STL files.

use karasu_core::mesh::stl::{import_stl_bytes, StlOptions, StlUnitPolicy};
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

const ASCII: &str = "solid one\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid one\n";
static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Self {
        for _ in 0..100 {
            let index = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("karasu-cli-inspect-{}-{index}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("cannot create isolated test directory: {error}"),
            }
        }
        panic!("cannot find a unique test directory");
    }

    fn write(&self, name: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, bytes).unwrap();
        path
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(id: &str, variant: Option<&str>) -> PathBuf {
    // Primitive extraction is confined to the controlled owned manifest, not production JSON parsing.
    let entry = include_str!("../../../fixtures/reference/manifest.json")
        .split("\"fixture_id\":")
        .skip(1)
        .find(|entry| {
            entry.trim_start().starts_with(&format!("\"{id}\""))
                && variant.is_none_or(|v| {
                    entry
                        .split_once("\"variant\":")
                        .unwrap()
                        .1
                        .trim_start()
                        .starts_with(&format!("\"{v}\""))
                })
        })
        .unwrap();
    let filename = entry
        .split_once("\"filename\":")
        .unwrap()
        .1
        .split('"')
        .nth(1)
        .unwrap();
    root().join("fixtures/reference").join(filename)
}

fn run(args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Output {
    Command::new(env!("CARGO_BIN_EXE_karasu-cli"))
        .args(args)
        .output()
        .unwrap()
}

fn inspect(path: &Path) -> Output {
    run([
        OsStr::new("inspect"),
        OsStr::new("--assume-mm"),
        path.as_os_str(),
    ])
}

fn success(output: &Output) -> String {
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stderr, [] as [u8; 0]);
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn failure(output: &Output, exit: i32) -> String {
    assert_eq!(output.status.code(), Some(exit));
    assert_eq!(output.stdout, [] as [u8; 0]);
    let error = String::from_utf8(output.stderr.clone()).unwrap();
    assert!(error.starts_with("error: "));
    assert!(!error.contains("panicked"));
    error
}

fn field<'a>(text: &'a str, label: &str) -> &'a str {
    text.lines()
        .find_map(|line| line.strip_prefix(&format!("{label}: ")))
        .unwrap()
}

#[test]
fn binary_cube_reports_exact_counts_units_bounds_and_topology() {
    let output = success(&inspect(&fixture("FX-01", None)));
    for (key, expected) in [
        ("STL format", "binary"),
        ("Triangles", "12"),
        ("Indexed vertices", "8"),
        ("Source minimum XYZ (mm)", "-10 -10 0"),
        ("Source maximum XYZ (mm)", "10 10 20"),
        ("Source dimensions XYZ (mm)", "20 20 20"),
        ("Import diagnostics total", "0"),
        ("Usable triangles", "12"),
        ("Excluded triangles", "0"),
        ("Edges", "18"),
        ("Boundary edges", "0"),
        ("Non-manifold edges", "0"),
        ("Winding conflicts", "0"),
        ("Edge-connected components", "1"),
        ("Vertex-connected components", "1"),
        ("Interior-manifold vertices", "8"),
        ("Edge-manifold check", "satisfied"),
        ("Vertex-manifold check", "satisfied"),
        ("Local winding check", "satisfied"),
        ("Closed combinatorial surface", "yes"),
        ("Topology issues total", "0"),
    ] {
        assert_eq!(field(&output, key), expected, "{key}");
    }
    assert_eq!(
        field(&output, "Assumed physical units"),
        "millimeters (1 STL coordinate unit = 1 mm)"
    );
    assert!(field(&output, "Coordinate space").starts_with("source"));
    assert!(field(&output, "Printability").starts_with("not assessed"));
}

#[test]
fn ascii_triangle_has_three_boundary_edges_and_source_dimensions() {
    let temp = TempDirectory::new();
    let output = success(&inspect(&temp.write("triangle.stl", ASCII)));
    for (key, expected) in [
        ("STL format", "ASCII"),
        ("Triangles", "1"),
        ("Indexed vertices", "3"),
        ("Source minimum XYZ (mm)", "0 0 0"),
        ("Source maximum XYZ (mm)", "1 1 0"),
        ("Source dimensions XYZ (mm)", "1 1 0"),
        ("Boundary edges", "3"),
    ] {
        assert_eq!(field(&output, key), expected);
    }
}

#[test]
fn cylinder_reports_indexed_rather_than_raw_corner_counts() {
    let output = success(&inspect(&fixture("FX-02", None)));
    assert_eq!(field(&output, "Triangles"), "512");
    assert_eq!(field(&output, "Indexed vertices"), "258");
    assert_eq!(field(&output, "Edges"), "768");
}

#[test]
fn baked_oblique_bounds_keep_full_precision_source_values() {
    let output = success(&inspect(&fixture("FX-05", None)));
    let min: Vec<f64> = field(&output, "Source minimum XYZ (mm)")
        .split_whitespace()
        .map(|v| v.parse().unwrap())
        .collect();
    let max: Vec<f64> = field(&output, "Source maximum XYZ (mm)")
        .split_whitespace()
        .map(|v| v.parse().unwrap())
        .collect();
    let size: Vec<f64> = field(&output, "Source dimensions XYZ (mm)")
        .split_whitespace()
        .map(|v| v.parse().unwrap())
        .collect();
    let expected_min = [-f64::from(9.186_4_f32), -f64::from(15.869_561_f32), 0.];
    let expected_max = [
        f64::from(9.186_4_f32),
        f64::from(15.869_561_f32),
        f64::from(35.429_768_f32),
    ];
    assert_eq!(min, expected_min);
    assert_eq!(max, expected_max);
    assert_eq!(
        size,
        (0..3)
            .map(|axis| expected_max[axis] - expected_min[axis])
            .collect::<Vec<_>>()
    );
    assert_ne!(
        field(&output, "Source dimensions XYZ (mm)"),
        "18.37 31.74 35.43"
    );
}

#[test]
fn empty_ascii_and_binary_meshes_have_absent_bounds_and_not_applicable_checks() {
    let temp = TempDirectory::new();
    let mut binary = vec![0_u8; 80];
    binary.extend(0_u32.to_le_bytes());
    for path in [
        temp.write("empty-ascii.stl", "solid empty\nendsolid empty\n"),
        temp.write("empty-binary.stl", binary),
    ] {
        let output = success(&inspect(&path));
        assert_eq!(field(&output, "Triangles"), "0");
        assert_eq!(field(&output, "Indexed vertices"), "0");
        for name in [
            "Source minimum XYZ (mm)",
            "Source maximum XYZ (mm)",
            "Source dimensions XYZ (mm)",
        ] {
            assert_eq!(field(&output, name), "absent (empty mesh)");
        }
        assert_eq!(field(&output, "Topology analysis"), "complete");
        assert_eq!(field(&output, "Edges"), "0");
        assert_eq!(field(&output, "Local winding check"), "not applicable");
    }
}

#[test]
fn degenerate_duplicate_and_indeterminate_findings_keep_source_ids() {
    let output = success(&inspect(&fixture("FX-08", None)));
    for (key, expected) in [
        ("Triangles", "15"),
        ("Indexed vertices", "9"),
        ("Degenerate triangles", "2"),
        ("Duplicate triangles", "1"),
        ("Usable triangles", "13"),
        ("Excluded triangles", "2"),
        ("Non-manifold edges", "3"),
        ("Non-manifold vertices", "2"),
        ("Indeterminate vertices", "3"),
    ] {
        assert_eq!(field(&output, key), expected, "{key}");
    }
    assert!(output.contains("triangle 12: degenerate; repeated vertex coordinates"));
    assert!(output.contains("triangle 13: degenerate; zero computed area"));
    assert!(output.contains("triangle 14: duplicate geometry of triangle 0"));
    assert!(output.contains("vertex 8: indeterminate"));
}

#[test]
fn reversed_face_reports_local_winding_failure_despite_closed_combinatorics() {
    let output = success(&inspect(&fixture("FX-09", None)));
    assert_eq!(field(&output, "Winding conflicts"), "3");
    assert_eq!(field(&output, "Local winding check"), "failed");
    assert_eq!(field(&output, "Closed combinatorial surface"), "yes");
    assert!(output.contains("winding conflict between triangles"));
    assert!(field(&output, "Printability").starts_with("not assessed"));
}

#[test]
fn open_mesh_has_boundary_findings_without_repair() {
    let output = success(&inspect(&fixture("FX-10", Some("A"))));
    assert_eq!(field(&output, "Triangles"), "10");
    assert_eq!(field(&output, "Boundary edges"), "4");
    assert_eq!(field(&output, "Closed combinatorial surface"), "no");
    assert!(output.contains(": boundary\n"));
}

#[test]
fn nonmanifold_edge_and_vertex_findings_are_exposed() {
    let output = success(&inspect(&fixture("FX-10", Some("B"))));
    assert_eq!(field(&output, "Triangles"), "13");
    assert_eq!(field(&output, "Non-manifold edges"), "1");
    assert_eq!(field(&output, "Boundary edges"), "2");
    assert_eq!(field(&output, "Non-manifold vertices"), "2");
    assert!(output.contains(": non-manifold; branched link"));
}

#[test]
fn disconnected_components_are_reported_under_both_definitions() {
    let output = success(&inspect(&fixture("FX-06", None)));
    assert_eq!(field(&output, "Edge-connected components"), "2");
    assert_eq!(field(&output, "Vertex-connected components"), "2");
}

#[test]
fn retained_normal_and_attribute_findings_are_not_hidden() {
    let temp = TempDirectory::new();
    let mut bytes = fs::read(fixture("FX-01", None)).unwrap();
    bytes[84..88].copy_from_slice(&f32::NAN.to_le_bytes());
    bytes[134..146].fill(0);
    for offset in [184, 188, 192] {
        let value = f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        bytes[offset..offset + 4].copy_from_slice(&(-value).to_le_bytes());
    }
    bytes[282..284].copy_from_slice(&0xbeef_u16.to_le_bytes());
    let output = success(&inspect(&temp.write("metadata.stl", bytes)));
    for name in [
        "Non-finite stored normals",
        "Zero stored normals",
        "Opposing stored normals",
        "Nonzero attributes",
    ] {
        assert_eq!(field(&output, name), "1", "{name}");
    }
    assert!(output.contains("triangle 0: non-finite stored normal (retained)"));
    assert!(output.contains("triangle 3: opaque nonzero attribute bits 48879 (retained)"));
}

#[test]
fn detail_limits_keep_accurate_totals_and_explicit_omitted_counts() {
    let temp = TempDirectory::new();
    let mut text = String::from("solid many\n");
    for _ in 0..201 {
        text.push_str("facet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 0 0 0\nvertex 1 0 0\nendloop\nendfacet\n");
    }
    text.push_str("endsolid many\n");
    let output = success(&inspect(&temp.write("many-defects.stl", text)));
    for (key, expected) in [
        ("Triangles", "201"),
        ("Degenerate triangles", "201"),
        ("Duplicate triangles", "200"),
        ("Import diagnostics total", "401"),
        ("Import diagnostic details shown", "100 (limit 100)"),
        ("Import diagnostic details omitted", "301"),
        ("Topology issues total", "203"),
        ("Topology issue details shown", "100 (limit 100)"),
        ("Topology issue details omitted", "103"),
    ] {
        assert_eq!(field(&output, key), expected, "{key}");
    }
    assert_eq!(
        output.lines().filter(|line| line.starts_with("  ")).count(),
        200
    );
}

#[test]
fn missing_file_is_exit_one_with_context_and_empty_stdout() {
    let temp = TempDirectory::new();
    let error = failure(&inspect(&temp.0.join("missing.stl")), 1);
    assert!(error.contains("cannot import STL"));
    assert!(error.contains("missing.stl"));
    assert!(error.contains("STL I/O failure"));
}

#[test]
fn malformed_ascii_and_binary_fail_without_partial_stdout_or_panics() {
    let temp = TempDirectory::new();
    for (name, bytes) in [
        (
            "bad-ascii.stl",
            b"solid bad\nfacet normal invalid\n".as_slice(),
        ),
        ("bad-binary.stl", [0_u8; 84].as_slice()),
        ("empty-input.stl", b"".as_slice()),
    ] {
        let mut bytes = bytes.to_vec();
        if name == "bad-binary.stl" {
            bytes[80..84].copy_from_slice(&1_u32.to_le_bytes());
        }
        let error = failure(&inspect(&temp.write(name, bytes)), 1);
        assert!(error.contains("cannot import STL"));
    }
}

#[test]
fn missing_explicit_unit_policy_is_usage_error() {
    assert!(failure(&run(["inspect", "model.stl"]), 2).contains("--assume-mm"));
}

#[test]
fn missing_path_is_usage_error() {
    assert!(failure(&run(["inspect", "--assume-mm"]), 2).contains("input path"));
    failure(&run(["inspect"]), 2);
}

#[test]
fn unexpected_options_extra_paths_empty_paths_and_wrong_order_are_rejected() {
    for args in [
        vec!["inspect", "--assume-mm", "one.stl", "two.stl"],
        vec!["inspect", "--assume-mm", "--repair"],
        vec!["inspect", "--assume-mm", ""],
        vec!["inspect", "one.stl", "--assume-mm"],
        vec!["inspect", "--assume-mm", "one.stl", "--repair"],
        vec!["--help", "extra"],
        vec!["--version", "extra"],
    ] {
        failure(&run(args), 2);
    }
}

#[test]
fn absolute_path_with_spaces_is_one_native_argument() {
    let temp = TempDirectory::new();
    let path = temp.write("source mesh with spaces.stl", ASCII);
    assert!(path.is_absolute());
    let output = success(&inspect(&path));
    assert!(field(&output, "Input path").contains("source mesh with spaces.stl"));
    assert_eq!(field(&output, "Triangles"), "1");
}

#[test]
fn relative_input_path_works_without_changing_process_wide_current_directory() {
    let root = root();
    let path = fixture("FX-01", None);
    let relative = path.strip_prefix(&root).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_karasu-cli"))
        .current_dir(&root)
        .args(["inspect", "--assume-mm"])
        .arg(relative)
        .output()
        .unwrap();
    assert_eq!(field(&success(&output), "Triangles"), "12");
}

#[test]
fn leading_hyphen_filename_works_with_documented_relative_prefix() {
    let temp = TempDirectory::new();
    temp.write("-mesh.stl", ASCII);
    let output = Command::new(env!("CARGO_BIN_EXE_karasu-cli"))
        .current_dir(&temp.0)
        .args(["inspect", "--assume-mm", "./-mesh.stl"])
        .output()
        .unwrap();
    assert_eq!(field(&success(&output), "Triangles"), "1");
}

#[test]
fn repeated_output_is_identical_and_field_order_is_stable() {
    let path = fixture("FX-08", None);
    let first = inspect(&path);
    let second = inspect(&path);
    assert_eq!(first.stdout, second.stdout);
    assert_eq!(first.stderr, second.stderr);
    let output = success(&first);
    let mut previous = 0;
    for label in [
        "Input path:",
        "STL format:",
        "Assumed physical units:",
        "Coordinate space:",
        "Triangles:",
        "Indexed vertices:",
        "Source minimum XYZ",
        "Source maximum XYZ",
        "Source dimensions XYZ",
        "Import diagnostics total:",
        "Topology analysis:",
        "Usable triangles:",
        "Edges:",
        "Winding conflicts:",
        "Topology issues total:",
        "Printability:",
    ] {
        let index = output.find(label).unwrap();
        assert!(index >= previous, "{label}");
        previous = index;
    }
}

#[test]
fn all_canonical_input_bytes_and_imported_geometry_are_unchanged() {
    let policy = StlOptions::new(StlUnitPolicy::AssumeMillimeters);
    for (id, variant) in [
        ("FX-01", None),
        ("FX-02", None),
        ("FX-03", None),
        ("FX-04", None),
        ("FX-05", None),
        ("FX-06", None),
        ("FX-07", None),
        ("FX-08", None),
        ("FX-09", None),
        ("FX-10", Some("A")),
        ("FX-10", Some("B")),
    ] {
        let path = fixture(id, variant);
        let before = fs::read(&path).unwrap();
        let imported = import_stl_bytes(&before, policy).unwrap();
        success(&inspect(&path));
        let after = fs::read(path).unwrap();
        assert_eq!(before, after); // Exact byte equality also proves the file hash is unchanged.
        assert_eq!(imported, import_stl_bytes(&after, policy).unwrap());
    }
}

#[test]
fn help_default_and_both_version_aliases_preserve_the_existing_contract() {
    let help = success(&run(["--help"]));
    assert_eq!(help, success(&run(["-h"])));
    assert_eq!(help, success(&run(Vec::<&str>::new())));
    assert!(help.contains("Usage: karasu-cli [OPTIONS]"));
    assert!(help.contains("inspect --assume-mm <path>"));
    let version = success(&run(["--version"]));
    assert!(version.starts_with("karasu-slicer 0.1.0\nCopyright (C) 2026 Ivan Maruca\n"));
    assert!(version.contains("License AGPL-3.0-or-later"));
    assert!(version.contains("NO WARRANTY"));
    assert_eq!(success(&run(["-V"])), success(&run(["--version"])));
}

#[test]
fn slice_is_a_subcommand_not_an_option() {
    assert!(failure(&run(["--slice"]), 2).contains("unknown argument"));
    assert!(failure(&run(["slice", "model.stl"]), 2).contains("unknown slice option"));
}

#[cfg(unix)]
#[test]
fn non_unicode_paths_are_preserved_without_argument_panics() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let temp = TempDirectory::new();
    let path = temp.write(OsString::from_vec(b"mesh-\xff.stl".to_vec()), ASCII);
    let output = success(&inspect(&path));
    assert_eq!(field(&output, "Triangles"), "1");
    assert!(field(&output, "Input path").contains("\\xFF"));
    failure(&run([OsString::from_vec(vec![0xff])]), 2);
}

#[test]
fn former_cli_executable_preserves_commands_outputs_and_exit_statuses() {
    let cube = root().join("fixtures/reference/stl/fx-01-cube.stl");
    let cases: Vec<Vec<std::ffi::OsString>> = vec![
        vec![],
        vec!["--help".into()],
        vec!["--version".into()],
        vec!["--unknown".into()],
        vec![
            "inspect".into(),
            "--assume-mm".into(),
            cube.into_os_string(),
        ],
        vec![
            "inspect".into(),
            "--assume-mm".into(),
            "missing-karasu-input.stl".into(),
        ],
    ];
    for arguments in cases {
        let primary = run(&arguments);
        let legacy = Command::new(env!("CARGO_BIN_EXE_resin-cli"))
            .args(&arguments)
            .output()
            .unwrap();
        assert_eq!(legacy.status.code(), primary.status.code());
        assert_eq!(legacy.stdout, primary.stdout);
        assert_eq!(legacy.stderr, primary.stderr);
    }
}

#[test]
fn the_extended_diagnosis_is_appended_on_request_only() {
    let fin = fixture("FX-10", Some("B"));
    let plain = success(&inspect(&fin));
    let extended = success(&run([
        OsStr::new("inspect"),
        OsStr::new("--assume-mm"),
        fin.as_os_str(),
        OsStr::new("--diagnosis"),
    ]));
    // The plain report is unchanged and comes first.
    assert!(!plain.contains("Extended diagnosis"));
    let section = extended.strip_prefix(plain.as_str()).unwrap();
    assert!(
        section.starts_with("\nExtended diagnosis: complete\n"),
        "{section}"
    );
    assert_eq!(field(section, "Cap faces (three corners on a line)"), "0");
    // The fin lies in the plane of a box face, overlapping it.
    assert_eq!(field(section, "Self-intersecting face pairs"), "2");
    assert!(section.contains("  non-manifold edge at "), "{section}");
    assert!(section.contains("  hole left open at "), "{section}");
    // The fin makes layers fail: exact repair cannot remove it, the standard preset does.
    assert!(
        field(section, "Printability after exact repair").starts_with("needs repair"),
        "{section}"
    );
    assert_eq!(
        field(section, "Printability after standard repair"),
        "ready"
    );
    assert!(section.contains("  fin removed at "), "{section}");
    // A clean cube is ready, before and after, and the option is given once.
    let cube = success(&run([
        OsStr::new("inspect"),
        OsStr::new("--diagnosis"),
        OsStr::new("--assume-mm"),
        fixture("FX-01", None).as_os_str(),
    ]));
    assert_eq!(
        field(&cube, "Printability as is (union, holes closed in layers)"),
        "ready"
    );
    assert_eq!(field(&cube, "Exact repair locations shown"), "0");
    assert_eq!(field(&cube, "Printability after standard repair"), "ready");
    let twice = failure(
        &run([
            OsStr::new("inspect"),
            OsStr::new("--diagnosis"),
            OsStr::new("--diagnosis"),
            OsStr::new("--assume-mm"),
            fixture("FX-01", None).as_os_str(),
        ]),
        2,
    );
    assert!(twice.contains("only once"), "{twice}");
}

/// An ASCII STL of outward triangles, each three vertices.
fn ascii_stl(triangles: &[[[f64; 3]; 3]]) -> String {
    use std::fmt::Write as _;
    let mut text = String::from("solid minima\n");
    for triangle in triangles {
        text.push_str("facet normal 0 0 0\nouter loop\n");
        for [x, y, z] in triangle {
            let _ = writeln!(text, "vertex {x} {y} {z}");
        }
        text.push_str("endloop\nendfacet\n");
    }
    text.push_str("endsolid minima\n");
    text
}

#[test]
fn minima_report_where_material_starts_in_mid_air() {
    let temp = TempDirectory::new();
    // A cube on the plate (0..2 mm) and, beside it, a pyramid pointing down with its tip 1 mm
    // up: the cube's bottom rests on the plate, the tip starts in mid-air.
    let c = |x: f64, y: f64, z: f64| [x, y, z];
    let (l, h) = (0.0, 2.0);
    let cube = [
        [c(l, l, l), c(h, h, l), c(h, l, l)],
        [c(l, l, l), c(l, h, l), c(h, h, l)],
        [c(l, l, h), c(h, l, h), c(h, h, h)],
        [c(l, l, h), c(h, h, h), c(l, h, h)],
        [c(l, l, l), c(h, l, l), c(h, l, h)],
        [c(l, l, l), c(h, l, h), c(l, l, h)],
        [c(h, l, l), c(h, h, l), c(h, h, h)],
        [c(h, l, l), c(h, h, h), c(h, l, h)],
        [c(h, h, l), c(l, h, l), c(l, h, h)],
        [c(h, h, l), c(l, h, h), c(h, h, h)],
        [c(l, h, l), c(l, l, l), c(l, l, h)],
        [c(l, h, l), c(l, l, h), c(l, h, h)],
    ];
    let (b0, b1, top, tip) = (5.0, 7.0, 3.0, 1.0);
    let apex = c(6.0, 1.0, tip);
    let pyramid = [
        [c(b0, 0.0, top), c(b1, 0.0, top), c(b1, 2.0, top)],
        [c(b0, 0.0, top), c(b1, 2.0, top), c(b0, 2.0, top)],
        [c(b0, 0.0, top), apex, c(b1, 0.0, top)],
        [c(b1, 0.0, top), apex, c(b1, 2.0, top)],
        [c(b1, 2.0, top), apex, c(b0, 2.0, top)],
        [c(b0, 2.0, top), apex, c(b0, 0.0, top)],
    ];
    let triangles: Vec<_> = cube.iter().chain(pyramid.iter()).copied().collect();
    let path = temp.write("minima.stl", ascii_stl(&triangles));
    let text = success(&run([
        OsStr::new("inspect"),
        OsStr::new("--assume-mm"),
        path.as_os_str(),
        OsStr::new("--minima"),
    ]));
    assert_eq!(field(&text, "Minima"), "2 (1 on the plate, 1 in mid-air)");
    assert!(
        text.contains("x 1.0000 y 1.0000 z 0.0000 mm, 4 vertex(es), on the plate"),
        "{text}"
    );
    assert!(
        text.contains("x 6.0000 y 1.0000 z 1.0000 mm, 1 vertex(es)\n"),
        "{text}"
    );
    // Turned upside down (180 degrees about X), the pyramid's base is lowest and rests on the
    // plate, and the cube floats 1 mm up: its face is a level group in mid-air.
    let turned = success(&run([
        OsStr::new("inspect"),
        OsStr::new("--assume-mm"),
        path.as_os_str(),
        OsStr::new("--minima"),
        OsStr::new("--rotate"),
        OsStr::new("180"),
        OsStr::new("0"),
        OsStr::new("0"),
    ]));
    assert!(
        turned.contains("Placement: turned 180 0 0 degrees"),
        "{turned}"
    );
    assert_eq!(field(&turned, "Minima"), "2 (1 on the plate, 1 in mid-air)");
    // Misused options are refused.
    let error = failure(
        &run([
            OsStr::new("inspect"),
            OsStr::new("--assume-mm"),
            path.as_os_str(),
            OsStr::new("--rotate"),
            OsStr::new("1"),
            OsStr::new("2"),
            OsStr::new("3"),
        ]),
        2,
    );
    assert!(
        error.contains("--rotate is used only with --minima"),
        "{error}"
    );
}
