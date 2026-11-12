//! Extensible model import (TD-057): STL, OBJ and 3MF through one registry, unit resolution, and
//! the behaviour of each format on valid, unusual and invalid files.
use karasu_core::{
    cancellation::Cancellation,
    mesh::{
        formats::{
            FormatRegistry, ImportNote, ImportedModel, LengthUnit, ModelFormat, ModelImportError,
            ModelImportOptions, ObjError, RegistryError, SourceModel, ThreeMfError, UnitOrigin,
        },
        stl::{import_stl_path, StlOptions, StlUnitPolicy},
        TriangleMesh,
    },
};
use std::{fmt::Write as _, path::Path};

fn import(
    format: &str,
    bytes: &[u8],
    unit: Option<LengthUnit>,
) -> Result<ImportedModel, ModelImportError> {
    let registry = FormatRegistry::builtin();
    let format = registry.by_id(format).unwrap();
    registry.import_bytes(
        format,
        bytes,
        &ModelImportOptions::new(unit),
        &mut Cancellation::never(),
    )
}

fn corpus(name: &str) -> TriangleMesh {
    import_stl_path(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/reference/stl")
            .join(name),
        StlOptions::new(StlUnitPolicy::AssumeMillimeters),
    )
    .unwrap()
    .mesh
}

/// Six times the signed volume.
fn volume6(mesh: &TriangleMesh) -> f64 {
    mesh.triangles()
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|v| mesh.vertices()[v as usize]);
            a.x * (b.y * c.z - b.z * c.y)
                + a.y * (b.z * c.x - b.x * c.z)
                + a.z * (b.x * c.y - b.y * c.x)
        })
        .sum()
}

/// OBJ text of a mesh, with round-trip exact coordinates.
fn obj_text(mesh: &TriangleMesh) -> String {
    let mut text = String::from("# exported for a test\no body\n");
    for p in mesh.vertices() {
        writeln!(text, "v {} {} {}", p.x, p.y, p.z).unwrap();
    }
    for t in mesh.triangles() {
        writeln!(text, "f {} {} {}", t[0] + 1, t[1] + 1, t[2] + 1).unwrap();
    }
    text
}

/// A 3MF model part with one mesh object per entry of `objects` and the given build items.
fn model_xml(unit: &str, extra: &str, objects: &[(u32, &TriangleMesh)], build: &str) -> String {
    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<model unit=\"{unit}\" xml:lang=\"en-US\" \
         xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\" {extra}>\n\
         <metadata name=\"Title\">A &amp; B</metadata>\n<resources>\n"
    );
    for (id, mesh) in objects {
        writeln!(xml, "<object id=\"{id}\" type=\"model\"><mesh><vertices>").unwrap();
        for p in mesh.vertices() {
            writeln!(xml, "<vertex x=\"{}\" y=\"{}\" z=\"{}\"/>", p.x, p.y, p.z).unwrap();
        }
        xml.push_str("</vertices><triangles>\n");
        for t in mesh.triangles() {
            writeln!(
                xml,
                "<triangle v1=\"{}\" v2=\"{}\" v3=\"{}\"/>",
                t[0], t[1], t[2]
            )
            .unwrap();
        }
        xml.push_str("</triangles></mesh></object>\n");
    }
    writeln!(xml, "</resources>\n<build>{build}</build>\n</model>").unwrap();
    xml
}

const RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Target=\"/3D/3dmodel.model\" Id=\"rel0\" \
Type=\"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel\"/></Relationships>";

const CONTENT_TYPES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"model\" ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodel+xml\"/>\
</Types>";

fn crc32(data: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// An independent minimal stored (uncompressed) ZIP writer.
fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let (mut out, mut central) = (Vec::new(), Vec::new());
    for (name, data) in entries {
        let offset = u32::try_from(out.len()).unwrap();
        let (size, crc) = (u32::try_from(data.len()).unwrap(), crc32(data));
        let name_length = u16::try_from(name.len()).unwrap();
        out.extend(0x0403_4b50u32.to_le_bytes());
        out.extend([20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        out.extend(crc.to_le_bytes());
        out.extend(size.to_le_bytes());
        out.extend(size.to_le_bytes());
        out.extend(name_length.to_le_bytes());
        out.extend([0, 0]);
        out.extend(name.as_bytes());
        out.extend(*data);
        central.extend(0x0201_4b50u32.to_le_bytes());
        central.extend([20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        central.extend(crc.to_le_bytes());
        central.extend(size.to_le_bytes());
        central.extend(size.to_le_bytes());
        central.extend(name_length.to_le_bytes());
        central.extend([0; 12]);
        central.extend(offset.to_le_bytes());
        central.extend(name.as_bytes());
    }
    let start = u32::try_from(out.len()).unwrap();
    out.extend(&central);
    let count = u16::try_from(entries.len()).unwrap();
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend([0, 0, 0, 0]);
    out.extend(count.to_le_bytes());
    out.extend(count.to_le_bytes());
    out.extend(u32::try_from(central.len()).unwrap().to_le_bytes());
    out.extend(start.to_le_bytes());
    out.extend([0, 0]);
    out
}

fn package(model: &str) -> Vec<u8> {
    zip(&[
        ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
        ("_rels/.rels", RELS.as_bytes()),
        ("3D/3dmodel.model", model.as_bytes()),
    ])
}

fn same_mesh(a: &TriangleMesh, b: &TriangleMesh) {
    assert_eq!(a.vertices(), b.vertices());
    assert_eq!(a.triangles(), b.triangles());
}

/// A format added from outside the crate: one triangle per file, declared in centimetres.
struct Triangle;
impl ModelFormat for Triangle {
    fn id(&self) -> &'static str {
        "tri"
    }
    fn name(&self) -> &'static str {
        "Triangle"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["tri"]
    }
    fn declares_units(&self) -> bool {
        true
    }
    fn parse(
        &self,
        _: &[u8],
        _: &ModelImportOptions,
        _: &mut Cancellation<'_>,
    ) -> Result<SourceModel, ModelImportError> {
        let mesh = TriangleMesh::try_new(
            vec![
                karasu_core::geometry::Point3::new(0., 0., 0.),
                karasu_core::geometry::Point3::new(1., 0., 0.),
                karasu_core::geometry::Point3::new(0., 1., 0.),
            ],
            vec![[0, 1, 2]],
        )
        .map_err(ModelImportError::Mesh)?;
        let diagnostics = mesh.diagnostics().map_err(ModelImportError::Mesh)?;
        Ok(SourceModel {
            mesh,
            declared_unit: Some(LengthUnit::Centimeter),
            objects: 1,
            notes: Vec::new(),
            diagnostics,
        })
    }
}

struct Clash;
impl ModelFormat for Clash {
    fn id(&self) -> &'static str {
        "clash"
    }
    fn name(&self) -> &'static str {
        "Clash"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["obj"]
    }
    fn declares_units(&self) -> bool {
        false
    }
    fn parse(
        &self,
        _: &[u8],
        _: &ModelImportOptions,
        _: &mut Cancellation<'_>,
    ) -> Result<SourceModel, ModelImportError> {
        unreachable!()
    }
}

#[test]
fn registry_selects_by_extension_and_refuses_conflicts() {
    let mut registry = FormatRegistry::builtin();
    let ids: Vec<_> = registry.formats().map(ModelFormat::id).collect();
    assert_eq!(ids, ["stl", "obj", "3mf"]);
    for (path, id) in [("a.STL", "stl"), ("b.Obj", "obj"), ("dir/c.3mf", "3mf")] {
        assert_eq!(registry.for_path(Path::new(path)).unwrap().id(), id);
    }
    assert!(matches!(
        registry.for_path(Path::new("model.ply")),
        Err(ModelImportError::UnknownFormat { extension: Some(e) }) if e == "ply"
    ));
    assert!(matches!(
        registry.for_path(Path::new("model")),
        Err(ModelImportError::UnknownFormat { extension: None })
    ));

    registry.register(Box::new(Triangle)).unwrap();
    let format = registry.for_path(Path::new("x.TRI")).unwrap();
    let imported = registry
        .import_bytes(
            format,
            b"",
            &ModelImportOptions::new(None),
            &mut Cancellation::never(),
        )
        .unwrap();
    assert_eq!(imported.mesh.vertices()[1].x, 10.);
    assert_eq!(
        registry.register(Box::new(Triangle)),
        Err(RegistryError::DuplicateId("tri".into()))
    );

    assert_eq!(
        registry.register(Box::new(Clash)),
        Err(RegistryError::DuplicateExtension("obj".into()))
    );
}

#[test]
fn one_cube_gives_one_mesh_in_every_format() {
    for name in [
        "fx-01-cube.stl",
        "fx-04-hollow-box.stl",
        "fx-06-multiple-volumes.stl",
    ] {
        let stl = corpus(name);
        let obj = import(
            "obj",
            obj_text(&stl).as_bytes(),
            Some(LengthUnit::Millimeter),
        )
        .unwrap();
        same_mesh(&obj.mesh, &stl);
        assert_eq!(obj.unit_origin, UnitOrigin::Assumed);
        let model = model_xml("millimeter", "", &[(1, &stl)], "<item objectid=\"1\"/>");
        let three_mf = import("3mf", &package(&model), None).unwrap();
        same_mesh(&three_mf.mesh, &stl);
        assert_eq!(
            (three_mf.unit, three_mf.unit_origin),
            (LengthUnit::Millimeter, UnitOrigin::Declared)
        );
        assert_eq!(three_mf.objects, 1);
        // A consistent assumption is accepted.
        assert!(import("3mf", &package(&model), Some(LengthUnit::Millimeter)).is_ok());
    }
}

#[test]
fn units_are_required_declared_or_conflicting() {
    let cube = corpus("fx-01-cube.stl");
    let text = obj_text(&cube);
    assert!(matches!(
        import("obj", text.as_bytes(), None),
        Err(ModelImportError::UnitsRequired { format: "OBJ" })
    ));
    assert!(matches!(
        import("stl", b"solid a\nendsolid a\n", None),
        Err(ModelImportError::UnitsRequired { format: "STL" })
    ));
    let inches = import("obj", text.as_bytes(), Some(LengthUnit::Inch)).unwrap();
    for (scaled, original) in inches.mesh.vertices().iter().zip(cube.vertices()) {
        assert_eq!(scaled.x, original.x * 25.4);
    }
    for (unit, factor) in [
        ("micron", 0.001_f64),
        ("centimeter", 10.),
        ("inch", 25.4),
        ("foot", 304.8),
        ("meter", 1000.),
    ] {
        let model = model_xml(unit, "", &[(1, &cube)], "<item objectid=\"1\"/>");
        let imported = import("3mf", &package(&model), None).unwrap();
        assert_eq!(imported.unit.name(), unit);
        assert!(
            (volume6(&imported.mesh) - volume6(&cube) * factor.powi(3)).abs()
                < 1e-6 * volume6(&cube) * factor.powi(3)
        );
        assert!(matches!(
            import("3mf", &package(&model), Some(LengthUnit::Millimeter)),
            Err(ModelImportError::UnitConflict {
                assumed: LengthUnit::Millimeter,
                ..
            })
        ));
    }
    let model = model_xml("furlong", "", &[(1, &cube)], "<item objectid=\"1\"/>");
    assert!(matches!(
        import("3mf", &package(&model), None),
        Err(ModelImportError::ThreeMf(ThreeMfError::UnknownUnit(u))) if u == "furlong"
    ));
    assert_eq!(LengthUnit::parse("in"), Some(LengthUnit::Inch));
    assert_eq!(
        LengthUnit::parse("millimeter"),
        Some(LengthUnit::Millimeter)
    );
    assert_eq!(LengthUnit::parse("MM"), None);
}

fn obj(text: &str) -> Result<ImportedModel, ModelImportError> {
    import("obj", text.as_bytes(), Some(LengthUnit::Millimeter))
}

fn obj_line_error(text: &str) -> (usize, String) {
    match obj(text) {
        Err(ModelImportError::Obj(ObjError::Statement { line, problem })) => {
            (line, format!("{problem:?}"))
        }
        other => panic!("expected a statement error, got {other:?}"),
    }
}

#[test]
fn obj_forms_indices_polygons_and_ignored_statements() {
    // A unit square as one quad with every index form, negative indices and a continuation.
    let text =
        "mtllib a.mtl\r\nv 0 0 0\r\nv 1 0 0 1\nv 1 1 0 0.5 0.5 0.5\nv 0 1 0\nvt 0 0\nvn 0 0 1\n\
                g side\nusemtl red\ns off\nf 1/1/1 2//1 \\\n  -2/1 -1\n# done\n\0\0";
    let model = obj(text).unwrap();
    assert_eq!(model.mesh.triangles(), [[0, 1, 2], [0, 2, 3]]);
    assert!(model.notes.contains(&ImportNote::PolygonsTriangulated {
        polygons: 1,
        triangles: 2
    }));
    assert!(model
        .notes
        .contains(&ImportNote::VertexColorsIgnored { count: 1 }));
    for keyword in ["vt", "vn", "g", "usemtl", "mtllib", "s"] {
        assert!(
            model.notes.contains(&ImportNote::StatementsIgnored {
                keyword: keyword.into(),
                count: 1
            }),
            "{keyword}"
        );
    }
    // Forward references are resolved after all vertices are read.
    let forward = obj("f 1 2 3\nv 0 0 0\nv 1 0 0\nv 0 1 0\n").unwrap();
    assert_eq!(forward.mesh.triangles(), [[0, 1, 2]]);
}

/// A large OBJ grid (several MB, so it is parsed in parallel chunks) written with every index
/// form, negative indices and continued lines, and the mesh it must give.
#[allow(clippy::many_single_char_names)] // Grid notation: corners a..d of cell (i, j).
fn large_grid_obj(n: u32) -> (String, TriangleMesh) {
    let mut text = String::from("# grid\n");
    let mut points = Vec::new();
    let mut triangles = Vec::new();
    for j in 0..=n {
        for i in 0..=n {
            let (x, y) = (f64::from(i) * 0.25, f64::from(j) * 0.25);
            writeln!(text, "v {x} {y} {}", f64::from((i * 7 + j * 3) % 5) * 0.125).unwrap();
            points.push(karasu_core::geometry::Point3::new(
                x,
                y,
                f64::from((i * 7 + j * 3) % 5) * 0.125,
            ));
        }
    }
    let at = |i: u32, j: u32| j * (n + 1) + i;
    for j in 0..n {
        for i in 0..n {
            let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
            match (i + j) % 3 {
                // Absolute indices with texture and normal slots.
                0 => writeln!(text, "f {}/1/1 {}/1/1 {}/1/1", a + 1, b + 1, c + 1).unwrap(),
                // Negative indices relative to the vertex count (all vertices come first).
                1 => {
                    let total = i64::from((n + 1) * (n + 1));
                    let rel = |v: u32| i64::from(v) - total;
                    writeln!(text, "f {} {} \\\n {}", rel(a), rel(b), rel(c)).unwrap();
                }
                _ => writeln!(text, "f {}//1 {}//1 {}//1", a + 1, b + 1, c + 1).unwrap(),
            }
            writeln!(text, "f {} {} {}", a + 1, c + 1, d + 1).unwrap();
            triangles.push([a, b, c]);
            triangles.push([a, c, d]);
        }
    }
    (text, TriangleMesh::try_new(points, triangles).unwrap())
}

#[test]
fn large_obj_files_parse_in_parallel_chunks_like_a_sequential_pass() {
    let (text, expected) = large_grid_obj(400);
    assert!(text.len() > 8 << 20, "{} bytes", text.len());
    let model = obj(&text).unwrap();
    // Vertices are numbered by first use in faces, so compare triangle corner positions.
    let corners = |mesh: &TriangleMesh| -> Vec<[[u64; 3]; 3]> {
        mesh.triangles()
            .iter()
            .map(|t| t.map(|v| mesh.vertices()[v as usize].coordinates().map(f64::to_bits)))
            .collect()
    };
    assert_eq!(corners(&model.mesh), corners(&expected));
    assert_eq!(model.mesh.vertices().len(), expected.vertices().len());
    // An error deep in the file names its exact line, and the earliest one wins.
    let lines: Vec<&str> = text.lines().collect();
    let target = lines.len() * 3 / 4;
    let mut broken: Vec<String> = lines.iter().map(|l| (*l).to_owned()).collect();
    broken[target] = "f 1 2".into();
    broken[target + 1000] = "bogus".into();
    let (line, problem) = obj_line_error(&(broken.join("\n") + "\n"));
    assert_eq!((line, problem.as_str()), (target + 1, "FaceArity(2)"));
}

#[test]
fn concave_polygons_are_split_inside_their_outline() {
    // An L-shaped hexagon (area 3) whose fan from vertex 0 would leave the outline.
    let text = "v 1 1 0\nv 1 2 0\nv 0 2 0\nv 0 0 0\nv 2 0 0\nv 2 1 0\nf 1 2 3 4 5 6\n";
    let model = obj(text).unwrap();
    let v = model.mesh.vertices();
    let mut area = 0.;
    for t in model.mesh.triangles() {
        let [a, b, c] = t.map(|i| v[i as usize]);
        let twice = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
        assert!(
            twice > 0.,
            "every triangle keeps the polygon's counter-clockwise winding"
        );
        area += twice / 2.;
    }
    assert!((area - 3.).abs() < 1e-12);
    // Clockwise in the plane (normal -Z) keeps its winding too.
    let reversed =
        obj("v 1 1 0\nv 2 1 0\nv 2 0 0\nv 0 0 0\nv 0 2 0\nv 1 2 0\nf 1 2 3 4 5 6\n").unwrap();
    let v = reversed.mesh.vertices();
    for t in reversed.mesh.triangles() {
        let [a, b, c] = t.map(|i| v[i as usize]);
        assert!((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x) < 0.);
    }
    // A bow-tie (self-intersecting) quad cannot be split.
    let (line, problem) = obj_line_error("v 0 0 0\nv 1 1 0\nv 1 0 0\nv 0 1 0\nf 1 2 3 4\n");
    assert_eq!((line, problem.as_str()), (5, "Untriangulable"));
}

#[test]
fn obj_errors_name_their_line() {
    let v = "v 0 0 0\nv 1 0 0\nv 0 1 0\n";
    for (text, line, problem) in [
        (format!("{v}f 1 2\n"), 4, "FaceArity(2)"),
        (format!("{v}f 1 2 0\n"), 4, "InvalidIndex"),
        (format!("{v}f 1 2 4\n"), 4, "IndexOutOfRange"),
        (format!("{v}f 1 2 -4\n"), 4, "IndexOutOfRange"),
        (format!("{v}v 1 2\n"), 4, "VertexArity(2)"),
        (format!("{v}v 1 2 3 2\n"), 4, "RationalWeight"),
        (format!("{v}v 1 nan 3\n"), 4, "InvalidNumber"),
        (format!("{v}v 1 1e999 3\n"), 4, "NonFinite"),
        (
            format!("{v}curv 0 1 1 2\n"),
            4,
            "Unsupported(\"curv (free-form geometry)\")",
        ),
        (format!("{v}bogus 1\n"), 4, "Unsupported(\"bogus\")"),
    ] {
        assert_eq!(obj_line_error(&text), (line, problem.to_owned()), "{text}");
    }
    assert!(matches!(
        obj("v 0 0 0\n"),
        Err(ModelImportError::Obj(ObjError::NoFaces))
    ));
    assert!(matches!(
        import("obj", b"v 0 0 0\xff\n", Some(LengthUnit::Millimeter)),
        Err(ModelImportError::Obj(ObjError::NotUtf8 { line: 1 }))
    ));
}

fn three_mf(model: &str) -> Result<ImportedModel, ModelImportError> {
    import("3mf", &package(model), None)
}

fn three_mf_problem(model: &str) -> String {
    match three_mf(model) {
        Err(ModelImportError::ThreeMf(error)) => error.to_string(),
        other => panic!("expected a 3MF error, got {other:?}"),
    }
}

/// The FX-01 cube as an object, then the given component and build XML.
fn cube_model(extra: &str, resources: &str, build: &str) -> String {
    let cube = corpus("fx-01-cube.stl");
    model_xml("millimeter", extra, &[(1, &cube)], build)
        .replace("</resources>", &format!("{resources}</resources>"))
}

#[test]
fn components_items_and_mirroring_follow_the_row_vector_convention() {
    let cube = corpus("fx-01-cube.stl");
    // Object 2 places the cube twice: shifted along x, and mirrored in x then shifted.
    let resources = "<object id=\"2\"><components>\
        <component objectid=\"1\" transform=\"1 0 0 0 1 0 0 0 1 100 0 0\"/>\
        <component objectid=\"1\" transform=\"-1 0 0  0 1 0\n0 0 1 -100 0 0\"/>\
        </components></object>";
    // The item scales by 2 then moves up by 5: x' = 2x (row vector), z' = 2z + 5.
    let model = cube_model(
        "",
        resources,
        "<item objectid=\"2\" transform=\"2 0 0 0 2 0 0 0 2 0 0 5\"/>",
    );
    let imported = three_mf(&model).unwrap();
    assert_eq!(imported.objects, 2);
    assert!(imported
        .notes
        .contains(&ImportNote::MirroredInstances { count: 1 }));
    // Both instances stay outward: total volume is twice the scaled cube.
    let expected = 2. * volume6(&cube) * 8.;
    assert!((volume6(&imported.mesh) - expected).abs() < 1e-9 * expected);
    let (low, high) = imported
        .mesh
        .vertices()
        .iter()
        .fold((f64::MAX, f64::MIN), |(l, h), p| (l.min(p.x), h.max(p.x)));
    let (c_low, c_high) = cube
        .vertices()
        .iter()
        .fold((f64::MAX, f64::MIN), |(l, h), p| (l.min(p.x), h.max(p.x)));
    assert_eq!(low, 2. * (-c_high - 100.));
    assert_eq!(high, 2. * (c_high + 100.));
    assert!(c_low < c_high);
    let z_low = imported
        .mesh
        .vertices()
        .iter()
        .map(|p| p.z)
        .fold(f64::MAX, f64::min);
    let cube_z_low = cube.vertices().iter().map(|p| p.z).fold(f64::MAX, f64::min);
    assert_eq!(z_low, 2. * cube_z_low + 5.);
}

#[test]
fn object_types_extensions_and_parts() {
    let cube = corpus("fx-01-cube.stl");
    // A support object is skipped and reported; an `other` object may not be output.
    let support = model_xml(
        "millimeter",
        "",
        &[(1, &cube), (2, &cube)],
        "<item objectid=\"1\"/><item objectid=\"2\"/>",
    )
    .replacen(
        "<object id=\"2\" type=\"model\"",
        "<object id=\"2\" type=\"support\"",
        1,
    );
    let imported = three_mf(&support).unwrap();
    assert_eq!(imported.objects, 1);
    assert!(imported.notes.contains(&ImportNote::ObjectsSkipped {
        kind: "support".into(),
        count: 1
    }));
    let other = support.replace("type=\"support\"", "type=\"other\"");
    assert!(three_mf_problem(&other).contains("type 'other'"));
    // Required extensions: production and materials are accepted, anything else refused.
    let production = "xmlns:p=\"http://schemas.microsoft.com/3dmanufacturing/production/2015/06\" \
                      xmlns:m=\"http://schemas.microsoft.com/3dmanufacturing/material/2015/02\" \
                      requiredextensions=\"p m\"";
    let model = model_xml(
        "millimeter",
        production,
        &[(1, &cube)],
        "<item objectid=\"1\" p:UUID=\"x\"/>",
    )
    .replace(
        "<resources>",
        "<resources><m:colorgroup id=\"9\"><m:color color=\"#FF0000\"/></m:colorgroup>",
    );
    let imported = three_mf(&model).unwrap();
    assert!(imported.notes.contains(&ImportNote::PropertiesIgnored));
    let beams = "xmlns:b=\"http://schemas.microsoft.com/3dmanufacturing/beamlattice/2017/02\" requiredextensions=\"b\"";
    let refused = three_mf_problem(&model_xml(
        "millimeter",
        beams,
        &[(1, &cube)],
        "<item objectid=\"1\"/>",
    ));
    assert!(refused.contains("beamlattice"), "{refused}");
    // A Production-extension item refers to an object in another model part.
    let part = model_xml("millimeter", "", &[(7, &cube)], "");
    let root = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><model unit=\"millimeter\" \
         xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\" {production}>\
         <resources/><build><item objectid=\"7\" p:path=\"/3D/Objects/part.model\"/></build></model>"
    );
    let bytes = zip(&[
        ("_rels/.rels", RELS.as_bytes()),
        ("3D/3dmodel.model", root.as_bytes()),
        ("3D/Objects/part.model", part.as_bytes()),
    ]);
    let imported = import("3mf", &bytes, None).unwrap();
    same_mesh(&imported.mesh, &cube);
    assert!(imported
        .notes
        .contains(&ImportNote::ExternalParts { count: 1 }));
}

#[test]
fn invalid_packages_and_models_are_refused() {
    let cube = corpus("fx-01-cube.stl");
    let good = model_xml("millimeter", "", &[(1, &cube)], "<item objectid=\"1\"/>");
    for (model, expected) in [
        (good.replace("<?xml version=\"1.0\" encoding=\"UTF-8\"?>", "<?xml version=\"1.0\"?><!DOCTYPE model []>"), "document type"),
        (good.replace("encoding=\"UTF-8\"", "encoding=\"ISO-8859-1\""), "not UTF-8"),
        (good.replace("<item objectid=\"1\"/>", "<item objectid=\"5\"/>"), "object 5"),
        (good.replace("<item objectid=\"1\"/>", ""), "no build items"),
        (good.replace("v1=\"0\"", "v1=\"999\""), "out of range"),
        (good.replace("x=\"", "x=\"nan").replacen("x=\"nan", "x=\"", 0), "invalid value of 'x'"),
        (good.replace("<item objectid=\"1\"/>", "<item objectid=\"1\" transform=\"1 0 0\"/>"), "transform"),
        (good.replace("</resources>", "<object id=\"1\"><mesh/></object></resources>"), "duplicate object id 1"),
        (good.replace("</resources>", "<object id=\"3\"><components><component objectid=\"4\"/></components></object></resources>"), "object 4"),
        (good.replace("<build>", "<build><bogus/>"), "unexpected element 'bogus'"),
        (good.replace("</mesh></object>", "</mesh><components/></object>"), "both a mesh and components"),
        (good.replace("</model>", ""), "not valid XML"),
    ] {
        let message = match three_mf(&model) {
            Err(ModelImportError::ThreeMf(error)) => error.to_string(),
            other => panic!("{expected}: expected a 3MF error, got {:?}", other.map(|m| m.objects)),
        };
        assert!(message.contains(expected), "{expected}: {message}");
    }
    // Package-level failures.
    let message = match import("3mf", b"not a zip at all", None) {
        Err(ModelImportError::ThreeMf(ThreeMfError::Archive(e))) => e.to_string(),
        other => panic!("{other:?}"),
    };
    assert!(message.contains("not a ZIP"));
    let no_rels = zip(&[("3D/3dmodel.model", good.as_bytes())]);
    assert!(matches!(
        import("3mf", &no_rels, None),
        Err(ModelImportError::ThreeMf(ThreeMfError::MissingPart(p))) if p == "_rels/.rels"
    ));
    let wrong_target = zip(&[
        ("_rels/.rels", RELS.as_bytes()),
        ("3D/other.model", good.as_bytes()),
    ]);
    assert!(matches!(
        import("3mf", &wrong_target, None),
        Err(ModelImportError::ThreeMf(ThreeMfError::MissingPart(p))) if p == "/3D/3dmodel.model"
    ));
    // A corrupted stored entry fails its CRC.
    let mut corrupt = package(&good);
    // Still well-formed XML, so only the CRC-32 at the end of the part can catch it.
    let at = corrupt.windows(6).position(|w| w == b"x=\"-10").unwrap();
    corrupt[at + 5] = b'9';
    assert!(matches!(
        import("3mf", &corrupt, None),
        Err(ModelImportError::ThreeMf(ThreeMfError::Archive(_)))
    ));
}
