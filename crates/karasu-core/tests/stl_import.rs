//! External-input structure, limits, metadata, and deterministic importer regressions.

use karasu_core::geometry::{Point3, Vector3};
use karasu_core::mesh::stl::{
    import_stl_bytes, import_stl_path, import_stl_reader, DroppedFacets, NonFinitePolicy,
    StlFormat, StlHeader, StlOptions, StlUnitPolicy, MAX_DROPPED_RUNS,
};
use karasu_core::mesh::{Degeneracy, FaceNormal, ImportResource, MeshDiagnostic, StlError};
use std::{error::Error, io};

fn options() -> StlOptions {
    StlOptions::new(StlUnitPolicy::AssumeMillimeters)
}

fn forced(format: StlFormat) -> StlOptions {
    StlOptions {
        format,
        ..options()
    }
}

fn binary(points: &[[f32; 3]], normal: [f32; 3], attribute: u16) -> Vec<u8> {
    let mut bytes = vec![0; 80];
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    for value in normal.into_iter().chain(points.iter().flatten().copied()) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&attribute.to_le_bytes());
    bytes
}

fn valid_binary() -> Vec<u8> {
    binary(
        &[[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 3.0, 0.0]],
        [0.0, 0.0, 1.0],
        0,
    )
}

const ASCII: &str = "solid example\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 2 0 0\nvertex 0 3 0\nendloop\nendfacet\nendsolid example\n";

#[test]
fn binary_decodes_little_endian_geometry_and_preserves_opaque_header() {
    let mut bytes = valid_binary();
    bytes[..6].copy_from_slice(b"opaque");
    let result = import_stl_bytes(&bytes, options()).unwrap();
    assert_eq!(result.mesh.triangles(), [[0, 1, 2]]);
    assert_eq!(
        result.mesh.vertices(),
        [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 3.0, 0.0)
        ]
    );
    assert_eq!(result.diagnostics, []);
    let StlHeader::Binary(header) = result.metadata.header else {
        panic!("binary header expected")
    };
    assert_eq!(&header[..6], b"opaque");
    assert_eq!(
        result.metadata.facets[0].stored_normal,
        Vector3::new(0.0, 0.0, 1.0)
    );
    assert_eq!(result.metadata.units, StlUnitPolicy::AssumeMillimeters);
}

#[test]
fn empty_input_is_not_an_empty_mesh() {
    for format in [StlFormat::Auto, StlFormat::Binary, StlFormat::Ascii] {
        assert!(matches!(
            import_stl_bytes(&[], forced(format)),
            Err(StlError::EmptyInput)
        ));
    }
}

#[test]
fn explicit_empty_binary_and_ascii_solids_are_valid() {
    let bytes = vec![0; 84];
    for result in [
        import_stl_bytes(&bytes, options()),
        import_stl_bytes(b"solid\nendsolid\n", options()),
    ] {
        let result = result.unwrap();
        assert_eq!(result.mesh.statistics().triangle_count, 0);
        assert_eq!(result.mesh.source_bounds(), None);
    }
}

#[test]
fn every_binary_prefix_is_rejected_without_panic_or_partial_mesh() {
    let bytes = valid_binary();
    for length in 1..bytes.len() {
        assert!(
            matches!(
                import_stl_bytes(&bytes[..length], forced(StlFormat::Binary)),
                Err(StlError::Truncated { .. })
            ),
            "prefix {length}"
        );
    }
}

#[test]
fn truncated_header_and_count_report_required_lengths() {
    assert!(matches!(
        import_stl_bytes(&[0; 79], options()),
        Err(StlError::Truncated {
            expected: 80,
            actual: 79
        })
    ));
    assert!(matches!(
        import_stl_bytes(&[0; 82], options()),
        Err(StlError::Truncated {
            expected: 84,
            actual: 82
        })
    ));
}

#[test]
fn excessive_declared_count_is_rejected_before_allocation() {
    let mut bytes = vec![0; 84];
    bytes[80..84].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(
        import_stl_bytes(&bytes, options()),
        Err(StlError::LimitExceeded {
            resource: ImportResource::Triangles,
            ..
        } | StlError::SizeOverflow)
    ));
}

#[test]
fn declared_count_mismatch_is_not_silently_repaired() {
    let mut bytes = valid_binary();
    bytes[80..84].copy_from_slice(&2_u32.to_le_bytes());
    assert!(matches!(
        import_stl_bytes(&bytes, options()),
        Err(StlError::Truncated {
            expected: 184,
            actual: 134
        })
    ));
}

#[test]
fn ascii_looking_binary_header_uses_structural_detection() {
    let mut bytes = valid_binary();
    bytes[..11].copy_from_slice(b"solid model");
    assert!(matches!(
        import_stl_bytes(&bytes, options()).unwrap().metadata.header,
        StlHeader::Binary(_)
    ));
    assert!(import_stl_bytes(&bytes, forced(StlFormat::Ascii)).is_err());
}

#[test]
fn binary_nonfinite_vertices_have_facet_corner_axis_context() {
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        for corner in 0..3 {
            for axis in 0..3 {
                let mut points = [[0.0; 3]; 3];
                points[corner][axis] = value;
                assert!(
                    matches!(import_stl_bytes(&binary(&points, [0.0; 3], 0), options()), Err(StlError::NonFiniteCoordinate { facet: 0, corner: c, axis: a }) if c == corner && a == axis)
                );
            }
        }
    }
}

#[test]
fn nonzero_binary_attributes_are_retained_and_diagnosed() {
    let bytes = binary(&[[0.0; 3]; 3], [0.0, 0.0, 1.0], 0x8431);
    let result = import_stl_bytes(&bytes, options()).unwrap();
    assert_eq!(result.metadata.facets[0].attribute, Some(0x8431));
    assert!(result
        .diagnostics
        .contains(&MeshDiagnostic::NonzeroAttribute {
            triangle: 0,
            value: 0x8431
        }));
}

#[test]
fn binary_trailing_bytes_including_whitespace_are_rejected() {
    for suffix in [&b"\n"[..], &b"extra"[..]] {
        let mut bytes = valid_binary();
        bytes.extend_from_slice(suffix);
        assert!(matches!(
            import_stl_bytes(&bytes, options()),
            Err(StlError::TrailingBytes { expected: 134, .. })
        ));
    }
}

#[test]
fn zero_and_nonfinite_stored_normals_do_not_invalidate_finite_vertices() {
    for (normal, expected) in [
        ([0.0; 3], MeshDiagnostic::ZeroStoredNormal { triangle: 0 }),
        (
            [f32::NAN, 0.0, 0.0],
            MeshDiagnostic::NonFiniteStoredNormal { triangle: 0 },
        ),
    ] {
        let bytes = binary(&[[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], normal, 0);
        let result = import_stl_bytes(&bytes, options()).unwrap();
        assert_eq!(result.diagnostics, [expected]);
        assert_eq!(
            result.mesh.triangle_normal(0),
            Some(FaceNormal::Unit(Vector3::new(0.0, 0.0, 1.0)))
        );
    }
}

#[test]
fn opposing_stored_normal_is_reported_without_reversing_winding() {
    let bytes = binary(
        &[[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        [0.0, 0.0, -7.0],
        0,
    );
    let result = import_stl_bytes(&bytes, options()).unwrap();
    assert_eq!(result.mesh.triangles(), [[0, 1, 2]]);
    assert_eq!(
        result.metadata.facets[0].stored_normal,
        Vector3::new(0.0, 0.0, -7.0)
    );
    assert_eq!(
        result.diagnostics,
        [MeshDiagnostic::OpposingStoredNormal { triangle: 0 }]
    );
}

#[test]
fn conventional_ascii_imports_the_same_geometry_as_binary() {
    let ascii = import_stl_bytes(ASCII.as_bytes(), options()).unwrap();
    let binary = import_stl_bytes(&valid_binary(), options()).unwrap();
    assert_eq!(ascii.mesh, binary.mesh);
    assert_eq!(
        ascii.metadata.header,
        StlHeader::Ascii {
            name: "example".into(),
            end_name: "example".into()
        }
    );
    assert_eq!(ascii.metadata.facets[0].attribute, None);
}

#[test]
fn ascii_accepts_blank_lines_tabs_crlf_signed_and_scientific_numbers() {
    let text = " \r\n solid multiple word name\r\n\tfacet\t normal +0 -0 +1e0\r\n outer   loop\r\n\nvertex -2.5E+1 +0.0 1e-3\r\nvertex 0 +2 0\r\nvertex +3 0 -4\r\nendloop\r\nendfacet\r\nendsolid different label\r\n\r\n";
    let result = import_stl_bytes(text.as_bytes(), options()).unwrap();
    assert_eq!(result.mesh.vertices()[0], Point3::new(-25.0, 0.0, 0.001));
    assert_eq!(
        result.metadata.header,
        StlHeader::Ascii {
            name: "multiple word name".into(),
            end_name: "different label".into()
        }
    );
}

#[test]
fn ascii_facet_structure_errors_are_not_skipped() {
    for (from, to) in [
        ("outer loop", "loop outer"),
        ("endloop", "endfacet"),
        ("endfacet", "unknown"),
        ("vertex 0 3 0\n", ""),
        ("vertex 0 3 0", "vertex 0 3 0 4"),
        ("facet normal", "facet normals"),
    ] {
        let text = ASCII.replace(from, to);
        assert!(
            matches!(
                import_stl_bytes(text.as_bytes(), forced(StlFormat::Ascii)),
                Err(StlError::AsciiSyntax { .. })
            ),
            "replacement {from} -> {to}"
        );
    }
}

#[test]
fn ascii_invalid_numbers_and_decimal_underflow_are_reported() {
    for value in ["not-a-number", "0x1", "1e-9999"] {
        let text = ASCII.replace("vertex 2 0 0", &format!("vertex {value} 0 0"));
        assert!(matches!(
            import_stl_bytes(text.as_bytes(), options()),
            Err(StlError::InvalidNumber { line: 5 })
        ));
    }
}

#[test]
fn ascii_nonfinite_coordinates_and_overflow_are_rejected() {
    for value in ["NaN", "inf", "-inf", "1e9999"] {
        let text = ASCII.replace("vertex 2 0 0", &format!("vertex {value} 0 0"));
        assert!(matches!(
            import_stl_bytes(text.as_bytes(), options()),
            Err(StlError::NonFiniteCoordinate {
                facet: 0,
                corner: 1,
                axis: 0
            })
        ));
    }
}

/// A binary STL of `count` facets, each a unit triangle shifted by its number along X, with
/// every facet in `bad` given a NaN at corner 1, axis 2, and attribute = facet number.
fn binary_with_nan(count: usize, bad: &[usize]) -> Vec<u8> {
    let mut bytes = vec![0; 80];
    bytes.extend_from_slice(&u32::try_from(count).unwrap().to_le_bytes());
    for facet in 0..count {
        #[allow(clippy::cast_precision_loss)]
        let x = facet as f32;
        let mut corners = [[x, 0.0, 0.0], [x + 1.0, 0.0, 0.0], [x, 1.0, 0.0]];
        if bad.contains(&facet) {
            corners[1][2] = f32::NAN;
        }
        for value in [0.0, 0.0, 1.0]
            .into_iter()
            .chain(corners.into_iter().flatten())
        {
            bytes.extend_from_slice(&f32::to_le_bytes(value));
        }
        bytes.extend_from_slice(&u16::try_from(facet).unwrap().to_le_bytes());
    }
    bytes
}

fn dropping() -> StlOptions {
    StlOptions {
        non_finite: NonFinitePolicy::Drop,
        ..options()
    }
}

#[test]
fn nonfinite_facets_are_dropped_and_reported_only_on_request() {
    let bytes = binary_with_nan(10, &[2, 3, 4, 7]);
    // Refused by default, as before, at the first such coordinate.
    let refused = import_stl_bytes(&bytes, options()).unwrap_err();
    assert!(matches!(
        refused,
        StlError::NonFiniteCoordinate {
            facet: 2,
            corner: 1,
            axis: 2
        }
    ));
    let imported = import_stl_bytes(&bytes, dropping()).unwrap();
    let dropped = &imported.dropped;
    assert_eq!(
        (dropped.facets, dropped.runs, dropped.first),
        (4, 2, Some([2, 1, 2]))
    );
    assert_eq!(dropped.ranges, [(2, 3), (7, 1)]);
    // The refusal the default would give, word for word.
    assert_eq!(dropped.refusal().unwrap().to_string(), refused.to_string());
    // The kept facets in order, with their own metadata.
    assert_eq!(imported.mesh.triangles().len(), 6);
    let attributes: Vec<_> = imported
        .metadata
        .facets
        .iter()
        .map(|f| f.attribute.unwrap())
        .collect();
    assert_eq!(attributes, [0, 1, 5, 6, 8, 9]);
    let first_x: Vec<f64> = imported
        .mesh
        .triangles()
        .iter()
        .map(|t| imported.mesh.vertices()[t[0] as usize].x)
        .collect();
    assert_eq!(first_x, [0.0, 1.0, 5.0, 6.0, 8.0, 9.0]);
    // A file without such facets drops nothing and imports as before.
    let clean = binary_with_nan(3, &[]);
    let kept = import_stl_bytes(&clean, dropping()).unwrap();
    assert_eq!(kept.dropped, DroppedFacets::default());
    assert_eq!(kept, import_stl_bytes(&clean, options()).unwrap());
}

#[test]
fn dropped_runs_are_listed_up_to_a_bound_and_counted_beyond() {
    let bad: Vec<usize> = (0..40).map(|i| 3 * i).collect();
    let imported = import_stl_bytes(&binary_with_nan(120, &bad), dropping()).unwrap();
    let dropped = imported.dropped;
    assert_eq!((dropped.facets, dropped.runs), (40, 40));
    assert_eq!(dropped.ranges.len(), MAX_DROPPED_RUNS);
    assert_eq!(dropped.ranges[MAX_DROPPED_RUNS - 1], (45, 1));
    // Every facet dropped leaves an empty mesh, not an error.
    let all: Vec<usize> = (0..5).collect();
    let empty = import_stl_bytes(&binary_with_nan(5, &all), dropping()).unwrap();
    assert_eq!(empty.mesh.triangles().len(), 0);
    assert_eq!((empty.dropped.facets, empty.dropped.runs), (5, 1));
}

#[test]
fn ascii_nonfinite_facets_are_dropped_on_request() {
    let facet = |third: &str| {
        format!(
            "facet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 2 0 0\nvertex {third}\nendloop\nendfacet\n"
        )
    };
    let second = format!(
        "solid example\n{}{}{}endsolid example\n",
        facet("0 3 0"),
        facet("0 inf 0"),
        facet("0 3 5")
    );
    let refused = import_stl_bytes(second.as_bytes(), options()).unwrap_err();
    assert!(matches!(
        refused,
        StlError::NonFiniteCoordinate {
            facet: 1,
            corner: 2,
            axis: 1
        }
    ));
    let imported = import_stl_bytes(second.as_bytes(), dropping()).unwrap();
    assert_eq!(imported.mesh.triangles().len(), 2);
    assert_eq!(imported.metadata.facets.len(), 2);
    assert_eq!(imported.dropped.ranges, [(1, 1)]);
    assert_eq!(
        imported.dropped.refusal().unwrap().to_string(),
        refused.to_string()
    );
}

#[test]
fn incomplete_ascii_and_trailing_records_fail_with_line_context() {
    for marker in [
        "outer loop",
        "vertex 0 3 0",
        "endloop",
        "endfacet",
        "endsolid",
    ] {
        let end = ASCII.find(marker).unwrap();
        assert!(matches!(
            import_stl_bytes(&ASCII.as_bytes()[..end], options()),
            Err(StlError::AsciiSyntax { .. })
        ));
    }
    let multiple = format!("{ASCII}{ASCII}");
    assert!(matches!(
        import_stl_bytes(multiple.as_bytes(), options()),
        Err(StlError::AsciiSyntax { line: 10, .. })
    ));
}

#[test]
fn ascii_degenerate_geometry_is_preserved() {
    let text = ASCII.replace("vertex 0 3 0", "vertex 2 0 0");
    let result = import_stl_bytes(text.as_bytes(), options()).unwrap();
    assert_eq!(result.mesh.statistics().triangle_count, 1);
    assert_eq!(
        result.diagnostics,
        [MeshDiagnostic::DegenerateTriangle {
            triangle: 0,
            kind: Degeneracy::RepeatedVertex
        }]
    );
}

#[test]
fn exact_dedup_normalizes_signed_zero_but_not_nearby_coordinates() {
    let text = ASCII
        .replace("vertex 2 0 0", "vertex -0 -0 -0")
        .replace("vertex 0 3 0", "vertex 1e-100 0 0");
    let result = import_stl_bytes(text.as_bytes(), options()).unwrap();
    assert_eq!(result.mesh.statistics().vertex_count, 2);
    assert_eq!(result.mesh.triangles(), [[0, 0, 1]]);
    assert_eq!(
        result.mesh.vertices()[0].coordinates().map(f64::to_bits),
        [0, 0, 0]
    );
}

#[test]
fn triangle_vertex_byte_line_and_numeric_limits_are_enforced() {
    for (resource, mut policy) in [
        (ImportResource::Triangles, options()),
        (ImportResource::Vertices, options()),
        (ImportResource::InputBytes, options()),
        (ImportResource::AsciiLineBytes, options()),
        (ImportResource::NumericTokenBytes, options()),
    ] {
        match resource {
            ImportResource::Triangles => policy.limits.max_triangles = 0,
            ImportResource::Vertices => policy.limits.max_vertices = 2,
            ImportResource::InputBytes => policy.limits.max_input_bytes = ASCII.len() - 1,
            ImportResource::AsciiLineBytes => policy.limits.max_ascii_line_bytes = 10,
            ImportResource::NumericTokenBytes => policy.limits.max_numeric_token_bytes = 0,
        }
        assert!(
            matches!(import_stl_bytes(ASCII.as_bytes(), policy), Err(StlError::LimitExceeded { resource: found, .. }) if found == resource)
        );
    }
    let mut policy = options();
    policy.limits.max_triangles = 0;
    assert!(matches!(
        import_stl_bytes(&valid_binary(), policy),
        Err(StlError::LimitExceeded {
            resource: ImportResource::Triangles,
            ..
        })
    ));
}

struct CountingReader {
    count: usize,
}
impl io::Read for CountingReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        buffer.fill(0);
        self.count += buffer.len();
        Ok(buffer.len())
    }
}

#[test]
fn reader_stops_at_byte_limit_plus_one() {
    let mut reader = CountingReader { count: 0 };
    let mut policy = options();
    policy.limits.max_input_bytes = 23;
    assert!(matches!(
        import_stl_reader(&mut reader, policy),
        Err(StlError::LimitExceeded {
            resource: ImportResource::InputBytes,
            limit: 23
        })
    ));
    assert_eq!(reader.count, 24);
}

#[test]
fn reader_and_byte_imports_are_identical() {
    assert_eq!(
        import_stl_reader(io::Cursor::new(ASCII.as_bytes()), options()).unwrap(),
        import_stl_bytes(ASCII.as_bytes(), options()).unwrap()
    );
}

struct BrokenReader;
impl io::Read for BrokenReader {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "test reader failure",
        ))
    }
}

#[test]
fn io_causes_are_preserved() {
    let error = import_stl_reader(BrokenReader, options()).unwrap_err();
    assert!(matches!(&error, StlError::Io(e) if e.kind() == io::ErrorKind::PermissionDenied));
    assert!(error.source().is_some());
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/nonexistent-mesh.stl");
    assert!(matches!(
        import_stl_path(path, options()),
        Err(StlError::Io(_))
    ));
}

#[test]
fn unknown_and_nonascii_inputs_fail_without_parser_fallback() {
    assert!(matches!(
        import_stl_bytes(b"not an STL", options()),
        Err(StlError::UnrecognizedFormat)
    ));
    assert!(import_stl_bytes(b"solid\n\xff\nendsolid", forced(StlFormat::Ascii)).is_err());
    assert!(import_stl_bytes(b"solid\n\0\nendsolid", forced(StlFormat::Ascii)).is_err());
    assert!(import_stl_bytes(ASCII.as_bytes(), forced(StlFormat::Binary)).is_err());
}

#[test]
fn repeated_imports_preserve_first_occurrence_order_and_diagnostics() {
    let first = import_stl_bytes(ASCII.as_bytes(), options()).unwrap();
    for _ in 0..10 {
        assert_eq!(
            import_stl_bytes(ASCII.as_bytes(), options()).unwrap(),
            first
        );
    }
}

#[test]
fn byte_limits_accept_exact_size_and_zero_limit_rejects_before_reading_payload() {
    let bytes = valid_binary();
    let mut policy = options();
    policy.limits.max_input_bytes = bytes.len();
    assert!(import_stl_reader(io::Cursor::new(&bytes), policy).is_ok());
    policy.limits.max_input_bytes = 0;
    let mut reader = CountingReader { count: 0 };
    assert!(matches!(
        import_stl_reader(&mut reader, policy),
        Err(StlError::LimitExceeded {
            resource: ImportResource::InputBytes,
            limit: 0
        })
    ));
    assert_eq!(reader.count, 1);
}

struct InterruptedReader(bool);
impl io::Read for InterruptedReader {
    fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        if self.0 {
            return Ok(0);
        }
        self.0 = true;
        Err(io::Error::from(io::ErrorKind::Interrupted))
    }
}

#[test]
fn interrupted_reader_is_retried_and_then_reports_empty_input() {
    assert!(matches!(
        import_stl_reader(InterruptedReader(false), options()),
        Err(StlError::EmptyInput)
    ));
}

#[test]
fn ascii_nonfinite_stored_normal_is_retained_as_metadata_not_vertex_geometry() {
    let text = ASCII.replace("normal 0 0 1", "normal NaN 0 1");
    let result = import_stl_bytes(text.as_bytes(), options()).unwrap();
    assert!(result.metadata.facets[0].stored_normal.x.is_nan());
    assert_eq!(
        result.diagnostics,
        [MeshDiagnostic::NonFiniteStoredNormal { triangle: 0 }]
    );
}

#[test]
fn deterministic_single_byte_mutations_do_not_panic() {
    // Bounded regression sweep, not a substitute for a sustained parser fuzzing campaign.
    for original in [valid_binary(), ASCII.as_bytes().to_vec()] {
        for index in 0..original.len() {
            for replacement in [0, b' ', b'\n', b'0', b'9', 127, 255] {
                let mut bytes = original.clone();
                bytes[index] = replacement;
                let _ = import_stl_bytes(&bytes, options());
            }
        }
    }
}
