//! Analytic M4 benchmark and independently decodable PNG export.
//!
//! Usage: `cargo run --release -p karasu-core --example raster_reference -- WIDTH HEIGHT SAMPLES OUTPUT.png [EDGES]`
//! Physical panel is [0,100] x [0,60] mm; default geometry is the triangle
//! [(5,5),(95,10),(20,55)] with rectangular hole [(20,15),(35,15),(35,25),(20,25)].
//! Optional EDGES replaces it with an ellipse centered at (50,30), radii (45,25).
use karasu_core::{
    cancellation::Cancellation,
    geometry::Point3,
    mesh::TriangleMesh,
    raster::{
        png::write_png, rasterize_layer, Compensation, LcdProfile, Orientation, OutsidePolicy,
        RasterConfig, RasterLimits,
    },
    slicing::{
        engine::{LayerConfig, LayerEngine, LayerStageLimits},
        grid::BinaryGridSpec,
        regions::RegionFillRule,
        schedule::{build_layer_schedule, ScheduleEnd, ScheduleLimits, UniformScheduleSpec},
        SectionTransform,
    },
};
use std::{error::Error, io, time::Instant};
fn main() -> Result<(), Box<dyn Error>> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if !(4..=5).contains(&arguments.len()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: raster_reference WIDTH HEIGHT SAMPLES OUTPUT.png [EDGES]",
        )
        .into());
    }
    let width = arguments[0].parse()?;
    let height = arguments[1].parse()?;
    let samples = arguments[2].parse()?;
    let rings = rings(arguments.get(4))?;
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    for ring in &rings {
        let base = u32::try_from(vertices.len())?;
        for &[x, y] in ring {
            vertices.extend([Point3::new(x, y, -1.), Point3::new(x, y, 1.)]);
        }
        for i in 0..ring.len() {
            let a = base + 2 * u32::try_from(i)?;
            let b = base + 2 * u32::try_from((i + 1) % ring.len())?;
            triangles.extend([[a, b, b + 1], [a, b + 1, a + 1]]);
        }
    }
    let mesh = TriangleMesh::try_new(vertices, triangles)?;
    let plan = build_layer_schedule(
        UniformScheduleSpec::try_new(0., 0.05, ScheduleEnd::Count(1))?,
        ScheduleLimits {
            max_layers: 1,
            max_payload_bytes: 1024,
        },
    )?;
    let mut config = LayerConfig {
        grid: BinaryGridSpec::try_new([0., 0.], -20)?,
        fill_rule: RegionFillRule::EvenOddStrict,
        stages: LayerStageLimits::default(),
        max_output_payload_bytes: 256 * 1024 * 1024,
    };
    // A 1000-edge ribbon produces 2000 section edges (1,999,000 validity pairs).
    // This explicit profiling-only budget leaves production defaults unchanged.
    config.stages.validity.max_pairs = 4_000_000;
    let start = Instant::now();
    let layer = LayerEngine::new(
        &mesh,
        &plan,
        SectionTransform::Identity,
        config,
        Cancellation::never(),
        None,
    )
    .next()
    .ok_or_else(|| io::Error::other("missing scheduled layer"))??;
    let section_ms = start.elapsed().as_secs_f64() * 1000.;
    let profile = LcdProfile::try_new(
        [width, height],
        [0., 0.],
        [100., 60.],
        Orientation::default(),
    )?;
    let start = Instant::now();
    let image = rasterize_layer(
        &layer,
        profile,
        RasterConfig {
            samples_per_axis: samples,
            grayscale_levels: 256,
            minimum_gray: 0,
            outside: OutsidePolicy::Reject,
            compensation: Compensation::None,
            kept: karasu_core::raster::KeptSampling::Antialiased,
        },
        RasterLimits::default(),
    )?;
    let raster_ms = start.elapsed().as_secs_f64() * 1000.;
    let mut file = std::io::BufWriter::new(std::fs::File::create(&arguments[3])?);
    let start = Instant::now();
    write_png(&mut file, &image)?;
    std::io::Write::flush(&mut file)?;
    println!(
        "width={width} height={height} samples={samples} source_edges={} section_ms={section_ms:.3} raster_ms={raster_ms:.3} png_ms={:.3} payload_bytes={} nonzero={} intensity_sum={} bounds={:?}",
        rings.iter().map(Vec::len).sum::<usize>(),
        start.elapsed().as_secs_f64() * 1000.,
        image.requested_payload_bytes(),
        image.stats().nonzero_pixels,
        image.stats().intensity_sum,
        image.stats().bounds
    );
    Ok(())
}

fn rings(edge_count: Option<&String>) -> Result<Vec<Vec<[f64; 2]>>, Box<dyn Error>> {
    let rings = if let Some(argument) = edge_count {
        let count: u32 = argument.parse()?;
        if !(3..=1000).contains(&count) {
            return Err(
                io::Error::new(io::ErrorKind::InvalidInput, "EDGES must be 3..1000").into(),
            );
        }
        vec![(0..count)
            .map(|i| {
                let angle = f64::from(i) * std::f64::consts::TAU / f64::from(count);
                [
                    ((50. + 45. * angle.cos()) * 4096.).round() / 4096.,
                    ((30. + 25. * angle.sin()) * 4096.).round() / 4096.,
                ]
            })
            .collect::<Vec<_>>()]
    } else {
        vec![
            vec![[5., 5.], [95., 10.], [20., 55.]],
            vec![[20., 15.], [35., 15.], [35., 25.], [20., 25.]],
        ]
    };
    Ok(rings)
}
