//! Helpers shared by the integration tests.
#![allow(dead_code)] // Each test binary uses a different subset.

use karasu_core::profiles::{
    ImageSettings, LayerSettings, MotionStage, PrintSettingProfile, PrintSettings,
    PrinterCalibration, PrinterLimits, PrinterProfile, ResinMaterial, ResinProfile, SettingStatus,
};
use std::{
    fmt::Write as _,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);
pub const PRINTER_ID: &str = "test-printer";

/// A fresh directory under the system temporary directory, removed on drop.
pub struct TempDirectory(pub PathBuf);

impl TempDirectory {
    pub fn new() -> Self {
        let index = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "karasu-project-pipeline-{}-{index}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    pub fn write(&self, name: &str, contents: impl AsRef<[u8]>) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, contents).unwrap();
        path
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// An ASCII STL box with outward faces.
pub fn box_stl(min: [f64; 3], max: [f64; 3]) -> String {
    let corner = |i: usize| -> [f64; 3] {
        std::array::from_fn(|axis| {
            if i >> axis & 1 == 1 {
                max[axis]
            } else {
                min[axis]
            }
        })
    };
    // Each quad is listed counter-clockwise seen from outside.
    let quads = [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ];
    let mut text = String::from("solid box\n");
    for quad in quads {
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

pub fn layer(exposure_s: f64) -> LayerSettings {
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

/// A 100 x 60 x 50 mm printer with 1 mm pixels.
pub fn printer() -> PrinterProfile {
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
        format_ids: vec!["goo-v3".into()],
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

/// A resin calibrated for [`printer`] at 0.25 mm layers.
pub fn resin() -> ResinProfile {
    ResinProfile {
        schema_version: 1,
        id: "test-resin".into(),
        name: "Test resin".into(),
        revision: 1,
        density_g_per_ml: None,
        cost_per_liter: None,
        currency: None,
        calibrations: vec![PrinterCalibration {
            printer_id: PRINTER_ID.into(),
            settings: PrintSettings {
                heating_temperature_c: None,
                layer_height_mm: 0.25,
                bottom_layer_count: 1,
                transition_layer_count: 1,
                bottom: layer(20.),
                normal: layer(2.),
            },
        }],
    }
}

pub fn field<'a>(summary: &'a str, key: &str) -> &'a str {
    summary
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key}: ")))
        .unwrap_or_else(|| panic!("no {key:?} in:\n{summary}"))
}

/// A binary STL box with outward faces, each face split into `n` x `n` squares: a dense model
/// (12 n^2 triangles) whose import takes measurable time.
#[allow(clippy::many_single_char_names)] // Grid indices and quad corners.
pub fn dense_box_stl(min: [f64; 3], max: [f64; 3], n: u32) -> Vec<u8> {
    let triangles = 12 * n * n;
    let mut bytes = vec![0; 80];
    bytes.extend_from_slice(&triangles.to_le_bytes());
    let lerp =
        |axis: usize, k: u32| min[axis] + (max[axis] - min[axis]) * f64::from(k) / f64::from(n);
    // Each face: the fixed axis and side, then the two in-plane axes in outward (CCW) order.
    for (fixed, high, u, v) in [
        (2, false, 1, 0),
        (2, true, 0, 1),
        (1, false, 0, 2),
        (1, true, 2, 0),
        (0, false, 2, 1),
        (0, true, 1, 2),
    ] {
        let point = |i: u32, j: u32| {
            let mut p = [0.0f64; 3];
            p[fixed] = if high { max[fixed] } else { min[fixed] };
            p[u] = lerp(u, i);
            p[v] = lerp(v, j);
            p
        };
        for i in 0..n {
            for j in 0..n {
                let [a, b, c, d] = [
                    point(i, j),
                    point(i + 1, j),
                    point(i + 1, j + 1),
                    point(i, j + 1),
                ];
                for triangle in [[a, b, c], [a, c, d]] {
                    bytes.extend_from_slice(&[0; 12]);
                    for vertex in triangle {
                        for coordinate in vertex {
                            #[allow(clippy::cast_possible_truncation)] // Exact for these grids.
                            bytes.extend_from_slice(&(coordinate as f32).to_le_bytes());
                        }
                    }
                    bytes.extend_from_slice(&[0; 2]);
                }
            }
        }
    }
    bytes
}

/// [`resin`] as a library resin (TD-106).
pub fn material() -> ResinMaterial {
    let resin = resin();
    ResinMaterial {
        colour: None,
        swatch: None,
        schema_version: 2,
        id: resin.id,
        name: resin.name,
        revision: 1,
        vendor: None,
        kind: None,
        density_g_per_ml: None,
        cost_per_liter: None,
        currency: None,
        based_on: None,
    }
}

/// [`resin`]'s calibration as a print setting for [`printer`] and [`material`].
pub fn print_setting() -> PrintSettingProfile {
    PrintSettingProfile {
        schema_version: 2,
        id: "test-setting".into(),
        name: "Test 0.25 mm".into(),
        revision: 1,
        printer_id: PRINTER_ID.into(),
        resin_id: "test-resin".into(),
        film: None,
        status: SettingStatus::User,
        source: None,
        based_on: None,
        settings: resin().calibrations[0].settings.clone(),
        image: ImageSettings::default(),
    }
}
