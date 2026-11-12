//! Dependency-free M2.4-C measurement worker. See tools/mesh-profile/README.md.

use karasu_core::geometry::{ObjectTransform, Point3, Rotation3, Vector3};
use karasu_core::mesh::stl::{
    import_stl_bytes, FacetMetadata, ImportedStl, StlOptions, StlUnitPolicy,
};
use karasu_core::mesh::topology::{CheckStatus, TopologyReport};
use karasu_core::mesh::Aabb;
use std::{error::Error, hint::black_box, path::PathBuf, time::Instant};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

struct Config {
    mode: String,
    operation: String,
    path: PathBuf,
    vertices: usize,
    faces: usize,
    edges: usize,
    bytes: usize,
    batch: usize,
    expected: [f64; 6],
}

fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

impl Config {
    fn parse(args: &[String]) -> Result<Self> {
        require(
            args.len() == 14,
            "expected mode operation path V F E bytes batch minXYZ maxXYZ",
        )?;
        let mut expected = [0.0; 6];
        for (value, text) in expected.iter_mut().zip(&args[8..]) {
            *value = text.parse()?;
        }
        let config = Self {
            mode: args[0].clone(),
            operation: args[1].clone(),
            path: args[2].clone().into(),
            vertices: args[3].parse()?,
            faces: args[4].parse()?,
            edges: args[5].parse()?,
            bytes: args[6].parse()?,
            batch: args[7].parse()?,
            expected,
        };
        require(
            matches!(config.mode.as_str(), "timing" | "memory"),
            "unknown mode",
        )?;
        require(
            (1..=8192).contains(&config.batch),
            "batch must be in 1..=8192",
        )?;
        require(
            config.expected.iter().all(|v| v.is_finite()),
            "expected bounds must be finite",
        )?;
        Ok(config)
    }

    fn validate_import(&self, imported: &ImportedStl) -> Result<()> {
        require(
            imported.mesh.vertices().len() == self.vertices,
            "vertex count mismatch",
        )?;
        require(
            imported.mesh.triangles().len() == self.faces,
            "triangle count mismatch",
        )?;
        require(
            imported.metadata.facets.len() == self.faces,
            "facet metadata count mismatch",
        )?;
        require(
            imported.diagnostics.is_empty(),
            "unexpected import diagnostic",
        )?;
        // Bounds arguments describe the transformed result only for bounds workloads.
        if !self.operation.starts_with("bounds-") {
            self.validate_bounds(imported.mesh.source_bounds())?;
        }
        Ok(())
    }

    fn validate_bounds(&self, bounds: Option<Aabb>) -> Result<()> {
        let bounds = bounds.ok_or("unexpected empty bounds")?;
        let actual = [bounds.min().coordinates(), bounds.max().coordinates()].concat();
        // Fixture/transformed coordinates are below 100 mm. This covers the independent
        // scalar versus quaternion evaluation chains, not a production global epsilon.
        let tolerance = 256.0 * f64::EPSILON * 100.0;
        require(
            actual
                .iter()
                .zip(self.expected)
                .all(|(a, e)| (a - e).abs() <= tolerance),
            "independent extrema mismatch",
        )?;
        let size = bounds.dimensions();
        require(
            [size.x, size.y, size.z]
                .iter()
                .enumerate()
                .all(|(axis, a)| {
                    (a - (self.expected[axis + 3] - self.expected[axis])).abs() <= 2.0 * tolerance
                }),
            "independent dimensions mismatch",
        )
    }

    fn validate_topology(&self, report: &TopologyReport) -> Result<()> {
        let s = report.summary();
        require(
            [
                s.source_vertices,
                s.source_triangles,
                s.usable_triangles,
                s.edges,
                s.two_face_edges,
                s.interior_vertices,
                s.edge_components,
                s.vertex_components,
            ] == [
                self.vertices,
                self.faces,
                self.faces,
                self.edges,
                self.edges,
                self.vertices,
                1,
                1,
            ],
            "topology count mismatch",
        )?;
        require(
            [
                s.excluded_triangles,
                s.duplicate_triangles,
                s.boundary_edges,
                s.non_manifold_edges,
                s.winding_conflicts,
                s.boundary_vertices,
                s.non_manifold_vertices,
                s.indeterminate_vertices,
                s.isolated_vertices,
            ] == [0; 9],
            "unexpected topology defect",
        )?;
        require(
            s.closed_combinatorial_surface
                && s.no_boundary_edges
                && report.issues().is_empty()
                && [s.edge_manifold, s.vertex_manifold, s.local_winding]
                    == [CheckStatus::Satisfied; 3],
            "topology classification mismatch",
        )
    }
}

fn transform(operation: &str) -> Result<ObjectTransform> {
    let zero = Vector3::new(0.0, 0.0, 0.0);
    let offset = Vector3::new(15.0, -7.0, 5.0);
    let scale = Vector3::new(1.25, 0.8, 1.1);
    let angles = Vector3::new(0.31, -0.47, 0.29);
    let (translation, rotation, scale, pivot) = match operation {
        "bounds-identity" => (zero, zero, Vector3::new(1.0, 1.0, 1.0), zero),
        "bounds-translation" => (offset, zero, Vector3::new(1.0, 1.0, 1.0), zero),
        "bounds-scale" => (zero, zero, scale, zero),
        "bounds-rotation" => (zero, angles, Vector3::new(1.0, 1.0, 1.0), zero),
        "bounds-combined" => (offset, angles, scale, Vector3::new(2.0, -3.0, 4.0)),
        _ => return Err("unknown bounds operation".into()),
    };
    Ok(ObjectTransform::new(
        translation,
        Rotation3::from_euler_xyz_radians(rotation)?,
        scale,
        Point3::new(pivot.x, pivot.y, pivot.z),
    )?)
}

fn measure<T>(
    config: &Config,
    mut operation: impl FnMut() -> Result<T>,
    validate: impl Fn(&T) -> Result<()>,
) -> Result<()> {
    if config.mode == "memory" {
        let result = black_box(operation()?);
        validate(black_box(&result))?;
        println!("validation=PASS");
        return Ok(());
    }
    // Preallocated result retention excludes result destruction and validation from timing.
    // The batch loop includes black_box, Result branching, and one Vec push per operation.
    let mut samples = Vec::with_capacity(10);
    for iteration in 0..13 {
        let mut results = Vec::with_capacity(config.batch);
        let start = Instant::now();
        for _ in 0..config.batch {
            results.push(black_box(operation()?));
        }
        let elapsed = start.elapsed().as_nanos();
        for result in &results {
            validate(black_box(result))?;
        }
        if iteration >= 3 {
            samples.push(elapsed);
        }
    }
    println!("sample,total_ns,batch");
    for (index, elapsed) in samples.iter().enumerate() {
        println!("{index},{elapsed},{}", config.batch);
    }
    eprintln!(
        "validation=PASS warmup_batches=3 timed_batches=10 batch={}",
        config.batch
    );
    Ok(())
}

fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["baseline"] {
        println!(
            "baseline=PASS point_bytes={} triangle_bytes={} facet_bytes={}",
            size_of::<Point3>(),
            size_of::<[u32; 3]>(),
            size_of::<FacetMetadata>()
        );
        return Ok(());
    }
    let config = Config::parse(&args)?;
    if config.operation == "read" {
        return measure(
            &config,
            || Ok(std::fs::read(black_box(&config.path))?),
            |bytes| require(bytes.len() == config.bytes, "read length mismatch"),
        );
    }
    let bytes = std::fs::read(&config.path)?;
    require(bytes.len() == config.bytes, "input length mismatch")?;
    let options = StlOptions::new(StlUnitPolicy::AssumeMillimeters);
    if config.operation == "import" {
        return measure(
            &config,
            || Ok(import_stl_bytes(black_box(&bytes), black_box(options))?),
            |imported| config.validate_import(imported),
        );
    }
    let imported = import_stl_bytes(&bytes, options)?;
    config.validate_import(&imported)?;
    drop(bytes);
    match config.operation.as_str() {
        "topology" => measure(
            &config,
            || Ok(black_box(&imported.mesh).analyze_topology()?),
            |report| config.validate_topology(report),
        ),
        operation if operation.starts_with("bounds-") => {
            let transform = transform(operation)?;
            measure(
                &config,
                || Ok(black_box(&imported.mesh).world_bounds(black_box(&transform))?),
                |bounds| config.validate_bounds(*bounds),
            )
        }
        _ => Err("unknown operation".into()),
    }
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("mesh-profile: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
