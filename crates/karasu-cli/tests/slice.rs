//! Native-process contracts for `slice`: STL → layers → LCD masks → create-only print artifacts.
//!
//! Profiles are serialized through the public core schema so these tests follow schema evolution.
//! The 100 × 60 px, 1 mm-pitch panel puts authored box faces on pixel boundaries, so binary mask
//! volume is exactly the analytic volume and layer counts are small enough for Debug runs.

use karasu_core::profiles::{
    store::ProfileDocument, ImageSettings, LayerSettings, MotionStage, PrintSettingProfile,
    PrintSettings, PrinterCalibration, PrinterLimits, PrinterProfile, ResinMaterial, ResinProfile,
    SettingStatus,
};
use std::{
    ffi::OsStr,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);
const PRINTER_ID: &str = "test-printer";
const LAYER_HEIGHT_MM: f64 = 0.25;
const MACHINE_FORMATS: [(&str, &str); 13] = [
    ("ctb-v2-plain", "ctb"),
    ("ctb-v3-plain", "ctb"),
    ("ctb-v4-plain", "ctb"),
    ("ctb-v5-plain", "ctb"),
    ("goo-v3", "goo"),
    ("anycubic-pp1-v3", "pp1"),
    ("photon-v1-firmware-motion", "photon"),
    ("anycubic-pws-v1", "pws"),
    ("anycubic-pwmx-v1", "pwmx"),
    ("anycubic-pmsq-v515", "pmsq"),
    ("anycubic-pwma-v516", "pwma"),
    ("anycubic-pm5-v517", "pm5"),
    ("anycubic-pm5s-v518", "pm5s"),
];
/// SL1 needs one bottom layer and a 3- to 20-layer fade: its own resin (see the SL1 test).
const SL1_FORMATS: [(&str, &str); 2] = [
    ("prusa-sl1-firmware-motion", "sl1"),
    ("prusa-sl1s-firmware-motion", "sl1s"),
];

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Self {
        for _ in 0..100 {
            let index = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("karasu-cli-slice-{}-{index}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("cannot create isolated test directory: {error}"),
            }
        }
        panic!("cannot find a unique test directory");
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    fn write(&self, name: &str, bytes: impl AsRef<[u8]>) -> PathBuf {
        let path = self.path(name);
        fs::write(&path, bytes).unwrap();
        path
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// ASCII STL for an axis-aligned box, optionally omitting its two top (+Z) triangles.
fn box_stl(min: [f64; 3], max: [f64; 3], omit_top: bool) -> String {
    let corner = |i: usize| {
        [
            if i & 1 == 0 { min[0] } else { max[0] },
            if i & 2 == 0 { min[1] } else { max[1] },
            if i & 4 == 0 { min[2] } else { max[2] },
        ]
    };
    // Outward counterclockwise quads, by corner bit index (x = 1, y = 2, z = 4).
    let quads: [([usize; 4], bool); 6] = [
        ([0, 2, 3, 1], false),
        ([4, 5, 7, 6], true),
        ([0, 1, 5, 4], false),
        ([2, 6, 7, 3], false),
        ([0, 4, 6, 2], false),
        ([1, 3, 7, 5], false),
    ];
    let mut text = String::from("solid box\n");
    for (quad, top) in quads {
        if top && omit_top {
            continue;
        }
        for triangle in [[quad[0], quad[1], quad[2]], [quad[0], quad[2], quad[3]]] {
            text.push_str("facet normal 0 0 0\nouter loop\n");
            for i in triangle {
                let [x, y, z] = corner(i);
                writeln!(text, "vertex {x} {y} {z}").unwrap();
            }
            text.push_str("endloop\nendfacet\n");
        }
    }
    text.push_str("endsolid box\n");
    text
}

fn printer() -> PrinterProfile {
    PrinterProfile {
        schema_version: 1,
        id: PRINTER_ID.into(),
        name: "Test printer".into(),
        revision: 1,
        build_size_mm: [100., 60., 50.],
        resolution_px: [100, 60],
        origin_xy_mm: [0., 0.],
        swap_xy: false,
        mirror_x: false,
        mirror_y: false,
        format_ids: MACHINE_FORMATS
            .iter()
            .chain(&SL1_FORMATS)
            .map(|(id, _)| (*id).into())
            .collect(),
        limits: PrinterLimits {
            min_layer_height_mm: 0.01,
            max_layer_height_mm: 0.3,
            max_exposure_s: 120.,
            max_speed_mm_s: 100.,
            max_lift_mm: 20.,
        },
        vendor: None,
        model: None,
        based_on: None,
        fixed_layer_time_s: None,
    }
}

fn layer(exposure_s: f64) -> LayerSettings {
    let stage = MotionStage {
        distance_mm: 5.,
        speed_mm_s: 2.,
    };
    LayerSettings {
        exposure_s,
        wait_before_exposure_s: 0.,
        wait_after_exposure_s: 0.,
        wait_after_lift_s: 0.,
        pwm: 255,
        lift: vec![stage.clone()],
        retract: vec![stage],
    }
}

fn resin(printer_id: &str) -> ResinProfile {
    ResinProfile {
        schema_version: 1,
        id: "test-resin".into(),
        name: "Test resin".into(),
        revision: 1,
        density_g_per_ml: None,
        cost_per_liter: None,
        currency: None,
        calibrations: vec![PrinterCalibration {
            printer_id: printer_id.into(),
            settings: PrintSettings {
                heating_temperature_c: None,
                layer_height_mm: LAYER_HEIGHT_MM,
                bottom_layer_count: 1,
                transition_layer_count: 1,
                bottom: layer(20.),
                normal: layer(2.),
            },
        }],
    }
}

struct Setup {
    temp: TempDirectory,
    printer: PathBuf,
    resin: PathBuf,
}

impl Setup {
    fn new() -> Self {
        Self::with_resin(resin(PRINTER_ID))
    }

    fn with_resin(resin: ResinProfile) -> Self {
        let temp = TempDirectory::new();
        let printer = temp.write(
            "printer.json",
            ProfileDocument::Printer(printer()).to_json().unwrap(),
        );
        let resin = temp.write(
            "resin.json",
            ProfileDocument::Resin(resin).to_json().unwrap(),
        );
        Self {
            temp,
            printer,
            resin,
        }
    }

    /// A 20 × 10 × 2 mm box resting on the plate at source XY [10, 30] × [20, 30].
    fn cube(&self) -> PathBuf {
        self.temp
            .write("box.stl", box_stl([10., 20., 0.], [30., 30., 2.], false))
    }

    fn slice(&self, model: &Path, format: &str, output: &Path, extra: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_karasu-cli"))
            .arg("slice")
            .arg("--assume-mm")
            .arg(model)
            .arg("--printer")
            .arg(&self.printer)
            .arg("--resin")
            .arg(&self.resin)
            .args(["--format", format])
            .arg("--output")
            .arg(output)
            .args(extra)
            .output()
            .unwrap()
    }
}

impl Setup {
    /// Slice with `--model` and an optional `--units`.
    fn slice_model(
        &self,
        model: &Path,
        units: Option<&str>,
        output: &Path,
        extra: &[&str],
    ) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_karasu-cli"));
        command.arg("slice").arg("--model").arg(model);
        if let Some(units) = units {
            command.args(["--units", units]);
        }
        command
            .arg("--printer")
            .arg(&self.printer)
            .arg("--resin")
            .arg(&self.resin)
            .args(["--format", "goo-v3"])
            .arg("--output")
            .arg(output)
            .args(extra)
            .output()
            .unwrap()
    }
}

/// Box corners and outward quads, as `box_stl` uses them.
fn box_quads(min: [f64; 3], max: [f64; 3]) -> (Vec<[f64; 3]>, [[usize; 4]; 6]) {
    let corners = (0..8)
        .map(|i| {
            [
                if i & 1 == 0 { min[0] } else { max[0] },
                if i & 2 == 0 { min[1] } else { max[1] },
                if i & 4 == 0 { min[2] } else { max[2] },
            ]
        })
        .collect();
    let quads = [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ];
    (corners, quads)
}

/// The box as an OBJ with one quad per side.
fn box_obj(min: [f64; 3], max: [f64; 3]) -> String {
    let (corners, quads) = box_quads(min, max);
    let mut text = String::from("o box\n");
    for [x, y, z] in corners {
        writeln!(text, "v {x} {y} {z}").unwrap();
    }
    for q in quads {
        writeln!(
            text,
            "f {} {} {} {}",
            q[0] + 1,
            q[1] + 1,
            q[2] + 1,
            q[3] + 1
        )
        .unwrap();
    }
    text
}

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

/// A minimal stored ZIP.
fn stored_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let (mut out, mut central) = (Vec::new(), Vec::new());
    for (name, data) in entries {
        let offset = u32::try_from(out.len()).unwrap();
        let (size, crc) = (u32::try_from(data.len()).unwrap(), crc32(data));
        let length = u16::try_from(name.len()).unwrap();
        out.extend(0x0403_4b50u32.to_le_bytes());
        out.extend([20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        out.extend(crc.to_le_bytes());
        out.extend(size.to_le_bytes());
        out.extend(size.to_le_bytes());
        out.extend(length.to_le_bytes());
        out.extend([0, 0]);
        out.extend(name.as_bytes());
        out.extend(*data);
        central.extend(0x0201_4b50u32.to_le_bytes());
        central.extend([20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        central.extend(crc.to_le_bytes());
        central.extend(size.to_le_bytes());
        central.extend(size.to_le_bytes());
        central.extend(length.to_le_bytes());
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

/// The box as a 3MF package in `unit`, triangulated like `box_stl`.
fn box_3mf(min: [f64; 3], max: [f64; 3], unit: &str) -> Vec<u8> {
    let (corners, quads) = box_quads(min, max);
    let mut model = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><model unit=\"{unit}\" \
         xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\"><resources>\
         <object id=\"1\" type=\"model\"><mesh><vertices>"
    );
    for [x, y, z] in corners {
        write!(model, "<vertex x=\"{x}\" y=\"{y}\" z=\"{z}\"/>").unwrap();
    }
    model.push_str("</vertices><triangles>");
    for q in quads {
        for t in [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] {
            write!(
                model,
                "<triangle v1=\"{}\" v2=\"{}\" v3=\"{}\"/>",
                t[0], t[1], t[2]
            )
            .unwrap();
        }
    }
    model.push_str(
        "</triangles></mesh></object></resources><build><item objectid=\"1\"/></build></model>",
    );
    let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships \
        xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship \
        Target=\"/3D/3dmodel.model\" Id=\"r\" \
        Type=\"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel\"/></Relationships>";
    stored_zip(&[
        ("_rels/.rels", rels.as_bytes()),
        ("3D/3dmodel.model", model.as_bytes()),
    ])
}

#[test]
fn obj_and_3mf_models_slice_like_the_same_stl() {
    let setup = Setup::new();
    // Inch multiples by powers of two are exact in binary64, so every unit gives the same mesh.
    let (min, max) = ([25.4, 25.4, 0.], [50.8, 50.8, 12.7]);
    let stl = setup.temp.write("box.stl", box_stl(min, max, false));
    let reference = setup.temp.path("stl.goo");
    success(&setup.slice(&stl, "goo-v3", &reference, &[]));
    let expected = fs::read(&reference).unwrap();
    let obj = setup.temp.write("box.obj", box_obj(min, max));
    let obj_cm = setup
        .temp
        .write("box-cm.OBJ", box_obj([2.54, 2.54, 0.], [5.08, 5.08, 1.27]));
    let three_mf = setup.temp.write("box.3mf", box_3mf(min, max, "millimeter"));
    let inches = setup
        .temp
        .write("inch.3mf", box_3mf([1., 1., 0.], [2., 2., 0.5], "inch"));
    for (model, units, label, notes) in [
        (
            &obj,
            Some("mm"),
            "OBJ (millimeter assumed, 1 objects)",
            Some("6 polygons split into 12 triangles"),
        ),
        (
            &obj_cm,
            Some("cm"),
            "OBJ (centimeter assumed, 1 objects)",
            None,
        ),
        (
            &three_mf,
            None,
            "3MF (millimeter declared, 1 objects)",
            None,
        ),
        (&inches, None, "3MF (inch declared, 1 objects)", None),
        (&inches, Some("in"), "3MF (inch declared, 1 objects)", None),
    ] {
        let output = setup
            .temp
            .path(&format!("{}-{}.goo", label.len(), units.unwrap_or("none")));
        let _ = fs::remove_file(&output);
        let report = success(&setup.slice_model(model, units, &output, &[]));
        assert_eq!(field(&report, "Model"), label);
        if let Some(notes) = notes {
            assert!(field(&report, "Import notes").contains(notes), "{report}");
        }
        assert_eq!(fs::read(&output).unwrap(), expected, "{label}");
    }
    // The legacy --assume-mm reads OBJ by its extension.
    let legacy = setup.temp.path("legacy.goo");
    success(&setup.slice(&obj, "goo-v3", &legacy, &[]));
    assert_eq!(fs::read(&legacy).unwrap(), expected);
    // Unit errors name the remedy, and nothing is written.
    let refused = setup.temp.path("refused.goo");
    let error = failure(&setup.slice_model(&obj, None, &refused, &[]), 1);
    assert!(
        error.contains("declare no length unit") && error.contains("--units"),
        "{error}"
    );
    let error = failure(&setup.slice(&inches, "goo-v3", &refused, &[]), 1);
    assert!(
        error.contains("declares inch units, but millimeter was assumed"),
        "{error}"
    );
    let ply = setup.temp.write("box.ply", "ply");
    let error = failure(&setup.slice_model(&ply, Some("mm"), &refused, &[]), 1);
    assert!(
        error.contains("'.ply'") && error.contains(".obj"),
        "{error}"
    );
    assert!(!refused.exists());
    // Inspection reports the format, units and notes, and the geometry in millimetres.
    let report = success(&run([
        OsStr::new("inspect"),
        OsStr::new("--model"),
        inches.as_os_str(),
    ]));
    assert_eq!(field(&report, "Model format"), "3MF");
    assert!(field(&report, "Units").starts_with("inch (declared"));
    assert_eq!(field(&report, "Source maximum XYZ (mm)"), "50.8 50.8 12.7");
    let usage = failure(
        &run([
            OsStr::new("inspect"),
            OsStr::new("--model"),
            obj.as_os_str(),
        ]),
        1,
    );
    assert!(usage.contains("--units"), "{usage}");
}

fn run(args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Output {
    Command::new(env!("CARGO_BIN_EXE_karasu-cli"))
        .args(args)
        .output()
        .unwrap()
}

/// GOO thumbnails: 116 × 116 and 290 × 290 RGB565 words, each followed by CRLF.
const GOO_PREVIEWS: std::ops::Range<usize> = 194..194 + 2 * (116 * 116 + 290 * 290) + 4;

fn goo_previews(goo: &[u8]) -> &[u8] {
    &goo[GOO_PREVIEWS]
}

fn goo_without_previews(goo: &[u8]) -> Vec<u8> {
    [&goo[..GOO_PREVIEWS.start], &goo[GOO_PREVIEWS.end..]].concat()
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
    assert_eq!(
        output.status.code(),
        Some(exit),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
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
fn reference_export_streams_exact_box_layers_and_manifest() {
    let setup = Setup::new();
    let output = setup.temp.path("reference");
    let report = success(&setup.slice(&setup.cube(), "reference", &output, &[]));
    assert_eq!(field(&report, "Format"), "reference");
    // Midpoint samples 0.125, 0.375, ..., 1.875 mm: eight 0.25 mm slabs through Z = 2.
    assert_eq!(field(&report, "Layers"), "8");
    assert_eq!(field(&report, "Sampled mask volume (mm^3)"), "400");
    for name in ["manifest.json", "layers.jsonl", "layer-00000000.png"] {
        assert!(output.join(name).is_file(), "missing {name}");
    }
    assert!(!output.join("layer-00000008.png").exists());
    let manifest = fs::read_to_string(output.join("manifest.json")).unwrap();
    assert!(manifest.contains("\"slice\""), "default job ID is recorded");
}

#[test]
fn a_library_resin_and_print_setting_slice_like_the_calibrated_resin() {
    let setup = Setup::new();
    let calibrated = resin(PRINTER_ID);
    let material = ResinMaterial {
        colour: None,
        swatch: None,
        schema_version: 2,
        id: calibrated.id.clone(),
        name: calibrated.name.clone(),
        revision: 1,
        vendor: None,
        kind: None,
        density_g_per_ml: None,
        cost_per_liter: None,
        currency: None,
        based_on: None,
    };
    let setting = PrintSettingProfile {
        schema_version: 2,
        id: "test-setting".into(),
        name: "Test".into(),
        revision: 1,
        printer_id: PRINTER_ID.into(),
        resin_id: calibrated.id.clone(),
        film: None,
        status: SettingStatus::User,
        source: None,
        based_on: None,
        settings: calibrated.calibrations[0].settings.clone(),
        image: ImageSettings {
            antialias: 4,
            min_gray: 0,
            xy_compensation_px: -1,
            bottom_xy_compensation_px: None,
        },
    };
    let material_file = setup.temp.write(
        "material.json",
        ProfileDocument::Material(material).to_json().unwrap(),
    );
    let setting_file = setup.temp.write(
        "setting.json",
        ProfileDocument::PrintSetting(setting).to_json().unwrap(),
    );
    let cube = setup.cube();
    let flags = setup.temp.path("flags.goo");
    success(&setup.slice(
        &cube,
        "goo-v3",
        &flags,
        &["--antialias", "4", "--xy-compensation", "-1"],
    ));
    let library = setup.temp.path("library.goo");
    let run = |output: &Path, resin: &Path, extra: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_karasu-cli"))
            .arg("slice")
            .arg("--assume-mm")
            .arg(&cube)
            .arg("--printer")
            .arg(&setup.printer)
            .arg("--resin")
            .arg(resin)
            .arg("--print-setting")
            .arg(&setting_file)
            .args(["--format", "goo-v3"])
            .arg("--output")
            .arg(output)
            .args(extra)
            .output()
            .unwrap()
    };
    success(&run(&library, &material_file, &[]));
    assert_eq!(
        goo_without_previews(&fs::read(&flags).unwrap()),
        goo_without_previews(&fs::read(&library).unwrap())
    );
    // The setting holds the masks: the flags are refused beside it.
    let refused = failure(
        &run(
            &setup.temp.path("x.goo"),
            &material_file,
            &["--antialias", "2"],
        ),
        2,
    );
    assert!(refused.contains("--print-setting"), "{refused}");
    // With --print-setting, --resin names a library resin.
    let refused = failure(&run(&setup.temp.path("y.goo"), &setup.resin, &[]), 1);
    assert!(refused.contains("\"material\""), "{refused}");
    // A library resin alone has no exposure.
    let alone = Command::new(env!("CARGO_BIN_EXE_karasu-cli"))
        .arg("slice")
        .arg("--assume-mm")
        .arg(&cube)
        .arg("--printer")
        .arg(&setup.printer)
        .arg("--resin")
        .arg(&material_file)
        .args(["--format", "goo-v3"])
        .arg("--output")
        .arg(setup.temp.path("z.goo"))
        .output()
        .unwrap();
    assert!(failure(&alone, 1).contains("--print-setting"));
}

#[test]
#[allow(clippy::too_many_lines)] // One scenario: the profiles, a project of two plates, refusals.
fn a_saved_project_slices_from_the_command_line_as_the_library_slices_it() {
    // TD-130: `slice --project` slices the plate with its own profiles and objects.
    use karasu_project::{
        pipeline::{slice, Silent},
        project::{file, PlateContent, PlateObject, PlateSettings, Project},
    };
    let setup = Setup::new();
    let calibrated = resin(PRINTER_ID);
    let material = ResinMaterial {
        colour: None,
        swatch: None,
        schema_version: 2,
        id: calibrated.id.clone(),
        name: calibrated.name.clone(),
        revision: 1,
        vendor: None,
        kind: None,
        density_g_per_ml: None,
        cost_per_liter: None,
        currency: None,
        based_on: None,
    };
    let setting = PrintSettingProfile {
        schema_version: 2,
        id: "test-setting".into(),
        name: "Test".into(),
        revision: 1,
        printer_id: PRINTER_ID.into(),
        resin_id: calibrated.id.clone(),
        film: None,
        status: SettingStatus::User,
        source: None,
        based_on: None,
        settings: calibrated.calibrations[0].settings.clone(),
        image: ImageSettings {
            antialias: 1,
            min_gray: 0,
            xy_compensation_px: 0,
            bottom_xy_compensation_px: None,
        },
    };
    let mut project = Project::new("Command line");
    let model = project.add_model(
        "box.stl",
        box_stl([10., 20., 0.], [30., 30., 2.], false).into_bytes(),
    );
    let content = |name: &str| PlateContent {
        name: name.to_owned(),
        printer: Some(printer()),
        resin: Some(material.clone()),
        print_setting: Some(setting.clone()),
        settings: PlateSettings::new("goo-v3"),
        objects: vec![PlateObject::new("box", model)],
    };
    let first = project.add_plate(content("First"));
    let second = project.add_plate(content("Second"));
    project
        .edit_plate(second, |p| p.objects[0].copies = 2)
        .unwrap();
    let path = setup.temp.path("project.karasu");
    file::save(&project, &path).unwrap();
    for (plate, choice) in [(first, "1"), (second, "Second")] {
        let library = setup.temp.path(&format!("library-{choice}.goo"));
        let (request, _) = project.request(plate, library.clone()).unwrap();
        slice(&request, &mut Silent, &|_| false).unwrap();
        let output = setup.temp.path(&format!("cli-{choice}.goo"));
        success(&run([
            OsStr::new("slice"),
            OsStr::new("--project"),
            path.as_os_str(),
            OsStr::new("--project-plate"),
            OsStr::new(choice),
            OsStr::new("--output"),
            output.as_os_str(),
            OsStr::new("--no-progress"),
        ]));
        assert_eq!(fs::read(&output).unwrap(), fs::read(&library).unwrap());
    }
    // Two plates: one must be chosen; the project holds every other option.
    let output = setup.temp.path("none.goo");
    let message = failure(
        &run([
            OsStr::new("slice"),
            OsStr::new("--project"),
            path.as_os_str(),
            OsStr::new("--output"),
            output.as_os_str(),
        ]),
        1,
    );
    assert!(message.contains("--project-plate"), "{message}");
    let message = failure(
        &run([
            OsStr::new("slice"),
            OsStr::new("--project"),
            path.as_os_str(),
            OsStr::new("--output"),
            output.as_os_str(),
            OsStr::new("--format"),
            OsStr::new("goo-v3"),
        ]),
        2,
    );
    assert!(message.contains("--format cannot be given"), "{message}");
    // A file that is not a project.
    let message = failure(
        &run([
            OsStr::new("slice"),
            OsStr::new("--project"),
            setup.printer.as_os_str(),
            OsStr::new("--output"),
            output.as_os_str(),
        ]),
        1,
    );
    assert!(message.contains("not a karasu-slicer project"), "{message}");
}

/// ASCII STL of one closed shell: a 10 × 10 plate of height `p` at XY [10, 20]² carrying a
/// 4 × 4 boss up to Z = 1, with the plate top as an annulus around the boss.
fn step_stl(p: f64) -> String {
    let points: [[f64; 3]; 16] = [
        [10., 10., 0.],
        [20., 10., 0.],
        [20., 20., 0.],
        [10., 20., 0.],
        [10., 10., p],
        [20., 10., p],
        [20., 20., p],
        [10., 20., p],
        [13., 13., p],
        [17., 13., p],
        [17., 17., p],
        [13., 17., p],
        [13., 13., 1.],
        [17., 13., 1.],
        [17., 17., 1.],
        [13., 17., 1.],
    ];
    let mut triangles: Vec<[usize; 3]> = vec![
        [0, 2, 1],
        [0, 3, 2],
        [0, 1, 5],
        [0, 5, 4],
        [1, 2, 6],
        [1, 6, 5],
        [2, 3, 7],
        [2, 7, 6],
        [3, 0, 4],
        [3, 4, 7],
        [8, 9, 13],
        [8, 13, 12],
        [9, 10, 14],
        [9, 14, 13],
        [10, 11, 15],
        [10, 15, 14],
        [11, 8, 12],
        [11, 12, 15],
        [12, 13, 14],
        [12, 14, 15],
    ];
    for k in 0..4 {
        let n = (k + 1) % 4;
        triangles.push([4 + k, 4 + n, 8 + n]);
        triangles.push([4 + k, 8 + n, 8 + k]);
    }
    let mut text = String::from("solid step\n");
    for triangle in triangles {
        text.push_str("facet normal 0 0 0\nouter loop\n");
        for i in triangle {
            let [x, y, z] = points[i];
            writeln!(text, "vertex {x} {y} {z}").unwrap();
        }
        text.push_str("endloop\nendfacet\n");
    }
    text.push_str("endsolid step\n");
    text
}

#[test]
fn a_layer_plane_on_a_step_face_keeps_the_material_below() {
    // Midpoint planes 0.125, 0.375, 0.625, 0.875 mm; the plate top at 0.375 lies exactly on the
    // second one. That layer must hold the whole plate (100 mm²), not the plate minus the boss
    // footprint (84 mm²), which would leave the boss without material beneath for one layer.
    let setup = Setup::new();
    let model = setup.temp.write("step.stl", step_stl(0.375));
    let output = setup.temp.path("step");
    let report = success(&setup.slice(&model, "reference", &output, &[]));
    assert_eq!(field(&report, "Layers"), "4");
    assert_eq!(field(&report, "Sampled mask volume (mm^3)"), "58");
    // Control: a plate top off every plane (0.25 mm) gives one plate layer and three boss
    // layers by ordinary sampling, 0.25 × (100 + 3 × 16) mm³.
    let model = setup.temp.write("step-low.stl", step_stl(0.25));
    let output = setup.temp.path("step-low");
    let report = success(&setup.slice(&model, "reference", &output, &[]));
    assert_eq!(field(&report, "Sampled mask volume (mm^3)"), "37");
}

#[test]
fn every_machine_format_publishes_one_complete_file() {
    let setup = Setup::new();
    let model = setup.cube();
    for (format, extension) in MACHINE_FORMATS {
        let output = setup.temp.path(&format!("{format}.{extension}"));
        let report = success(&setup.slice(&model, format, &output, &["--job-id", "box-job"]));
        assert_eq!(field(&report, "Format"), format);
        assert_eq!(field(&report, "Layers"), "8");
        assert_eq!(field(&report, "Sampled mask volume (mm^3)"), "400");
        let bytes = fs::metadata(&output).unwrap().len();
        assert_eq!(field(&report, "Output bytes"), bytes.to_string());
    }
    // Only the published files and the inputs remain; no staging files leak.
    assert_eq!(
        fs::read_dir(&setup.temp.0).unwrap().count(),
        3 + MACHINE_FORMATS.len()
    );
}

#[test]
fn prusa_sl1_archives_need_a_fade_and_store_config_and_layers() {
    let mut faded = resin(PRINTER_ID);
    faded.calibrations[0].settings.transition_layer_count = 3;
    let setup = Setup::with_resin(faded);
    let model = setup.cube();
    for (format, extension) in SL1_FORMATS {
        let output = setup.temp.path(&format!("part.{extension}"));
        let report = success(&setup.slice(&model, format, &output, &["--job-id", "part"]));
        assert_eq!(field(&report, "Format"), format);
        let data = fs::read(&output).unwrap();
        // A stored ZIP: the first member is the first layer, config.ini comes last.
        assert_eq!(&data[30..30 + 13], b"part00000.png");
        let config_at = data.windows(10).rposition(|w| w == b"config.ini").unwrap();
        assert!(config_at > data.len() / 2);
        let text = String::from_utf8_lossy(&data);
        let model_name = if extension == "sl1" { "SL1" } else { "SL1S" };
        assert!(
            text.contains(&format!("printerModel = {model_name}\n")),
            "{format}"
        );
        assert!(text.contains("numFade = 3\n"));
        assert!(text.contains("thumbnail/thumbnail800x480.png"));
    }
    // The default test resin fades over one layer, which SL1 cannot store.
    let setup = Setup::new();
    let output = setup.temp.path("refused.sl1");
    let error = failure(
        &setup.slice(&setup.cube(), "prusa-sl1-firmware-motion", &output, &[]),
        1,
    );
    assert!(error.contains("3 to 20"), "{error}");
    assert!(!output.exists());
}

#[test]
fn every_machine_format_stores_rendered_previews() {
    let setup = Setup::new();
    let model = setup.cube();
    // Background in the corner, the box in the centre; three visible faces and their edges.
    let shaded = |pixels: &[u32], [width, height]: [usize; 2]| {
        let distinct: std::collections::BTreeSet<_> = pixels.iter().collect();
        pixels.len() == width * height
            && pixels[0] != pixels[height / 2 * width + width / 2]
            && distinct.len() >= 4
    };
    for (format, extension) in MACHINE_FORMATS {
        let output = setup.temp.path(&format!("previews-{format}.{extension}"));
        success(&setup.slice(&model, format, &output, &[]));
        let b = fs::read(&output).unwrap();
        let u32_at = |at: usize| u32::from_le_bytes(b[at..at + 4].try_into().unwrap()) as usize;
        match extension {
            "goo" => {
                for (start, side) in [(194, 116), (194 + 116 * 116 * 2 + 2, 290)] {
                    let words: Vec<u32> = b[start..start + side * side * 2]
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|&w| u32::from(u16::from_be_bytes(w)))
                        .collect();
                    assert!(shaded(&words, [side, side]), "{format} {side}");
                }
            }
            "pws" | "pwmx" | "pmsq" | "pwma" | "pm5" | "pm5s" => {
                // PREVIEW section: 224 x 168 words, blue in the high bits.
                let preview = u32_at(28);
                assert_eq!([u32_at(preview + 16), u32_at(preview + 24)], [224, 168]);
                let words: Vec<u32> = b[preview + 28..preview + 28 + 224 * 168 * 2]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|&w| u32::from(u16::from_le_bytes(w)))
                    .collect();
                assert!(shaded(&words, [224, 168]), "{format}");
            }
            "ctb" | "photon" => {
                for (at, size) in [(60, [400usize, 300]), (72, [200, 125])] {
                    let descriptor = u32_at(at);
                    assert_eq!([u32_at(descriptor), u32_at(descriptor + 4)], size);
                    let (data, n) = (u32_at(descriptor + 8), u32_at(descriptor + 12));
                    // RLE15: a word with bit 5 set repeats (next word & 0xfff) + 1 times.
                    let mut words = b[data..data + n]
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|&w| u32::from(u16::from_le_bytes(w)));
                    let mut pixels = Vec::new();
                    while let Some(word) = words.next() {
                        let run = if word & 0x20 == 0 {
                            1
                        } else {
                            (words.next().unwrap() & 0xfff) as usize + 1
                        };
                        pixels.extend(std::iter::repeat_n(word & !0x20, run));
                    }
                    assert!(shaded(&pixels, size), "{format} {size:?}");
                }
            }
            _ => {
                // PP1: ZIP members, deflated when smaller (TD-133), opaque RGBA PNGs (TD-117); a
                // rendered image has colour where the gray projection has none.
                let archive = karasu_core::archive::ArchiveReader::open(
                    &b,
                    karasu_core::archive::ArchiveLimits {
                        max_entries: 1 << 16,
                        max_entry_bytes: 1 << 30,
                    },
                )
                .unwrap();
                for (name, size) in [
                    ("preview_images/preview_0.png", [224u32, 168]),
                    ("preview_images/preview_1.png", [336, 252]),
                ] {
                    let member = archive.read(name).unwrap().unwrap();
                    let png = &member[..];
                    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
                    let dimensions = [&png[16..20], &png[20..24]]
                        .map(|d| u32::from_be_bytes(d.try_into().unwrap()));
                    assert_eq!((dimensions, png[24], png[25]), (size, 8, 6));
                    assert!(coloured(png, size[0] as usize), "{format} {size:?}");
                }
            }
        }
    }
}

/// Whether an RGBA PNG of stored DEFLATE blocks (karasu's previews) of rows `width` wide has a
/// pixel that is not gray.
fn coloured(png: &[u8], width: usize) -> bool {
    let mut data = Vec::new();
    let mut at = 8;
    while at + 8 <= png.len() {
        let length = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
        let kind = &png[at + 4..at + 8];
        if kind == b"IDAT" {
            data.extend_from_slice(&png[at + 8..at + 8 + length]);
        }
        if kind == b"IEND" {
            break;
        }
        at += 12 + length;
    }
    // A zlib header, then stored blocks: a final flag, the length and its complement.
    let mut raw = Vec::new();
    let mut at = 2;
    loop {
        let last = data[at] & 1 == 1;
        let length = u16::from_le_bytes([data[at + 1], data[at + 2]]) as usize;
        raw.extend_from_slice(&data[at + 5..at + 5 + length]);
        at += 5 + length;
        if last {
            break;
        }
    }
    raw.chunks(width * 4 + 1)
        .flat_map(|row| row[1..].chunks(4))
        .any(|p| p[0] != p[1] || p[1] != p[2])
}

#[test]
fn translation_places_the_model_and_is_applied_exactly() {
    let setup = Setup::new();
    let lifted = setup.temp.write(
        "lifted.stl",
        box_stl([-40., -5., 0.1], [-20., 5., 2.1], false),
    );
    let output = setup.temp.path("placed");
    let report = success(&setup.slice(
        &lifted,
        "reference",
        &output,
        &["--translate", "50", "30", "-0.1"],
    ));
    assert_eq!(field(&report, "Layers"), "8");
}

#[test]
fn center_and_drop_compute_the_translation_exactly() {
    let setup = Setup::new();
    // A box floating at Z 0.1 to 2.1, far from the plate in X/Y.
    let floating = setup.temp.write(
        "floating.stl",
        box_stl([-40., -5., 0.1], [-20., 5., 2.1], false),
    );
    let computed = setup.temp.path("computed.goo");
    let report = success(&setup.slice(&floating, "goo-v3", &computed, &["--center", "--drop"]));
    // The 100 x 60 mm plate centre is (50, 30); the box centre (-30, 0) moves there.
    assert_eq!(field(&report, "Translation (mm)"), "80 30 -0.1");
    assert!(!report.contains("Transform:"), "{report}");
    let explicit = setup.temp.path("explicit.goo");
    let report = success(&setup.slice(
        &floating,
        "goo-v3",
        &explicit,
        &["--translate", "80", "30", "-0.1"],
    ));
    assert_eq!(field(&report, "Translation (mm)"), "80 30 -0.1");
    assert_eq!(fs::read(&computed).unwrap(), fs::read(&explicit).unwrap());
    // --translate is explicit and excludes the computed placements.
    for option in ["--center", "--drop"] {
        let error = failure(
            &setup.slice(
                &floating,
                "goo-v3",
                &setup.temp.path("refused.goo"),
                &["--translate", "1", "2", "3", option],
            ),
            2,
        );
        assert!(error.contains("cannot be combined"), "{error}");
    }
}

#[test]
fn rotation_scale_and_mirror_transform_about_the_model_centre() {
    let setup = Setup::new();
    let model = setup.cube();
    let slice = |name: &str, extra: &[&str]| {
        let output = setup.temp.path(name);
        let report = success(&setup.slice(&model, "goo-v3", &output, extra));
        (report, fs::read(&output).unwrap())
    };
    let volume =
        |report: &str| -> f64 { field(report, "Sampled mask volume (mm^3)").parse().unwrap() };
    let (_, plain) = slice("plain.goo", &[]);
    // Mirroring a box about its own centre gives the same box: the reflected orientation is
    // handled, not printed inside out.
    let (report, mirrored) = slice("mirrored.goo", &["--mirror", "xz"]);
    assert_eq!(
        field(&report, "Transform"),
        "rotate 0 0 0 degrees, scale 1 1 1, mirror xz"
    );
    assert_eq!(
        goo_without_previews(&mirrored),
        goo_without_previews(&plain)
    );
    // A half turn about Z is exact and also maps the box onto itself.
    let (_, turned) = slice("turned.goo", &["--rotate", "0", "0", "180"]);
    assert_eq!(goo_without_previews(&turned), goo_without_previews(&plain));
    // A quarter turn about Z: 10 x 20 mm footprint around the same centre (20, 25), 400 mm^3.
    let (report, _) = slice("quarter.goo", &["--rotate", "0", "0", "90", "--drop"]);
    assert!((volume(&report) - 400.0).abs() < 1e-6, "{report}");
    // Scaling doubles X and Y about the centre: 40 x 20 x 2 mm, 1600 mm^3.
    let (report, _) = slice("scaled.goo", &["--scale", "2", "2", "1"]);
    assert!((volume(&report) - 1600.0).abs() < 1e-9, "{report}");
    // A tilt leaves the plate; --drop rests it again exactly.
    let output = setup.temp.path("tilted.goo");
    let error = failure(
        &setup.slice(&model, "goo-v3", &output, &["--rotate", "10", "0", "0"]),
        1,
    );
    assert!(error.contains("use --drop"), "{error}");
    let (report, _) = slice("tilted.goo", &["--rotate", "10", "0", "0", "--drop"]);
    assert!(volume(&report) > 0.0);
    // A mirrored slab hollows as the plain one does (TD-089: hollowing runs in the model frame,
    // and the shell is mirrored after).
    let hollow_args = ["--hollow", "0.5", "--hollow-cell", "0.1"];
    let (hollow, _) = slice("hollow.goo", &hollow_args);
    let (mirrored_hollow, _) = slice(
        "mirrored-hollow.goo",
        &[&["--mirror", "x"][..], &hollow_args].concat(),
    );
    let cavity = |report: &str| -> f64 { field(report, "Cavity volume (mm^3)").parse().unwrap() };
    let (plain_cavity, mirrored_cavity) = (cavity(&hollow), cavity(&mirrored_hollow));
    assert!(plain_cavity > 0.0);
    // Recomputed from the mirrored surfaces, the volume agrees to rounding.
    assert!((mirrored_cavity - plain_cavity).abs() < 1e-9 * plain_cavity);
    // Invalid factors are usage errors.
    for extra in [
        &["--scale", "0", "1", "1"][..],
        &["--scale", "-1", "1", "1"],
        &["--mirror", "w"],
        &["--mirror", "xx"],
        &["--rotate", "1", "2"],
    ] {
        let output = setup.temp.path("refused.goo");
        failure(&setup.slice(&model, "goo-v3", &output, extra), 2);
        assert!(!output.exists());
    }
}

#[test]
fn floating_and_buried_models_are_refused_with_the_exact_correction() {
    let setup = Setup::new();
    let floating = setup.temp.write(
        "floating.stl",
        box_stl([10., 20., 0.1], [30., 30., 2.], false),
    );
    let output = setup.temp.path("out.ctb");
    let error = failure(&setup.slice(&floating, "ctb-v2-plain", &output, &[]), 1);
    assert!(error.contains("must rest on the build plate"), "{error}");
    assert!(
        error.contains("use --drop, or --translate 0 0 -0.1"),
        "{error}"
    );
    assert!(!output.exists());

    let error = failure(
        &setup.slice(
            &setup.cube(),
            "ctb-v2-plain",
            &output,
            &["--translate", "0", "0", "-1"],
        ),
        1,
    );
    assert!(error.contains("world minimum Z is -1 mm"), "{error}");
    assert!(
        error.contains("use --drop, or --translate 0 0 0"),
        "{error}"
    );
    assert!(!output.exists());

    // The suggested translation replaces the given one, keeping X/Y (review finding: adding the
    // world offset to the old Z failed again whenever that Z was not zero).
    let error = failure(
        &setup.slice(
            &floating,
            "ctb-v2-plain",
            &output,
            &["--translate", "5", "5", "1"],
        ),
        1,
    );
    assert!(error.contains("world minimum Z is 1.1 mm"), "{error}");
    assert!(
        error.contains("use --drop, or --translate 5 5 -0.1"),
        "{error}"
    );
    let report = success(&setup.slice(
        &floating,
        "ctb-v2-plain",
        &output,
        &["--translate", "5", "5", "-0.1"],
    ));
    assert_eq!(field(&report, "Layers"), "8");
}

#[test]
fn geometry_outside_the_lcd_or_build_height_is_refused() {
    let setup = Setup::new();
    let output = setup.temp.path("out.goo");
    let error = failure(
        &setup.slice(
            &setup.cube(),
            "goo-v3",
            &output,
            &["--translate", "80", "0", "0"],
        ),
        1,
    );
    assert!(error.contains("outside the LCD area"), "{error}");
    let tall = setup
        .temp
        .write("tall.stl", box_stl([10., 20., 0.], [30., 30., 60.], false));
    let error = failure(&setup.slice(&tall, "goo-v3", &output, &[]), 1);
    assert!(
        error.contains("exceeds the printer Z build size"),
        "{error}"
    );
    assert!(!output.exists());
}

#[test]
fn existing_output_is_never_replaced() {
    let setup = Setup::new();
    let model = setup.cube();
    let file = setup.temp.write("taken.ctb", b"keep");
    let error = failure(&setup.slice(&model, "ctb-v2-plain", &file, &[]), 1);
    assert!(error.contains("already exists"), "{error}");
    assert_eq!(fs::read(&file).unwrap(), b"keep");

    let directory = setup.temp.path("taken");
    fs::create_dir(&directory).unwrap();
    failure(&setup.slice(&model, "reference", &directory, &[]), 1);
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);
}

#[test]
fn open_meshes_close_their_sections_under_the_gap_bound() {
    let setup = Setup::new();
    // Open top: the rim lies at the top height, so no layer needs a chord.
    let open = setup
        .temp
        .write("open.stl", box_stl([10., 20., 0.], [30., 30., 2.], true));
    let output = setup.temp.path("open-union");
    let report = success(&setup.slice(&open, "reference", &output, &[]));
    assert_eq!(field(&report, "Layers"), "8");
    assert!(field(&report, "Mesh defects").starts_with("4 boundary edges"));
    assert_eq!(field(&report, "Closed sections"), "none");
    // Without section closure the union refuses holes and names the option.
    let refused = setup.temp.path("open-refused");
    let error = failure(
        &setup.slice(&open, "reference", &refused, &["--no-close-holes"]),
        1,
    );
    assert!(error.contains("--close-holes"), "{error}");
    assert!(!refused.exists());
    // Strict filling still requires a closed manifold or explicit acceptance.
    let strict = setup.temp.path("open-strict");
    let error = failure(
        &setup.slice(&open, "reference", &strict, &["--fill", "strict"]),
        1,
    );
    assert!(error.contains("not a closed manifold surface"), "{error}");
    assert!(error.contains("boundary edges: 4"), "{error}");
    success(&setup.slice(
        &open,
        "reference",
        &strict,
        &["--fill", "strict", "--allow-topology-defects"],
    ));

    // Missing X walls: two 10 mm wide holes crossed by every layer.
    let mut text = box_stl([10., 20., 0.], [30., 30., 2.], false);
    let first_side = text.match_indices("facet").nth(16).unwrap().0;
    text.truncate(first_side);
    text.push_str("endsolid box\n");
    let walls = setup.temp.write("walls.stl", text);
    // The default 2 mm bound refuses the 10 mm chords with their position.
    let output = setup.temp.path("walls-default.goo");
    let error = failure(&setup.slice(&walls, "goo-v3", &output, &[]), 1);
    assert!(error.contains("10 mm chord"), "{error}");
    assert!(!output.exists());
    // A wider bound closes them; the layers are the complete box's, byte for byte. The previews
    // show the open source surface that was sliced.
    let closed = setup.temp.path("walls-closed.goo");
    let report = success(&setup.slice(&walls, "goo-v3", &closed, &["--close-holes", "12"]));
    assert_eq!(
        field(&report, "Closed sections"),
        "8 layers, 16 chords, longest 10 mm at layer 0 (bound 12 mm)"
    );
    let complete = setup.temp.path("complete.goo");
    success(&setup.slice(&setup.cube(), "goo-v3", &complete, &[]));
    let (closed, complete) = (fs::read(&closed).unwrap(), fs::read(&complete).unwrap());
    assert_ne!(goo_previews(&closed), goo_previews(&complete));
    assert_eq!(
        goo_without_previews(&closed),
        goo_without_previews(&complete)
    );
    // Strict filling cannot resolve the open sections even when defects are accepted.
    let strict = setup.temp.path("walls-strict");
    let error = failure(
        &setup.slice(
            &walls,
            "reference",
            &strict,
            &["--fill", "strict", "--allow-topology-defects"],
        ),
        1,
    );
    assert!(error.contains("unresolved"), "{error}");
}

#[test]
fn repairs_fix_reversed_faces_and_inside_out_bodies() {
    let setup = Setup::new();
    let complete = setup.temp.path("complete.goo");
    success(&setup.slice(&setup.cube(), "goo-v3", &complete, &[]));
    // One reversed triangle: flipped back by the default repairs, identical to the clean box.
    let text = box_stl([10., 20., 0.], [30., 30., 2.], false);
    let lines: Vec<&str> = text.lines().collect();
    let mut reversed = lines.clone();
    // Facet 4 occupies lines 1 + 7 * 4 onward; swap its second and third vertices.
    reversed.swap(1 + 7 * 4 + 3, 1 + 7 * 4 + 4);
    let model = setup.temp.write("reversed.stl", reversed.join("\n") + "\n");
    let output = setup.temp.path("reversed.goo");
    let report = success(&setup.slice(&model, "goo-v3", &output, &[]));
    assert_eq!(field(&report, "Mesh defects"), "3 winding conflicts");
    assert!(field(&report, "Repairs").starts_with("1 faces flipped"));
    assert_eq!(field(&report, "Remaining defects"), "none");
    assert_eq!(fs::read(&output).unwrap(), fs::read(&complete).unwrap());
    // Without repairs the conflicts are refused, naming the repair.
    let refused = setup.temp.path("reversed-refused.goo");
    let error = failure(
        &setup.slice(&model, "goo-v3", &refused, &["--repair", "none"]),
        1,
    );
    assert!(error.contains("--repair orientation"), "{error}");
    // An inside-out box (every facet reversed): negative winding by default, with the hint;
    // turned outward by the repair.
    let mut text = String::from("solid box\n");
    for facet in lines[1..lines.len() - 1].chunks(7) {
        let mut facet = facet.to_vec();
        facet.swap(3, 4);
        text.push_str(&(facet.join("\n") + "\n"));
    }
    text.push_str("endsolid box\n");
    let model = setup.temp.write("inverted.stl", text);
    let output = setup.temp.path("inverted.goo");
    let error = failure(&setup.slice(&model, "goo-v3", &output, &[]), 1);
    assert!(error.contains("--repair outward"), "{error}");
    let report = success(&setup.slice(&model, "goo-v3", &output, &["--repair", "outward"]));
    assert!(field(&report, "Repairs").contains("1 turned outward"));
    assert_eq!(fs::read(&output).unwrap(), fs::read(&complete).unwrap());
}

#[test]
fn filled_holes_slice_and_hollow_like_closed_models() {
    let setup = Setup::new();
    let open = setup.temp.write(
        "open-top.stl",
        box_stl([10., 10., 0.], [30., 30., 10.], true),
    );
    let closed = setup.temp.write(
        "closed.stl",
        box_stl([10., 10., 0.], [30., 30., 10.], false),
    );
    let fill = ["--repair", "duplicates,orientation,fill-holes"];
    // Slicing: the capped box equals the closed box byte for byte.
    let filled = setup.temp.path("filled.goo");
    let report = success(&setup.slice(&open, "goo-v3", &filled, &fill));
    assert!(field(&report, "Repairs").starts_with("1 holes filled (2 cap triangles, 400 mm^2"));
    assert_eq!(field(&report, "Remaining defects"), "none");
    let reference = setup.temp.path("closed.goo");
    success(&setup.slice(&closed, "goo-v3", &reference, &[]));
    assert_eq!(fs::read(&filled).unwrap(), fs::read(&reference).unwrap());
    // Hollowing an open model names the repair it needs.
    let refused = setup.temp.path("hollow-open.goo");
    let error = failure(
        &setup.slice(&open, "goo-v3", &refused, &["--hollow", "2"]),
        1,
    );
    assert!(error.contains("add fill-holes to --repair"), "{error}");
    assert!(!refused.exists());
    // With the hole filled, the plan's repair makes it hollow like the closed box.
    let hollow = setup.temp.path("hollow-filled.goo");
    let report = success(&setup.slice(
        &open,
        "goo-v3",
        &hollow,
        &[&fill[..], &["--hollow", "2"]].concat(),
    ));
    let closed_hollow = setup.temp.path("hollow-closed.goo");
    let reference = success(&setup.slice(&closed, "goo-v3", &closed_hollow, &["--hollow", "2"]));
    assert_eq!(
        field(&report, "Cavity volume (mm^3)"),
        field(&reference, "Cavity volume (mm^3)")
    );
    assert_eq!(
        fs::read(&hollow).unwrap(),
        fs::read(&closed_hollow).unwrap()
    );
}

/// Two overlapping boxes in one STL: [10, 30] x [20, 30] and [20, 40] x [25, 35], 2 mm tall.
fn overlapping_boxes() -> String {
    let first = box_stl([10., 20., 0.], [30., 30., 2.], false);
    let second = box_stl([20., 25., 0.], [40., 35., 2.], false);
    let body = |text: &str| {
        text.strip_prefix("solid box\n")
            .and_then(|t| t.strip_suffix("endsolid box\n"))
            .unwrap()
            .to_owned()
    };
    format!(
        "solid boxes\n{}{}endsolid boxes\n",
        body(&first),
        body(&second)
    )
}

#[test]
fn union_fill_merges_overlapping_bodies_that_strict_fill_refuses() {
    let setup = Setup::new();
    let model = setup.temp.write("overlap.stl", overlapping_boxes());
    let output = setup.temp.path("overlap.goo");
    let report = success(&setup.slice(&model, "goo-v3", &output, &[]));
    assert_eq!(field(&report, "Fill"), "union");
    // 200 + 200 - 50 mm^2 of union per layer, over 2 mm.
    assert_eq!(field(&report, "Sampled mask volume (mm^3)"), "700");
    assert_eq!(
        field(&report, "Layers with merged overlapping material"),
        "8"
    );
    let strict = setup.temp.path("overlap-strict.goo");
    let error = failure(
        &setup.slice(&model, "goo-v3", &strict, &["--fill", "strict"]),
        1,
    );
    assert!(error.contains("unresolved"), "{error}");
    assert!(!strict.exists());
}

#[test]
fn union_and_strict_fills_publish_identical_files_for_embedded_models() {
    let setup = Setup::new();
    // The step model has degenerate triangles, which only strict filling refuses by default.
    let step = setup.temp.write("step.stl", step_stl(1.));
    for (name, model, flags) in [
        ("box", setup.cube(), &[][..]),
        ("step", step, &["--allow-topology-defects"][..]),
    ] {
        let union = setup.temp.path(&format!("{name}-union.goo"));
        let strict = setup.temp.path(&format!("{name}-strict.goo"));
        let report = success(&setup.slice(&model, "goo-v3", &union, flags));
        assert_eq!(
            field(&report, "Layers with merged overlapping material"),
            "0"
        );
        let strict_flags = [flags, &["--fill", "strict"]].concat();
        success(&setup.slice(&model, "goo-v3", &strict, &strict_flags));
        assert_eq!(
            fs::read(&union).unwrap(),
            fs::read(&strict).unwrap(),
            "{name}"
        );
    }
}

#[test]
fn antialiasing_supersamples_masks_and_is_recorded() {
    let setup = Setup::new();
    let model = setup.cube();
    // A quarter-pixel shift leaves partially covered columns on both X edges.
    let shift = ["--translate", "0.25", "0", "0"];
    let reference = setup.temp.path("aa-ref");
    let report = success(&setup.slice(
        &model,
        "reference",
        &reference,
        &[&shift[..], &["--antialias", "4"]].concat(),
    ));
    assert_eq!(field(&report, "Antialiasing (samples per pixel)"), "16");
    // Exact sampling of a 20 x 10 x 2 mm box: the shift moves material, never adds or removes it.
    assert_eq!(field(&report, "Sampled mask volume (mm^3)"), "400");
    let layers = fs::read_to_string(reference.join("layers.jsonl")).unwrap();
    assert!(layers.contains("\"samples_per_axis\":4"), "{layers}");
    let goo = setup.temp.path("aa.goo");
    success(&setup.slice(
        &model,
        "goo-v3",
        &goo,
        &[&shift[..], &["--antialias", "4"]].concat(),
    ));
    let bytes = fs::read(&goo).unwrap();
    assert_eq!(u16::from_be_bytes([bytes[188], bytes[189]]), 4);
    // Binary masks stay the default and are recorded as such.
    let binary = setup.temp.path("binary.goo");
    let report = success(&setup.slice(&model, "goo-v3", &binary, &shift));
    assert_eq!(field(&report, "Antialiasing (samples per pixel)"), "1");
    let bytes = fs::read(&binary).unwrap();
    assert_eq!(u16::from_be_bytes([bytes[188], bytes[189]]), 1);
    // Every grayscale format accepts antialiased masks.
    for format in ["ctb-v2-plain", "anycubic-pp1-v3"] {
        let extension = if format == "ctb-v2-plain" {
            "ctb"
        } else {
            "pp1"
        };
        let output = setup.temp.path(&format!("aa.{extension}"));
        success(&setup.slice(
            &model,
            format,
            &output,
            &[&shift[..], &["--antialias", "2"]].concat(),
        ));
    }
    // Legacy Photon stores binary masks only; unsupported levels are usage errors.
    for (format, level) in [("photon-v1-firmware-motion", "2"), ("goo-v3", "3")] {
        let output = setup.temp.path("refused");
        let error = failure(
            &setup.slice(&model, format, &output, &["--antialias", level]),
            2,
        );
        assert!(error.contains("--antialias"), "{error}");
        assert!(!output.exists());
    }
}

#[test]
fn summary_reports_resin_volume_mass_and_cost() {
    let number = |report: &str, name: &str| -> f64 { field(report, name).parse().unwrap() };
    // Without density or cost only the volume is reported.
    let setup = Setup::new();
    let output = setup.temp.path("plain.goo");
    let report = success(&setup.slice(&setup.cube(), "goo-v3", &output, &[]));
    assert_eq!(number(&report, "Sampled mask volume (mm^3)"), 400.0);
    assert!((number(&report, "Resin volume (ml)") - 0.4).abs() < 1e-12);
    assert!(
        !report.contains("Resin mass") && !report.contains("Resin cost"),
        "{report}"
    );
    let mut priced = resin(PRINTER_ID);
    priced.density_g_per_ml = Some(1.25);
    priced.cost_per_liter = Some(40.0);
    priced.currency = Some("EUR".into());
    let setup = Setup::with_resin(priced);
    let output = setup.temp.path("priced.goo");
    let report = success(&setup.slice(&setup.cube(), "goo-v3", &output, &[]));
    assert!((number(&report, "Resin mass (g)") - 0.5).abs() < 1e-12);
    assert!((number(&report, "Resin cost (EUR)") - 0.016).abs() < 1e-12);
}

#[test]
fn xy_compensation_grows_or_shrinks_layers_and_bottom_layers_separately() {
    let setup = Setup::new();
    let model = setup.cube();
    // 1 mm pixels; the 20 x 10 mm box covers 200 pixels in each of its 8 layers of 0.25 mm.
    let run = |name: &str, extra: &[&str]| {
        let output = setup.temp.path(name);
        let report = success(&setup.slice(&model, "reference", &output, extra));
        let volume: f64 = field(&report, "Sampled mask volume (mm^3)")
            .parse()
            .unwrap();
        let layers = fs::read_to_string(output.join("layers.jsonl")).unwrap();
        (report, volume, layers)
    };
    let (report, volume, _) = run("plain", &[]);
    assert_eq!(volume, 400.0);
    assert!(!report.contains("XY compensation"), "{report}");
    // One pixel outward: 22 x 12 pixels per layer.
    let (report, volume, _) = run("grown", &["--xy-compensation", "1"]);
    assert_eq!(volume, 22.0 * 12.0 * 2.0);
    assert_eq!(
        field(&report, "XY compensation"),
        "1 px (1 mm in X, 1 mm in Y)"
    );
    // One pixel inward: 18 x 8 pixels per layer.
    let (_, volume, _) = run("shrunk", &["--xy-compensation", "-1"]);
    assert_eq!(volume, 18.0 * 8.0 * 2.0);
    // Elephant-foot correction: only the one bottom layer shrinks.
    let (report, volume, layers) = run("foot", &["--bottom-xy-compensation", "-1"]);
    assert_eq!(volume, 18.0 * 8.0 * 0.25 + 200.0 * 0.25 * 7.0);
    assert_eq!(
        field(&report, "XY compensation"),
        "0 px (0 mm in X, 0 mm in Y)"
    );
    assert_eq!(
        field(&report, "Bottom XY compensation"),
        "-1 px (-1 mm in X, -1 mm in Y) on the first 1 layers"
    );
    let kinds: Vec<bool> = layers
        .lines()
        .map(|line| line.contains("\"kind\":\"erode\""))
        .collect();
    assert_eq!(
        kinds,
        [true, false, false, false, false, false, false, false]
    );
    // Both together: the bottom value replaces the general one on the bottom layers.
    let (_, volume, _) = run(
        "both",
        &["--xy-compensation", "1", "--bottom-xy-compensation", "-1"],
    );
    assert_eq!(volume, 18.0 * 8.0 * 0.25 + 22.0 * 12.0 * 0.25 * 7.0);
    for bad in ["65", "-65", "0.5", "x"] {
        let output = setup.temp.path("refused");
        failure(
            &setup.slice(&model, "reference", &output, &["--xy-compensation", bad]),
            2,
        );
        assert!(!output.exists());
    }
}

fn support_document(body: &str) -> String {
    format!(r#"{{"format":"karasu-supports","version":1,{body}}}"#)
}

#[test]
fn manual_supports_hold_a_lifted_model() {
    let setup = Setup::new();
    // A 20 x 10 x 2 mm slab floating at Z 5 to 7.
    let slab = setup
        .temp
        .write("slab.stl", box_stl([10., 20., 5.], [30., 30., 7.], false));
    let supports = setup.temp.write(
        "supports.json",
        support_document(
            r#""supports":[{"id":1,"tip":{"point":[15,25,5]},"base":{"vertical":true}},
                {"id":2,"tip":{"point":[25,25,5]},"base":{"vertical":true},"parameters":{"contact_shape":"sphere"}}]"#,
        ),
    );
    let output = setup.temp.path("supported.goo");
    let report = success(&setup.slice(
        &slab,
        "goo-v3",
        &output,
        &["--supports", supports.to_str().unwrap()],
    ));
    assert_eq!(
        field(&report, "Supports"),
        "2 (plate 2, model 0, on supports 0; interior 0), braces 0, raft no, parts 13"
    );
    assert_eq!(field(&report, "Layers"), "28");
    let volume: f64 = field(&report, "Sampled mask volume (mm^3)")
        .parse()
        .unwrap();
    assert!(volume > 400.0, "{volume}");
    // Without supports the same floating slab is refused.
    let error = failure(
        &setup.slice(&slab, "goo-v3", &setup.temp.path("floating.goo"), &[]),
        1,
    );
    assert!(error.contains("must rest on the build plate"), "{error}");
    // Strict filling cannot merge supports; acceptance needs a document.
    for extra in [
        &["--supports", supports.to_str().unwrap(), "--fill", "strict"][..],
        &["--accept-support-collisions"],
    ] {
        failure(
            &setup.slice(&slab, "goo-v3", &setup.temp.path("refused.goo"), extra),
            2,
        );
    }
    // --drop --lift raises a plate-resting box to rest on the same supports.
    let low = setup
        .temp
        .write("low.stl", box_stl([10., 20., 0.], [30., 30., 2.], false));
    let lifted = setup.temp.path("lifted.goo");
    let report = success(&setup.slice(
        &low,
        "goo-v3",
        &lifted,
        &[
            "--drop",
            "--lift",
            "5",
            "--supports",
            supports.to_str().unwrap(),
        ],
    ));
    assert_eq!(field(&report, "Translation (mm)"), "0 0 5");
    assert_eq!(
        goo_without_previews(&fs::read(&lifted).unwrap()),
        goo_without_previews(&fs::read(&output).unwrap())
    );
}

#[test]
fn supports_are_binary_unless_antialiased_with_the_model() {
    // TD-134: a slab floating at Z 5 to 7, a quarter pixel off the grid, on two supports.
    let setup = Setup::new();
    let slab = setup.temp.write(
        "aa-slab.stl",
        box_stl([10.25, 20., 5.], [30.25, 30., 7.], false),
    );
    let supports = setup.temp.write(
        "aa-supports.json",
        support_document(
            r#""supports":[{"id":1,"tip":{"point":[15,25,5]},"base":{"vertical":true}},
                {"id":2,"tip":{"point":[25,25,5]},"base":{"vertical":true}}]"#,
        ),
    );
    // Per layer: its height, lit pixels and intensity sum.
    let layers = |extra: &[&str]| -> Vec<(f64, u64, u64)> {
        let output = setup.temp.path(&format!("aa-supports-{}", extra.len()));
        let arguments = [
            &["--supports", supports.to_str().unwrap(), "--antialias", "4"][..],
            extra,
        ]
        .concat();
        success(&setup.slice(&slab, "reference", &output, &arguments));
        let text = fs::read_to_string(output.join("layers.jsonl")).unwrap();
        let number = |line: &str, key: &str| -> f64 {
            let start = line.find(key).unwrap() + key.len();
            line[start..]
                .split(|c: char| !(c.is_ascii_digit() || c == '.'))
                .next()
                .unwrap()
                .parse()
                .unwrap()
        };
        text.lines()
            .map(|line| {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let count = |key| number(line, key) as u64;
                (
                    number(line, "\"position_z_mm\":"),
                    count("\"nonzero_pixels\":"),
                    count("\"intensity_sum\":"),
                )
            })
            .collect()
    };
    let gray = |&(_, lit, sum): &(f64, u64, u64)| sum < 255 * lit;
    let binary = layers(&[]);
    let antialiased = layers(&["--antialias-supports"]);
    let below = |all: &[(f64, u64, u64)]| -> Vec<(f64, u64, u64)> {
        all.iter()
            .copied()
            .filter(|l| l.0 < 4.0 && l.1 > 0)
            .collect()
    };
    // Below the slab only the supports print: binary, no gray; anti-aliased, gray at their edges.
    assert_ne!(below(&binary).len(), 0);
    assert!(!below(&binary).iter().any(gray), "{:?}", below(&binary));
    assert!(below(&antialiased).iter().any(gray));
    // The slab's own edges, a quarter pixel off the grid, stay anti-aliased either way.
    let slab_layers = |all: &[(f64, u64, u64)]| -> Vec<(f64, u64, u64)> {
        all.iter().copied().filter(|l| l.0 > 5.5).collect()
    };
    assert!(slab_layers(&binary).iter().all(gray));
    assert_eq!(slab_layers(&binary), slab_layers(&antialiased));
}

#[test]
fn supports_bound_to_a_pose_follow_moves_on_the_plate() {
    let setup = Setup::new();
    let slab = setup
        .temp
        .write("slab.stl", box_stl([10., 20., 5.], [30., 30., 7.], false));
    let tips = r#""supports":[{"id":1,"tip":{"point":[15,25,5]},"base":{"vertical":true}},
        {"id":2,"tip":{"point":[25,25,5]},"base":{"vertical":true}}]"#;
    let bound = setup.temp.write(
        "bound.json",
        support_document(&format!(
            r#""binding":{{"translation_mm":[0,0,0],"rotate_deg":[0,0,0]}},{tips}"#
        )),
    );
    let report = success(&setup.slice(
        &slab,
        "goo-v3",
        &setup.temp.path("moved.goo"),
        &[
            "--translate",
            "40",
            "10",
            "0",
            "--supports",
            bound.to_str().unwrap(),
        ],
    ));
    assert_eq!(
        field(&report, "Supports follow the model"),
        "turned 0 degrees, from (20, 25) to (60, 35) mm"
    );
    // A translation-only binding still requires exactly the recorded placement.
    let exact = setup.temp.write(
        "exact.json",
        support_document(&format!(r#""binding":{{"translation_mm":[0,0,0]}},{tips}"#)),
    );
    let error = failure(
        &setup.slice(
            &slab,
            "goo-v3",
            &setup.temp.path("refused.goo"),
            &[
                "--translate",
                "40",
                "10",
                "0",
                "--supports",
                exact.to_str().unwrap(),
            ],
        ),
        1,
    );
    assert!(error.contains("was written for translation"), "{error}");
}

#[test]
fn support_findings_refuse_or_record_problems() {
    let setup = Setup::new();
    // A slab over a block: a vertical support at (15, 25) passes through the block.
    let mut text = box_stl([10., 20., 5.], [30., 30., 7.], false).replace("endsolid box\n", "");
    text.push_str(&box_stl([13., 23., 0.], [17., 27., 1.], false).replacen("solid box\n", "", 1));
    let model = setup.temp.write("blocked.stl", text);
    let document = |accept: &str| {
        support_document(&format!(
            r#""supports":[{{"id":1,"tip":{{"point":[15,25,5]}},"base":{{"vertical":true}}{accept}}},
                {{"id":2,"tip":{{"point":[25,25,5]}},"base":{{"vertical":true}}}}]"#
        ))
    };
    let blocked = setup.temp.write("blocked.json", document(""));
    let output = setup.temp.path("blocked.goo");
    let args = ["--supports", blocked.to_str().unwrap()];
    let error = failure(&setup.slice(&model, "goo-v3", &output, &args), 1);
    assert!(
        error.contains("cannot be printed") && error.contains("support 1"),
        "{error}"
    );
    assert!(!output.exists());
    let report = success(&setup.slice(
        &model,
        "goo-v3",
        &output,
        &[&args[..], &["--accept-support-collisions"]].concat(),
    ));
    assert!(
        field(&report, "Support findings").contains("accepted collisions"),
        "{report}"
    );
    assert!(
        report.contains("Support finding: accepted: support 1"),
        "{report}"
    );
    // Accepting per support in the document records the same.
    let accepted = setup
        .temp
        .write("accepted.json", document(r#","accept_collision":true"#));
    let output = setup.temp.path("accepted.goo");
    success(&setup.slice(
        &model,
        "goo-v3",
        &output,
        &["--supports", accepted.to_str().unwrap()],
    ));
    // A contact off the model surface.
    let off = setup.temp.write(
        "off.json",
        support_document(
            r#""supports":[{"id":3,"tip":{"point":[25,25,4]},"base":{"vertical":true}}]"#,
        ),
    );
    let error = failure(
        &setup.slice(
            &model,
            "goo-v3",
            &setup.temp.path("off.goo"),
            &["--supports", off.to_str().unwrap()],
        ),
        1,
    );
    assert!(error.contains("not on the model surface"), "{error}");
}

#[test]
fn supports_stand_inside_hollowed_models() {
    let setup = Setup::new();
    let cube = setup
        .temp
        .write("cube.stl", box_stl([10., 10., 0.], [30., 30., 12.], false));
    // A 3 mm pillar (wider than the 1 mm pixels) from the cavity ceiling (Z 10) to its floor
    // (Z 2), inside a 2 mm wall.
    let supports = setup.temp.write(
        "inner.json",
        support_document(
            r#""anchor_tolerance_mm":0.2,"supports":[{"id":1,"tip":{"point":[20,20,10]},"base":{"model":[20,20,2]},
                "parameters":{"shaft_diameter_mm":3,"tip_body_diameter_mm":3,"foot_body_diameter_mm":3}}]"#,
        ),
    );
    let output = setup.temp.path("inner.goo");
    let report = success(&setup.slice(
        &cube,
        "goo-v3",
        &output,
        &["--hollow", "2", "--supports", supports.to_str().unwrap()],
    ));
    assert_eq!(
        field(&report, "Supports"),
        "1 (plate 0, model 1, on supports 0; interior 1), braces 0, raft no, parts 5"
    );
    // The pillar adds material inside the cavity.
    let hollow = setup.temp.path("plain.goo");
    let plain = success(&setup.slice(&cube, "goo-v3", &hollow, &["--hollow", "2"]));
    let volume = |r: &str| -> f64 { field(r, "Sampled mask volume (mm^3)").parse().unwrap() };
    assert!(volume(&report) > volume(&plain), "{report}");
    // A mini support (owner, 2026-10-30) in its place: a 2 mm rod with a ball at each end.
    let mini = setup.temp.write(
        "mini.json",
        support_document(
            r#""anchor_tolerance_mm":0.2,"supports":[{"id":1,"kind":"mini","tip":{"point":[20,20,10]},"base":{"model":[20,20,2]},
                "parameters":{"mini_diameter_mm":2}}]"#,
        ),
    );
    let report = success(&setup.slice(
        &cube,
        "goo-v3",
        &setup.temp.path("mini.goo"),
        &["--hollow", "2", "--supports", mini.to_str().unwrap()],
    ));
    assert_eq!(
        field(&report, "Supports"),
        "1 (plate 0, model 1, on supports 0; mini 1; interior 1), braces 0, raft no, parts 5"
    );
    assert!(volume(&report) > volume(&plain), "{report}");
}

#[test]
fn minimum_gray_raises_partial_edge_pixels() {
    let setup = Setup::new();
    let model = setup.cube();
    let shift = ["--translate", "0.25", "0", "0", "--antialias", "4"];
    // Per-layer statistics: (nonzero pixels, intensity sum).
    let statistics = |extra: &[&str]| {
        let output = setup.temp.path(&format!("min-gray-{}", extra.join("-")));
        let report =
            success(&setup.slice(&model, "reference", &output, &[&shift[..], extra].concat()));
        let layers = fs::read_to_string(output.join("layers.jsonl")).unwrap();
        let numbers = |line: &str, key: &str| -> u64 {
            let start = line.find(key).unwrap() + key.len();
            line[start..]
                .split(|c: char| !c.is_ascii_digit())
                .next()
                .unwrap()
                .parse()
                .unwrap()
        };
        let stats: Vec<(u64, u64)> = layers
            .lines()
            .map(|line| {
                (
                    numbers(line, "\"nonzero_pixels\":"),
                    numbers(line, "\"intensity_sum\":"),
                )
            })
            .collect();
        (report, layers, stats)
    };
    let (report, layers, linear) = statistics(&[]);
    assert!(!layers.contains("minimum_gray"), "{layers}");
    assert!(!report.contains("Minimum edge gray"), "{report}");
    let (report, layers, raised) = statistics(&["--min-gray", "100"]);
    assert_eq!(field(&report, "Minimum edge gray (0-255)"), "100");
    assert!(layers.contains("\"minimum_gray\":100"), "{layers}");
    // The same pixels are lit; the partial ones are brighter, so the mask volume grows.
    for (linear, raised) in linear.iter().zip(&raised) {
        assert_eq!(linear.0, raised.0);
        assert!(raised.1 > linear.1);
    }
    let volume: f64 = field(&report, "Sampled mask volume (mm^3)")
        .parse()
        .unwrap();
    assert!(volume > 400.0, "{volume}");
    // At the maximum every lit pixel is fully exposed.
    let (_, _, full) = statistics(&["--min-gray", "255"]);
    assert!(full.iter().all(|&(nonzero, sum)| sum == 255 * nonzero));
    // Binary masks have no partial pixels: the option is a usage error, and nothing is written.
    let output = setup.temp.path("min-gray-binary.goo");
    let error = failure(
        &setup.slice(&model, "goo-v3", &output, &["--min-gray", "100"]),
        2,
    );
    assert!(error.contains("requires --antialias"), "{error}");
    assert!(!output.exists());
}

#[test]
fn profile_kind_and_calibration_mismatches_are_reported() {
    let setup = Setup::with_resin(resin("other-printer"));
    let model = setup.cube();
    let output = setup.temp.path("out.ctb");
    let error = failure(&setup.slice(&model, "ctb-v2-plain", &output, &[]), 1);
    assert!(
        error.contains("cannot be used with this printer"),
        "{error}"
    );

    let swapped = Command::new(env!("CARGO_BIN_EXE_karasu-cli"))
        .arg("slice")
        .arg("--assume-mm")
        .arg(&model)
        .arg("--printer")
        .arg(&setup.resin)
        .arg("--resin")
        .arg(&setup.printer)
        .args(["--format", "ctb-v2-plain", "--output"])
        .arg(&output)
        .output()
        .unwrap();
    assert!(failure(&swapped, 1).contains("--printer requires kind \"printer\""));

    let invalid = setup.temp.write("invalid.json", b"{\"kind\":\"printer\"}");
    let output_invalid = Command::new(env!("CARGO_BIN_EXE_karasu-cli"))
        .arg("slice")
        .arg("--assume-mm")
        .arg(&model)
        .arg("--printer")
        .arg(&invalid)
        .arg("--resin")
        .arg(&setup.resin)
        .args(["--format", "ctb-v2-plain", "--output"])
        .arg(&output)
        .output()
        .unwrap();
    assert!(failure(&output_invalid, 1).contains("invalid printer profile"));
    assert!(!output.exists());
}

#[test]
fn usage_errors_exit_with_status_two_before_any_work() {
    let base = [
        "slice",
        "--assume-mm",
        "m.stl",
        "--printer",
        "p.json",
        "--resin",
        "r.json",
        "--output",
        "o.ctb",
    ];
    let with = |extra: &[&str]| run(base.iter().chain(extra));
    assert!(failure(&run(base), 2).contains("slice requires --format"));
    assert!(failure(&with(&["--format", "stl"]), 2).contains("unsupported --format"));
    assert!(failure(&with(&["--format", "goo-v3", "--format", "goo-v3"]), 2).contains("only once"));
    assert!(failure(
        &with(&["--format", "goo-v3", "--translate", "1", "nan", "0"]),
        2
    )
    .contains("Y is invalid"));
    assert!(
        failure(&with(&["--format", "goo-v3", "--translate", "1"]), 2).contains("Y is invalid")
    );
    assert!(failure(&with(&["--format", "goo-v3", "--colour"]), 2).contains("unknown slice option"));
}

#[test]
fn help_documents_slice_and_every_format() {
    let help = success(&run(["--help"]));
    assert!(help.contains(
        "karasu-cli slice (--assume-mm <model> | --model <model> [--units <unit>] | --plate <plate.json>)"
    ));
    for option in ["--layer-plan", "--copies", "--arrange", "--spacing"] {
        assert!(help.contains(option), "{option}");
    }
    assert!(help.contains("STL, OBJ or 3MF"));
    for (format, _) in MACHINE_FORMATS.iter().chain(&SL1_FORMATS) {
        assert!(help.contains(format), "{format}");
    }
    assert!(help.contains("reference"));
    assert!(help.contains("--allow-topology-defects"));
}

#[test]
fn legacy_alias_slices_identically() {
    let setup = Setup::new();
    let model = setup.cube();
    let primary = setup.temp.path("primary.goo");
    let legacy = setup.temp.path("legacy.goo");
    success(&setup.slice(&model, "goo-v3", &primary, &[]));
    let output = Command::new(env!("CARGO_BIN_EXE_resin-cli"))
        .arg("slice")
        .arg("--assume-mm")
        .arg(&model)
        .arg("--printer")
        .arg(&setup.printer)
        .arg("--resin")
        .arg(&setup.resin)
        .args(["--format", "goo-v3", "--output"])
        .arg(&legacy)
        .output()
        .unwrap();
    success(&output);
    assert_eq!(fs::read(primary).unwrap(), fs::read(legacy).unwrap());
}

#[test]
fn explicit_progress_reports_phases_and_layers_on_stderr_only() {
    let setup = Setup::new();
    let output = setup.temp.path("progress.goo");
    let run = setup.slice(&setup.cube(), "goo-v3", &output, &["--progress"]);
    assert_eq!(run.status.code(), Some(0));
    let quiet = setup.temp.path("quiet.goo");
    let quiet_run = setup.slice(&setup.cube(), "goo-v3", &quiet, &["--no-progress"]);
    // `--no-progress` keeps stderr empty; progress never changes the stdout report contract.
    let quiet_report = success(&quiet_run).replace(&format!("{quiet:?}"), "OUTPUT");
    let report = String::from_utf8(run.stdout.clone())
        .unwrap()
        .replace(&format!("{output:?}"), "OUTPUT");
    assert_eq!(report, quiet_report);
    let progress = String::from_utf8(run.stderr).unwrap();
    let lines: Vec<_> = progress.lines().collect();
    assert_eq!(
        lines[..6],
        [
            "progress: Reading profiles...",
            "progress: Importing model...",
            "progress: Checking topology...",
            "progress: Rendering previews...",
            "progress: Planning layers...",
            "progress: Slicing layer 0/8 (0%)",
        ]
    );
    assert_eq!(lines.last(), Some(&"progress: Slicing layer 8/8 (100%)"));
    assert!(lines.iter().all(|line| line.starts_with("progress: ")));
}

#[cfg(unix)]
#[test]
fn sigint_cancels_cooperatively_and_publishes_nothing() {
    use std::{
        io::{BufRead, BufReader, Read},
        process::Stdio,
    };
    let mut fine = resin(PRINTER_ID);
    fine.calibrations[0].settings.layer_height_mm = 0.01;
    let setup = Setup::with_resin(fine);
    // 4,500 layers keep the job running far longer than signal delivery takes.
    let model = setup
        .temp
        .write("tall.stl", box_stl([10., 20., 0.], [30., 30., 45.], false));
    let output = setup.temp.path("interrupted.ctb");
    let mut child = Command::new(env!("CARGO_BIN_EXE_karasu-cli"))
        .arg("slice")
        .arg("--assume-mm")
        .arg(&model)
        .arg("--printer")
        .arg(&setup.printer)
        .arg("--resin")
        .arg(&setup.resin)
        .args(["--format", "ctb-v2-plain", "--progress", "--output"])
        .arg(&output)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stderr = BufReader::new(child.stderr.take().unwrap());
    let mut seen = String::new();
    // The handler is installed before any progress is written, so layer progress proves readiness.
    while !seen.contains("Slicing layer") {
        assert_ne!(stderr.read_line(&mut seen).unwrap(), 0, "{seen}");
    }
    let status = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(status.success());
    stderr.read_to_string(&mut seen).unwrap();
    let mut stdout = Vec::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut stdout)
        .unwrap();
    assert_eq!(child.wait().unwrap().code(), Some(130), "{seen}");
    assert_eq!(stdout, [] as [u8; 0]);
    assert!(seen.contains("Interrupt received"), "{seen}");
    assert!(
        seen.ends_with("error: interrupted by Ctrl-C; no output was published\n"),
        "{seen}"
    );
    assert!(!seen.contains("panicked"));
    assert!(!output.exists());
    // Only the three inputs remain: no hidden staging file survives cancellation.
    assert_eq!(fs::read_dir(&setup.temp.0).unwrap().count(), 3);
}

#[test]
fn log_writes_the_job_and_changes_no_output() {
    // TD-096: the command line logs only with --log, and the output is the same either way.
    let setup = Setup::new();
    let model = setup.temp.write(
        "cube.stl",
        box_stl([10.0, 20.0, 0.0], [30.0, 40.0, 20.0], false),
    );
    let (plain, logged) = (setup.temp.path("plain.goo"), setup.temp.path("logged.goo"));
    let log = setup.temp.path("job.log");
    success(&setup.slice(&model, "goo-v3", &plain, &["--hollow", "2"]));
    let log_arg = log.to_str().unwrap();
    success(&setup.slice(
        &model,
        "goo-v3",
        &logged,
        &["--hollow", "2", "--log", log_arg],
    ));
    assert_eq!(fs::read(&plain).unwrap(), fs::read(&logged).unwrap());
    let text = fs::read_to_string(&log).unwrap();
    for expected in [
        "INFO  session: karasu-slicer",
        "INFO  session: command: ",
        "\"--log\"",
        "INFO  slice: slicing to goo-v3",
        "INFO  models: imported",
        "INFO  hollow: hollowed",
        "INFO  slice: sliced ",
        "INFO  slice: summary:",
        "    Hollow wall guaranteed (mm): ",
    ] {
        assert!(text.contains(expected), "{expected:?} in {text}");
    }
    // A refused job's error is logged too.
    let refused = setup.temp.path("refused.log");
    let output = setup.slice(
        &model,
        "goo-v3",
        &plain,
        &["--log", refused.to_str().unwrap()],
    );
    assert!(failure(&output, 1).contains("already exists"));
    assert!(fs::read_to_string(&refused)
        .unwrap()
        .contains("ERROR slice: "));
    // Given once, and to a file that can be written.
    let twice = setup.slice(
        &model,
        "goo-v3",
        &setup.temp.path("twice.goo"),
        &["--log", log_arg, "--log", log_arg],
    );
    assert!(failure(&twice, 2).contains("--log may be given only once"));
    let nowhere = setup.temp.path("missing/folder/job.log");
    let output = setup.slice(
        &model,
        "goo-v3",
        &setup.temp.path("nowhere.goo"),
        &["--log", nowhere.to_str().unwrap()],
    );
    assert!(failure(&output, 1).contains("cannot write the log"));
}

#[test]
fn verify_walls_reports_the_printed_wall_breaches_and_pockets() {
    // TD-095, on 1 mm pixels and 0.05 mm layers: the lattice is coarse across, fine in Z.
    let setup = Setup::new();
    let model = setup.temp.write(
        "cube.stl",
        box_stl([10.0, 20.0, 0.0], [30.0, 40.0, 20.0], false),
    );
    let verify = |name: &str, extra: &[&str]| {
        let mut args = extra.to_vec();
        args.push("--verify-walls");
        success(&setup.slice(&model, "goo-v3", &setup.temp.path(name), &args))
    };
    // Solid: nothing to measure.
    let report = verify("solid.goo", &[]);
    assert_eq!(field(&report, "Printed wall (mm)"), "no cavity");
    assert_eq!(field(&report, "Undrained pockets"), "none");
    assert_eq!(field(&report, "Wall verification"), "passed");
    // Hollowed and closed: one undrained pocket, the cavity; the printed wall within a pixel of
    // the certified one.
    let report = verify("closed.goo", &["--hollow", "2"]);
    assert!(
        field(&report, "Undrained pockets").starts_with("1 ("),
        "{report}"
    );
    assert_eq!(field(&report, "Wall breaches"), "none");
    assert_eq!(field(&report, "Wall verification"), "passed");
    let measured: f64 = field(&report, "Printed wall (mm)")
        .split(' ')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let guaranteed: f64 = field(&report, "Hollow wall guaranteed (mm)")
        .parse()
        .unwrap();
    assert!((measured - guaranteed).abs() <= 1.5, "{report}");
    // A drain hole through the top drains it, and is not a breach.
    let report = verify(
        "drained.goo",
        &[
            "--hollow",
            "2",
            "--surface-drain-hole",
            "20",
            "30",
            "25",
            "3",
        ],
    );
    assert_eq!(field(&report, "Undrained pockets"), "none", "{report}");
    assert_eq!(field(&report, "Wall breaches"), "none");
    assert_eq!(field(&report, "Wall verification"), "passed");
    // Without the option, nothing is verified.
    let output = setup.temp.path("plain.goo");
    let report = success(&setup.slice(&model, "goo-v3", &output, &["--hollow", "2"]));
    assert!(!report.contains("Wall verification"));
    // A wall thinner than a pixel is not printed: on 1 mm pixels a 0.3 mm wall leaves no pixel
    // centre in the wall, so the cavity reaches the outside. The job fails after writing its
    // output, for inspection.
    let small = setup.temp.write(
        "small.stl",
        box_stl([10.0, 20.0, 0.0], [16.0, 26.0, 6.0], false),
    );
    let output = setup.temp.path("breached.goo");
    let breached = setup.slice(
        &small,
        "goo-v3",
        &output,
        &["--hollow", "0.3", "--verify-walls"],
    );
    // The summary is printed, then the failure.
    assert_eq!(breached.status.code(), Some(1));
    let error = String::from_utf8_lossy(&breached.stderr);
    assert!(
        error.contains("wall verification failed: the cavity reaches the outside"),
        "{error}"
    );
    assert!(
        error.contains("the output was written for inspection"),
        "{error}"
    );
    assert!(output.exists());
    let report = String::from_utf8_lossy(&breached.stdout);
    assert!(
        field(&report, "Wall breaches").contains("voxels"),
        "{report}"
    );
    assert!(
        field(&report, "Wall verification").starts_with("failed"),
        "{report}"
    );
    // Once only.
    let twice = setup.slice(
        &model,
        "goo-v3",
        &setup.temp.path("twice.goo"),
        &["--verify-walls", "--verify-walls"],
    );
    assert!(failure(&twice, 2).contains("--verify-walls may be given only once"));
}

#[test]
fn hollowing_removes_the_certified_cavity_from_every_layer() {
    let setup = Setup::new();
    // 20 mm cube on 1 mm pixels: a 2 mm wall leaves a 16 mm cavity from Z 2 to 18.
    let model = setup.temp.write(
        "cube.stl",
        box_stl([10.0, 20.0, 0.0], [30.0, 40.0, 20.0], false),
    );
    let solid = setup.temp.path("solid.goo");
    let report = success(&setup.slice(&model, "goo-v3", &solid, &[]));
    assert_eq!(field(&report, "Sampled mask volume (mm^3)"), "8000");
    assert!(!report.contains("Cavity"));
    let output = setup.temp.path("hollow.goo");
    let report = success(&setup.slice(&model, "goo-v3", &output, &["--hollow", "2"]));
    let volume: f64 = field(&report, "Sampled mask volume (mm^3)")
        .parse()
        .unwrap();
    // TD-093: the wall is guaranteed by hollowing at a raised offset, on the rule's cell
    // (sqrt(0.05 * 2 / 0.39) mm: a 0.05 mm over-thickness).
    let number = |key: &str| -> f64 { field(&report, key).parse().unwrap() };
    let offset = number("Hollow offset (mm)");
    let cell = number("Hollow grid cell (mm)");
    assert!(
        (cell - (0.05f64 * 2.0 / 0.39).sqrt()).abs() < 1e-12,
        "{report}"
    );
    assert!(
        (offset - (2.0 + 0.40 * cell * cell / 2.0)).abs() < 1e-12,
        "{report}"
    );
    assert_eq!(field(&report, "Hollowing passes"), "1");
    // The cube's cavity is the cube eroded by the offset (15.9 mm); the 1 mm pixel centres still
    // see a 16 mm cavity, so the sampled material is 8000 - 16^3 up to its edges.
    let side = 20.0 - 2.0 * offset;
    assert!((volume - 3904.0).abs() < 40.0, "{report}");
    let cavity: f64 = field(&report, "Cavity volume (mm^3)").parse().unwrap();
    assert!(
        (cavity - side.powi(3)).abs() / side.powi(3) < 0.01,
        "{report}"
    );
    assert_eq!(field(&report, "Cavity surfaces"), "1");
    let wall: Vec<f64> = field(&report, "Certified wall range (mm)")
        .split(' ')
        .map(|v| v.parse().unwrap())
        .collect();
    let guaranteed = number("Hollow wall guaranteed (mm)");
    assert!(guaranteed >= 2.0 - 2e-6 && guaranteed <= offset, "{report}");
    assert_eq!(wall[0], guaranteed);
    assert!(wall[1] >= offset && wall[1] < 3.0, "{report}");
    assert!(
        number("Thickest wall at cavity vertices (mm)") >= offset,
        "{report}"
    );
    assert!(report.contains("Drainage: none"));
    // Explicit cells are honoured and recorded.
    let fine = setup.temp.path("fine.goo");
    let report = success(&setup.slice(
        &model,
        "goo-v3",
        &fine,
        &["--hollow", "2", "--hollow-cell", "0.2"],
    ));
    assert_eq!(field(&report, "Hollow grid cell (mm)"), "0.2");
}

#[test]
fn hollowing_requests_are_validated() {
    let setup = Setup::new();
    let output = setup.temp.path("out.goo");
    // The 2 mm thick box has no point deeper than a 2 mm wall.
    let error = failure(
        &setup.slice(&setup.cube(), "goo-v3", &output, &["--hollow", "2"]),
        1,
    );
    assert!(error.contains("nothing to hollow"), "{error}");
    let model = setup.temp.write(
        "cube.stl",
        box_stl([10.0, 20.0, 0.0], [30.0, 40.0, 20.0], false),
    );
    let error = failure(
        &setup.slice(
            &model,
            "goo-v3",
            &output,
            &["--hollow", "1", "--hollow-cell", "0.7"],
        ),
        1,
    );
    assert!(error.contains("cell too coarse"), "{error}");
    assert!(!output.exists());
    let base = [
        "slice",
        "--assume-mm",
        "m.stl",
        "--printer",
        "p.json",
        "--resin",
        "r.json",
        "--format",
        "goo-v3",
        "--output",
        "o.goo",
    ];
    let with = |extra: &[&str]| run(base.iter().chain(extra));
    assert!(failure(&with(&["--hollow-cell", "0.2"]), 2).contains("requires --hollow"));
    for bad in ["0", "-1", "nan", "inf", "x"] {
        assert!(failure(&with(&["--hollow", bad]), 2).contains("positive millimeter"));
    }
}

#[test]
fn drain_holes_open_the_cavity_and_report_every_region() {
    let setup = Setup::new();
    let model = setup.temp.write(
        "cube.stl",
        box_stl([10.0, 20.0, 0.0], [30.0, 40.0, 20.0], false),
    );
    // A 4 mm hole up through the 2 mm floor, centred on pixel boundaries (1 mm pitch).
    let output = setup.temp.path("drained.goo");
    let report = success(&setup.slice(
        &model,
        "goo-v3",
        &output,
        &[
            "--hollow",
            "2",
            "--drain-hole",
            "20",
            "30",
            "-1",
            "+z",
            "4",
            "4",
        ],
    ));
    let volume: f64 = field(&report, "Sampled mask volume (mm^3)")
        .parse()
        .unwrap();
    // 3904 minus the floor plug: about 4 mm diameter x 2 mm, i.e. 25 mm^3 sampled on 1 mm pixels.
    assert!(volume < 3904.0 - 10.0 && volume > 3904.0 - 45.0, "{report}");
    assert_eq!(field(&report, "Cavity regions"), "1");
    assert_eq!(
        field(&report, "Drain hole 1"),
        "opens to the outside, reaches cavity region 1",
        "{report}"
    );
    assert!(
        report.contains("Drainage: every cavity region is opened to the outside by a drain hole")
    );

    // A hole whose entry lies inside the floor is cut as asked (TD-089): it reaches the cavity
    // but opens nowhere, which the summary says, and the cavity stays enclosed.
    let closed = setup.temp.path("closed.goo");
    let report = success(&setup.slice(
        &model,
        "goo-v3",
        &closed,
        &[
            "--hollow",
            "2",
            "--drain-hole",
            "20",
            "30",
            "0.5",
            "+z",
            "4",
            "4",
        ],
    ));
    assert_eq!(
        field(&report, "Drain hole 1"),
        "does not open to the outside, reaches cavity region 1",
        "{report}"
    );
    assert!(report.contains("1 cavity region(s) have no drain hole opening them"));
}

#[test]
fn drain_hole_arguments_are_validated() {
    let base = [
        "slice",
        "--assume-mm",
        "m.stl",
        "--printer",
        "p.json",
        "--resin",
        "r.json",
        "--format",
        "goo-v3",
        "--output",
        "o.goo",
    ];
    let with = |extra: &[&str]| run(base.iter().chain(extra));
    // Holes no longer need hollowing (TD-089), except surface holes, whose depth reaches the
    // cavity.
    assert!(
        failure(&with(&["--surface-drain-hole", "1", "2", "3", "2"]), 2)
            .contains("requires --hollow")
    );
    assert!(failure(
        &with(&[
            "--hollow",
            "2",
            "--drain-hole",
            "1",
            "2",
            "3",
            "up",
            "4",
            "2"
        ]),
        2
    )
    .contains("axis must be one of"));
    assert!(failure(
        &with(&[
            "--hollow",
            "2",
            "--drain-hole",
            "1",
            "2",
            "3",
            "+z",
            "0",
            "2"
        ]),
        2
    )
    .contains("positive millimeter"));
    let mut many = vec!["--hollow", "2"];
    for _ in 0..1025 {
        many.extend(["--drain-hole", "1", "2", "3", "+z", "4", "2"]);
    }
    assert!(failure(&with(&many), 2).contains("at most 1024 --drain-hole"));
}

#[test]
fn drain_holes_respect_print_resolution_panel_edges_and_layer_tangency() {
    let setup = Setup::new();
    // The 20 mm cube touches the panel's X = 0 edge; 0.25 mm layers sample at 0.125 + 0.25 k.
    let model = setup.temp.write(
        "edge.stl",
        box_stl([0.0, 20.0, 0.0], [20.0, 40.0, 20.0], false),
    );
    // Horizontal hole from outside the panel, diameter 2.25 centred at Z 10: its top and bottom
    // prism edges lie exactly on the layer planes 11.125 and 8.875.
    let output = setup.temp.path("edge.goo");
    let report = success(&setup.slice(
        &model,
        "goo-v3",
        &output,
        &[
            "--hollow",
            "2",
            "--drain-hole",
            "-1",
            "30",
            "10",
            "+x",
            "4",
            "2.25",
        ],
    ));
    let volume: f64 = field(&report, "Sampled mask volume (mm^3)")
        .parse()
        .unwrap();
    assert!(volume < 3904.0 - 2.0, "{report}");
    assert!(
        report.contains("Drainage: every cavity region is opened to the outside by a drain hole")
    );
    // Below two pixel pitches (1 mm panel): refused before any geometry work.
    let refused = setup.temp.path("small.goo");
    let error = failure(
        &setup.slice(
            &model,
            "goo-v3",
            &refused,
            &[
                "--hollow",
                "2",
                "--drain-hole",
                "10",
                "30",
                "-1",
                "+z",
                "4",
                "1.5",
            ],
        ),
        1,
    );
    assert!(
        error.contains("drain hole 1: diameter 1.5 mm is below 2 mm"),
        "{error}"
    );
    // A second hole whose cylinder cannot be built is named by position.
    let error = failure(
        &setup.slice(
            &model,
            "goo-v3",
            &refused,
            &[
                "--hollow",
                "2",
                "--drain-hole",
                "10",
                "30",
                "-1",
                "+z",
                "4",
                "4",
                "--drain-hole",
                "10",
                "30",
                "25",
                "-z",
                "4",
                "30000",
            ],
        ),
        1,
    );
    assert!(
        error.contains("drain hole 2: cannot build its cylinder"),
        "{error}"
    );
    assert!(!refused.exists());
}

fn layer_plan(body: &str) -> String {
    format!(r#"{{"format":"karasu-layers","version":1,{body}}}"#)
}

#[test]
fn layer_plans_vary_heights_with_explicit_exposures() {
    let setup = Setup::new();
    let model = setup.cube();
    // Calibrated bottom and transition layers to 0.5 mm, four 0.125 mm layers at 1 s to 1 mm,
    // then 0.2 mm layers whose exposure the table interpolates (2.5 s).
    let plan = setup.temp.write(
        "layers.json",
        layer_plan(
            r#""ranges":[{"from_mm":0,"height_mm":0.25},
                {"from_mm":0.5,"height_mm":0.125,"exposure_s":1},
                {"from_mm":1,"height_mm":0.2}],
               "exposures":[{"height_mm":0.1,"exposure_s":1.5},{"height_mm":0.3,"exposure_s":3.5}]"#,
        ),
    );
    let output = setup.temp.path("reference");
    let report = success(&setup.slice(
        &model,
        "reference",
        &output,
        &["--layer-plan", plan.to_str().unwrap()],
    ));
    assert_eq!(field(&report, "Layers"), "11");
    assert_eq!(
        field(&report, "Layer heights (mm)"),
        "0.125 to 0.25, 9 layers with explicit settings"
    );
    assert_eq!(field(&report, "Sampled mask volume (mm^3)"), "400");
    let layers: Vec<(f64, f64, f64)> = fs::read_to_string(output.join("layers.jsonl"))
        .unwrap()
        .lines()
        .map(|line| {
            // The first occurrence of each key is the layer's own (settings follow).
            let number = |key: &str| -> f64 {
                let start = line.find(&format!("\"{key}\":")).unwrap() + key.len() + 3;
                let end = start + line[start..].find([',', '}']).unwrap();
                line[start..end].parse().unwrap()
            };
            (
                number("position_z_mm"),
                number("thickness_mm"),
                number("exposure_s"),
            )
        })
        .collect();
    assert_eq!(layers[1], (0.5, 0.25, 11.0));
    assert_eq!(layers[2], (0.625, 0.125, 1.0));
    assert_eq!(layers[10], (2.0, 0.2, 2.5));
    // A machine format stores the per-layer positions too.
    let goo = setup.temp.path("plan.goo");
    success(&setup.slice(
        &model,
        "goo-v3",
        &goo,
        &["--layer-plan", plan.to_str().unwrap()],
    ));
    // SL1 archives store one height; a thickness without an exposure is refused; so is a
    // malformed plan.
    let error = failure(
        &setup.slice(
            &model,
            "prusa-sl1-firmware-motion",
            &setup.temp.path("plan.sl1"),
            &["--layer-plan", plan.to_str().unwrap()],
        ),
        1,
    );
    assert!(error.contains("stores one layer height"), "{error}");
    let unexposed = setup.temp.write(
        "unexposed.json",
        layer_plan(r#""ranges":[{"from_mm":0,"height_mm":0.25},{"from_mm":1,"height_mm":0.2}]"#),
    );
    let error = failure(
        &setup.slice(
            &model,
            "reference",
            &setup.temp.path("unexposed"),
            &["--layer-plan", unexposed.to_str().unwrap()],
        ),
        1,
    );
    assert!(
        error.contains("neither its range nor the exposure table"),
        "{error}"
    );
    let malformed = setup.temp.write("bad.json", layer_plan(r#""ranges":[]"#));
    let error = failure(
        &setup.slice(
            &model,
            "reference",
            &setup.temp.path("malformed"),
            &["--layer-plan", malformed.to_str().unwrap()],
        ),
        1,
    );
    assert!(error.contains("between 1 and"), "{error}");
}

impl Setup {
    /// Slice a plate document.
    fn slice_plate(&self, plate: &Path, format: &str, output: &Path) -> Output {
        Command::new(env!("CARGO_BIN_EXE_karasu-cli"))
            .arg("slice")
            .arg("--plate")
            .arg(plate)
            .arg("--printer")
            .arg(&self.printer)
            .arg("--resin")
            .arg(&self.resin)
            .args(["--format", format])
            .arg("--output")
            .arg(output)
            .output()
            .unwrap()
    }
}

#[test]
fn copies_are_arranged_centred_and_refused_when_they_do_not_fit() {
    let setup = Setup::new();
    let model = setup.cube();
    let output = setup.temp.path("copies");
    let report = success(&setup.slice(&model, "reference", &output, &["--copies", "3"]));
    // From the middle of the plate less one pixel: 20 x 10 mm footprints 2 mm apart, the first
    // centred on 100 x 60 mm, the others below and above it (nearer the middle than beside).
    assert_eq!(field(&report, "Objects"), "1, instances 3, spacing 2 mm");
    assert_eq!(
        field(&report, "Instances"),
        "3 (arranged), footprint 20 x 10 mm"
    );
    for (copy, y) in [(1, 5), (2, -7), (3, 17)] {
        assert_eq!(
            field(&report, &format!("Instance {copy} translation (mm)")),
            format!("30 {y} 0")
        );
    }
    assert_eq!(field(&report, "Sampled mask volume (mm^3)"), "1200");
    assert_eq!(field(&report, "Overlapping footprints"), "none");
    // Twenty fit (four per shelf, five shelves); the twenty-first does not.
    let error = failure(
        &setup.slice(
            &model,
            "goo-v3",
            &setup.temp.path("full.goo"),
            &["--copies", "21"],
        ),
        1,
    );
    assert!(
        error.contains("1 of 21 arranged instances do not fit on the plate"),
        "{error}"
    );
    for extra in [
        &["--spacing", "3"][..],
        &["--copies", "0"],
        &["--copies", "2", "--center"],
        &["--copies", "2", "--spacing", "-1"],
    ] {
        failure(
            &setup.slice(&model, "goo-v3", &setup.temp.path("refused.goo"), extra),
            2,
        );
    }
}

#[test]
fn plates_unite_fixed_and_arranged_objects() {
    let setup = Setup::new();
    setup.cube();
    // Paths are relative to the plate document.
    let plate = setup.temp.write(
        "plate.json",
        r#"{"format":"karasu-plate","version":1,"objects":[
            {"model":"box.stl","units":"mm","arrange":false},
            {"name":"pair","model":"box.stl","units":"mm","drop":true,"copies":2},
            {"name":"shifted","model":"box.stl","units":"mm","translate_mm":[5,0,0],"arrange":false}]}"#,
    );
    let output = setup.temp.path("plate");
    let report = success(&setup.slice_plate(&plate, "reference", &output));
    assert_eq!(field(&report, "Objects"), "3, instances 4, spacing 2 mm");
    // With fixed objects on the plate the arranged pair goes nearest the middle, clear of them.
    let translations: Vec<&str> = report
        .lines()
        .filter_map(|l| l.strip_prefix("Instance 1 translation (mm): "))
        .collect();
    assert_eq!(translations, ["0 0 0", "30 5 0", "5 0 0"]);
    assert_eq!(field(&report, "Instance 2 translation (mm)"), "30 -7 0");
    // The overlapping fixed boxes print as their union: 25 x 10 mm, plus the pair.
    assert_eq!(
        field(&report, "Overlapping footprints"),
        r#"1 pairs, united: object 1 ("box.stl") copy 1 and object 3 ("shifted") copy 1"#
    );
    assert_eq!(
        field(&report, "Sampled mask volume (mm^3)"),
        (2.0 * (250.0 + 400.0)).to_string()
    );
    assert_eq!(
        field(&report, "Layers with merged overlapping material"),
        "8"
    );
    // Plate documents exclude model options; invalid objects are named.
    let error = failure(
        &setup.slice(
            &setup.temp.path("box.stl"),
            "goo-v3",
            &setup.temp.path("both.goo"),
            &["--plate", plate.to_str().unwrap()],
        ),
        2,
    );
    assert!(error.contains("--plate describes every object"), "{error}");
    let invalid = setup.temp.write(
        "invalid.json",
        r#"{"format":"karasu-plate","version":1,"objects":[{"model":"box.stl","copies":2,"arrange":false}]}"#,
    );
    let error = failure(
        &setup.slice_plate(&invalid, "goo-v3", &setup.temp.path("invalid.goo")),
        1,
    );
    assert!(error.contains("object 1: copies"), "{error}");
    let missing = setup.temp.write(
        "missing.json",
        r#"{"format":"karasu-plate","version":1,"objects":[{"model":"absent.stl","units":"mm"}]}"#,
    );
    let error = failure(
        &setup.slice_plate(&missing, "goo-v3", &setup.temp.path("missing.goo")),
        1,
    );
    assert!(error.contains("absent.stl"), "{error}");
}

#[test]
#[allow(clippy::too_many_lines)] // One scenario: simplification, both hole kinds, refusals.
fn cavity_simplification_and_oriented_drain_holes() {
    let setup = Setup::new();
    let model = setup.temp.write(
        "cube.stl",
        box_stl([10.0, 20.0, 0.0], [30.0, 40.0, 20.0], false),
    );
    // A 12 mm cube keeps the debug-build simplification quick.
    let small = setup.temp.write(
        "small.stl",
        box_stl([10.0, 20.0, 0.0], [22.0, 32.0, 12.0], false),
    );
    let hollow_on = |model: &PathBuf, name: &str, extra: &[&str]| {
        let mut args = vec!["--hollow", "2", "--hollow-cell", "0.5"];
        args.extend_from_slice(extra);
        success(&setup.slice(model, "goo-v3", &setup.temp.path(name), &args))
    };
    let hollow = |name: &str, extra: &[&str]| hollow_on(&small, name, extra);
    let plain = hollow("plain.goo", &[]);
    // Flat cavity walls lose most of their triangles within 0.05 mm (TD-066).
    let report = hollow("simplified.goo", &["--hollow-simplify", "0.05"]);
    let line = field(&report, "Cavity simplification");
    let counts: Vec<usize> = line
        .split("triangles ")
        .nth(1)
        .unwrap()
        .split(',')
        .next()
        .unwrap()
        .split(" -> ")
        .map(|v| v.parse().unwrap())
        .collect();
    assert!(counts[1] * 4 < counts[0], "{line}");
    assert!(line.starts_with("tolerance (mm) 0.05,"), "{line}");
    let volume = |r: &str| -> f64 { field(r, "Sampled mask volume (mm^3)").parse().unwrap() };
    assert!((volume(&report) - volume(&plain)).abs() < 1.0, "{report}");
    // Auto: half the 1 mm pixel, at most half the 0.25 mm layer.
    let report = hollow("auto.goo", &["--hollow-simplify", "auto"]);
    assert!(
        field(&report, "Cavity simplification").starts_with("tolerance (mm) 0.125,"),
        "{report}"
    );
    // A hole perpendicular to the -X face, through the 2 mm wall, 1 mm out and 1 mm past it.
    let report = hollow_on(
        &model,
        "surface.goo",
        &["--surface-drain-hole", "9.5", "30", "10", "4"],
    );
    let axis = field(&report, "Drain hole 1 axis");
    assert!(
        axis.starts_with("entry (mm) 9 30 10, direction 1 0 0, length (mm) "),
        "{axis}"
    );
    let length: f64 = axis.rsplit(' ').next().unwrap().parse().unwrap();
    // 1 mm out, through the wall to the cavity (the guarantee's offset, TD-093), 1 mm past it.
    let offset: f64 = field(&report, "Hollow offset (mm)").parse().unwrap();
    assert!((length - (2.0 + offset)).abs() < 1e-9, "{axis}");
    assert!(
        report.contains("Drainage: every cavity region is opened to the outside by a drain hole")
    );
    // A hole at 45 degrees through the same wall.
    let report = hollow_on(
        &model,
        "oblique.goo",
        &["--drain-hole", "6", "30", "6", "1,0,1", "12", "4"],
    );
    assert!(
        field(&report, "Drain hole 1 axis")
            .starts_with("entry (mm) 6 30 6, direction 0.7071067811865475 0 0.7071067811865475"),
        "{report}"
    );
    assert!(
        report.contains("Drainage: every cavity region is opened to the outside by a drain hole")
    );
    let base = [
        "slice",
        "--assume-mm",
        "m.stl",
        "--printer",
        "p.json",
        "--resin",
        "r.json",
        "--format",
        "goo-v3",
        "--output",
        "o.goo",
    ];
    let with = |extra: &[&str]| run(base.iter().chain(extra));
    assert!(failure(&with(&["--hollow-simplify", "0.05"]), 2).contains("requires --hollow"));
    for bad in ["0", "x", "nan"] {
        assert!(
            failure(&with(&["--hollow", "2", "--hollow-simplify", bad]), 2).contains("tolerance"),
            "{bad}"
        );
    }
    assert!(failure(
        &with(&[
            "--hollow",
            "2",
            "--drain-hole",
            "1",
            "2",
            "3",
            "0,0,0",
            "4",
            "2"
        ]),
        2
    )
    .contains("not all zero"));
    assert!(
        failure(&with(&["--surface-drain-hole", "1", "2", "3", "2"]), 2)
            .contains("requires --hollow")
    );
}

#[test]
fn holes_need_no_hollowing_may_overlap_and_may_pass_through() {
    // TD-089: a solid 20 mm block with vertical holes through its whole height.
    let setup = Setup::new();
    let model = setup.temp.write(
        "block.stl",
        box_stl([10.0, 20.0, 0.0], [30.0, 40.0, 20.0], false),
    );
    let volume = |name: &str, holes: &[&str]| -> (f64, String) {
        let report = success(&setup.slice(&model, "goo-v3", &setup.temp.path(name), holes));
        let volume = field(&report, "Sampled mask volume (mm^3)")
            .parse()
            .unwrap();
        (volume, report)
    };
    let hole = |x: &'static str| ["--drain-hole", x, "30", "21", "-z", "22", "4"];
    let (plain, _) = volume("plain.goo", &[]);
    let (one, report) = volume("one.goo", &hole("20"));
    assert_eq!(
        field(&report, "Drain hole 1"),
        "opens to the outside, passes through the object"
    );
    // Two holes 1 mm apart overlap: their union is removed. Under even-odd their shared lens
    // would be filled again and less than one hole would be removed.
    let (two, _) = volume("two.goo", &[hole("20"), hole("21")].concat());
    let single = plain - one;
    let both = plain - two;
    assert!(single > 0.0);
    assert!(both > single && both < 2.0 * single, "{single} {both}");
}

#[test]
fn surface_holes_take_a_depth_a_starting_surface_and_a_tilt() {
    // TD-089 (M6.4c c5): the command line shapes surface holes as the interface does.
    let setup = Setup::new();
    let model = setup.temp.write(
        "block.stl",
        box_stl([10.0, 20.0, 0.0], [30.0, 40.0, 20.0], false),
    );
    let report = |name: &str, extra: &[&str]| {
        success(&setup.slice(&model, "goo-v3", &setup.temp.path(name), extra))
    };
    let axis = |report: &str| -> Vec<f64> {
        let axis = field(report, "Drain hole 1 axis");
        axis.split([',', ' '])
            .filter_map(|v| v.parse().ok())
            .collect()
    };
    // A fixed depth needs no hollowing: from the top face, 1 mm out and 5 mm in.
    let top = report(
        "depth.goo",
        &[
            "--surface-drain-hole",
            "20",
            "30",
            "20",
            "4",
            "--hole-depth",
            "5",
        ],
    );
    assert_eq!(axis(&top), [20.0, 30.0, 21.0, 0.0, 0.0, -1.0, 6.0], "{top}");
    assert!(
        field(&top, "Drain hole 1").starts_with("opens to the outside"),
        "{top}"
    );
    // Tilted 30 degrees towards +X, the top face's reference tangent.
    let tilted = report(
        "tilt.goo",
        &[
            "--surface-drain-hole",
            "20",
            "30",
            "20",
            "4",
            "--hole-depth",
            "5",
            "--hole-tilt",
            "30",
            "0",
        ],
    );
    let direction = &axis(&tilted)[3..6];
    let expected = [0.5, 0.0, -(3.0_f64.sqrt()) / 2.0];
    for k in 0..3 {
        assert!((direction[k] - expected[k]).abs() < 1e-9, "{tilted}");
    }
    // From the cavity's floor of the hollowed block, out through the bottom wall.
    let cavity = report(
        "cavity.goo",
        &[
            "--hollow",
            "2",
            "--surface-drain-hole",
            "20",
            "30",
            "2.5",
            "3",
            "--hole-from",
            "cavity",
            "--hole-depth",
            "4",
        ],
    );
    assert!(axis(&cavity)[5] < -0.99, "{cavity}");
    let finding = field(&cavity, "Drain hole 1");
    assert!(finding.contains("opens to the outside"), "{cavity}");
    assert!(
        cavity.contains("Drainage: every cavity region is opened to the outside by a drain hole"),
        "{cavity}"
    );
}

#[test]
fn surface_hole_options_are_refused_when_misused() {
    // Refused before any work.
    let base = [
        "slice",
        "--assume-mm",
        "m.stl",
        "--printer",
        "p.json",
        "--resin",
        "r.json",
        "--format",
        "goo-v3",
        "--output",
        "o.goo",
    ];
    let with = |extra: &[&str]| run(base.iter().chain(extra));
    let hole = ["--surface-drain-hole", "1", "2", "3", "2"];
    for (extra, expected) in [
        (vec!["--hole-depth", "2"], "just before it"),
        (
            vec![
                "--drain-hole",
                "1",
                "2",
                "3",
                "+z",
                "4",
                "2",
                "--hole-depth",
                "2",
            ],
            "just before it",
        ),
        (
            [&hole[..], &["--hole-depth", "2", "--hole-depth", "3"]].concat(),
            "only once per",
        ),
        ([&hole[..], &["--hole-depth", "0"]].concat(), "positive"),
        (
            [&hole[..], &["--hole-from", "inside"]].concat(),
            "outer or cavity",
        ),
        (
            [&hole[..], &["--hole-depth", "2", "--hole-from", "cavity"]].concat(),
            "--hole-from cavity requires --hollow",
        ),
        (
            [&hole[..], &["--hole-depth", "2", "--hole-tilt", "75", "0"]].concat(),
            "from 0 to 60",
        ),
        ([&hole[..], &["--hole-tilt", "10"]].concat(), "from 0 to 60"),
        (hole.to_vec(), "or --hole-depth"),
    ] {
        let message = failure(&with(&extra), 2);
        assert!(message.contains(expected), "{extra:?}: {message}");
    }
}

/// One ASCII STL of several boxes.
fn boxes_stl(boxes: &[([f64; 3], [f64; 3])]) -> String {
    let mut text = String::from("solid boxes\n");
    for &(min, max) in boxes {
        let one = box_stl(min, max, false);
        for line in one
            .lines()
            .filter(|l| !l.starts_with("solid") && !l.starts_with("endsolid"))
        {
            text.push_str(line);
            text.push('\n');
        }
    }
    text.push_str("endsolid boxes\n");
    text
}

#[test]
fn unsupported_reports_islands_and_overhangs_on_the_sliced_layers() {
    // TD-107, on 1 mm pixels and 0.25 mm layers (reach under a pixel at 45 degrees).
    let setup = Setup::new();
    let pillar = ([10.0, 10.0, 0.0], [14.0, 14.0, 10.0]);
    let floating = ([20.0, 10.0, 5.0], [24.0, 14.0, 10.0]);
    let island = setup
        .temp
        .write("island.stl", boxes_stl(&[pillar, floating]));
    let report = success(&setup.slice(
        &island,
        "goo-v3",
        &setup.temp.path("island.goo"),
        &["--unsupported"],
    ));
    assert_eq!(
        field(&report, "Unsupported regions (critical angle 45 degrees)"),
        "1 island(s), 16.000 mm^2; 0 overhang(s), 0.000 mm^2",
        "{report}"
    );
    assert!(
        report.contains("Unsupported island: layers 20-20 (x 22.000, y 12.000, z 5.125 mm)"),
        "{report}"
    );
    // A bridge from the pillar under the floating box makes it one piece that rests: what
    // hangs out is an overhang, no longer an island.
    let bridge = ([14.0, 10.0, 5.0], [20.0, 14.0, 6.0]);
    let bridged = setup
        .temp
        .write("bridged.stl", boxes_stl(&[pillar, bridge, floating]));
    let report = success(&setup.slice(
        &bridged,
        "goo-v3",
        &setup.temp.path("bridged.goo"),
        &["--unsupported", "--overhang-angle", "30"],
    ));
    let line = field(&report, "Unsupported regions (critical angle 30 degrees)");
    assert!(line.starts_with("0 island(s)"), "{report}");
    assert!(!line.contains("; 0 overhang(s)"), "{report}");
    // Misused options are refused.
    let without = setup.slice(
        &island,
        "goo-v3",
        &setup.temp.path("without.goo"),
        &["--overhang-angle", "30"],
    );
    assert!(failure(&without, 2).contains("--overhang-angle is used only with --unsupported"));
    let flat = setup.slice(
        &island,
        "goo-v3",
        &setup.temp.path("flat.goo"),
        &["--unsupported", "--overhang-angle", "90"],
    );
    assert!(failure(&flat, 2).contains("between 0 and 90"));
}

#[test]
fn suction_cups_report_the_resin_a_ring_on_the_plate_encloses() {
    // TD-114, on 1 mm pixels and 0.25 mm layers: a ring 2 mm tall around 4 × 4 mm, capped by
    // the plate, is a cup until its rim.
    let setup = Setup::new();
    let walls = |top: &[([f64; 3], [f64; 3])]| {
        let mut boxes = vec![
            ([10.0, 10.0, 0.0], [12.0, 18.0, 2.0]),
            ([16.0, 10.0, 0.0], [18.0, 18.0, 2.0]),
            ([12.0, 10.0, 0.0], [16.0, 12.0, 2.0]),
        ];
        boxes.extend_from_slice(top);
        boxes_stl(&boxes)
    };
    let ring = setup
        .temp
        .write("ring.stl", walls(&[([12.0, 16.0, 0.0], [16.0, 18.0, 2.0])]));
    let report = success(&setup.slice(
        &ring,
        "goo-v3",
        &setup.temp.path("ring.goo"),
        &["--suction-cups"],
    ));
    assert_eq!(
        field(&report, "Suction cups (vents of at least 1 mm^2)"),
        "1, the largest 16.000 mm^2",
        "{report}"
    );
    assert_eq!(
        field(&report, "Suction cup"),
        "layers 0-7 (cap at x 14.000, y 14.000, z 0.000 mm), 16.000 mm^2, 32.000 mm^3, no \
         opening, then open, 1 cap(s)",
        "{report}"
    );
    // A slot 1 mm wide through the wall vents it, unless the vent must be wider.
    let slotted = setup.temp.write(
        "slotted.stl",
        walls(&[
            ([12.0, 16.0, 0.0], [13.0, 18.0, 2.0]),
            ([14.0, 16.0, 0.0], [16.0, 18.0, 2.0]),
        ]),
    );
    let report = success(&setup.slice(
        &slotted,
        "goo-v3",
        &setup.temp.path("slotted.goo"),
        &["--suction-cups", "--min-vent", "0"],
    ));
    assert_eq!(
        field(&report, "Suction cups (vents of at least 0 mm^2)"),
        "0, the largest 0.000 mm^2",
        "{report}"
    );
    let report = success(&setup.slice(
        &slotted,
        "goo-v3",
        &setup.temp.path("narrow.goo"),
        &["--suction-cups", "--min-vent", "20"],
    ));
    assert!(
        field(&report, "Suction cup").ends_with("openings too small, then open, 1 cap(s)"),
        "{report}"
    );
    // Misused options are refused.
    let without = setup.slice(
        &ring,
        "goo-v3",
        &setup.temp.path("without.goo"),
        &["--min-vent", "2"],
    );
    assert!(failure(&without, 2).contains("--min-vent is used only with --suction-cups"));
    let negative = setup.slice(
        &ring,
        "goo-v3",
        &setup.temp.path("negative.goo"),
        &["--suction-cups", "--min-vent", "-1"],
    );
    assert!(failure(&negative, 2).contains("zero or more"));
}
