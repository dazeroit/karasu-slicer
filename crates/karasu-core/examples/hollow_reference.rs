//! Reproducible owned analytic hollowing fixtures; no print calibration is supplied.
use karasu_core::{
    cancellation::Cancellation,
    geometry::{ObjectTransform, Point3},
    mesh::TriangleMesh,
    preparation::{
        hollow::{hollow_mesh, HollowLimits},
        HollowSpec, Operation, OperationKind, PlanLimits, PreparationPlan, SamplingSpec,
        SourceBinding,
    },
};
use std::{
    error::Error,
    fs::{self, File},
    io::{BufWriter, Write},
    path::Path,
    time::Instant,
};
fn cube() -> Result<TriangleMesh, Box<dyn Error>> {
    let vertices = vec![
        Point3::new(0., 0., 0.),
        Point3::new(20., 0., 0.),
        Point3::new(20., 20., 0.),
        Point3::new(0., 20., 0.),
        Point3::new(0., 0., 20.),
        Point3::new(20., 0., 20.),
        Point3::new(20., 20., 20.),
        Point3::new(0., 20., 20.),
    ];
    let mut faces = Vec::new();
    for [a, b, c, d] in [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
    ] {
        faces.push([a, b, c]);
        faces.push([a, c, d]);
    }
    Ok(TriangleMesh::try_new(vertices, faces)?)
}
fn determinant(origin: Point3, first: Point3, second: Point3, query: Point3) -> f64 {
    let edge = [first.x - origin.x, first.y - origin.y, first.z - origin.z];
    let other = [
        second.x - origin.x,
        second.y - origin.y,
        second.z - origin.z,
    ];
    let delta = [query.x - origin.x, query.y - origin.y, query.z - origin.z];
    (edge[1] * other[2] - edge[2] * other[1]) * delta[0]
        + (edge[2] * other[0] - edge[0] * other[2]) * delta[1]
        + (edge[0] * other[1] - edge[1] * other[0]) * delta[2]
}

fn sphere(subdivisions: u32) -> Result<TriangleMesh, Box<dyn Error>> {
    let phi = 1_f64.midpoint(5_f64.sqrt());
    let scale = 10. / (1. + phi * phi).sqrt();
    let mut vertices = Vec::new();
    for a in [-1., 1.] {
        for b in [-phi, phi] {
            for p in [[0., a, b], [a, b, 0.], [b, 0., a]] {
                vertices.push(Point3::new(p[0] * scale, p[1] * scale, p[2] * scale + 10.));
            }
        }
    }
    let mut faces = Vec::new();
    for a in 0..vertices.len() {
        for b in a + 1..vertices.len() {
            for c in b + 1..vertices.len() {
                let signs: Vec<_> = vertices
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != a && *i != b && *i != c)
                    .map(|(_, p)| determinant(vertices[a], vertices[b], vertices[c], *p))
                    .collect();
                if signs.iter().all(|v| *v < 0.) {
                    faces.push([u32::try_from(a)?, u32::try_from(b)?, u32::try_from(c)?]);
                } else if signs.iter().all(|v| *v > 0.) {
                    faces.push([u32::try_from(a)?, u32::try_from(c)?, u32::try_from(b)?]);
                }
            }
        }
    }
    if faces.len() != 20 {
        return Err("fixture hull must have 20 faces".into());
    }
    for _ in 0..subdivisions {
        let mut edges = std::collections::BTreeMap::new();
        let mut refined = Vec::new();
        for [a, b, c] in faces {
            let mut middle = [0; 3];
            for (i, (left, right)) in [(a, b), (b, c), (c, a)].into_iter().enumerate() {
                let key = (left.min(right), left.max(right));
                middle[i] = if let Some(&id) = edges.get(&key) {
                    id
                } else {
                    let left = vertices[left as usize];
                    let right = vertices[right as usize];
                    let mut p = [
                        left.x.midpoint(right.x),
                        left.y.midpoint(right.y),
                        left.z.midpoint(right.z) - 10.,
                    ];
                    let length = p.iter().map(|v| v * v).sum::<f64>().sqrt();
                    for v in &mut p {
                        *v *= 10. / length;
                    }
                    let id = u32::try_from(vertices.len())?;
                    vertices.push(Point3::new(p[0], p[1], p[2] + 10.));
                    edges.insert(key, id);
                    id
                };
            }
            let [ab, bc, ca] = middle;
            refined.extend([[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]]);
        }
        faces = refined;
    }
    Ok(TriangleMesh::try_new(vertices, faces)?)
}
fn write_stl(mesh: &TriangleMesh, path: &Path) -> Result<(), Box<dyn Error>> {
    let mut out = BufWriter::new(File::create_new(path)?);
    writeln!(out, "solid karasu_reference")?;
    for face in mesh.triangles() {
        writeln!(out, "facet normal 0 0 0\nouter loop")?;
        for &id in face {
            let p = mesh.vertices()[id as usize];
            writeln!(out, "vertex {} {} {}", p.x, p.y, p.z)?;
        }
        writeln!(out, "endloop\nendfacet")?;
    }
    writeln!(out, "endsolid karasu_reference")?;
    out.flush()?;
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || args.len() > 3 || args.get(1).is_some_and(|v| v != "--sphere") {
        return Err(
            "usage: hollow_reference NEW_OUTPUT_DIRECTORY [--sphere [SUBDIVISIONS_0_TO_2]]".into(),
        );
    }
    let subdivisions = args.get(2).map_or(Ok(0), |v| v.parse::<u32>())?;
    if subdivisions > 2 {
        return Err("fixture subdivisions must be 0..2".into());
    }
    let source = if args.len() >= 2 {
        sphere(subdivisions)?
    } else {
        cube()?
    };
    let plan = PreparationPlan::new(
        SourceBinding {
            original_sha256: [0; 32],
            placement: ObjectTransform::identity(),
        },
        SamplingSpec {
            cell_mm: 0.1,
            distance_error_mm: 0.000_001,
            max_cells: 1,
        },
        vec![Operation {
            id: 1,
            kind: OperationKind::Hollow(HollowSpec {
                blockers: Vec::new(),
                thickness_mm: 2.,
                guarantee: None,
            }),
        }],
        PlanLimits::default(),
        &mut Cancellation::never(),
    )?;
    let start = Instant::now();
    let result = hollow_mesh(
        &source,
        &plan,
        HollowLimits::default(),
        &mut Cancellation::never(),
    )?;
    let elapsed = start.elapsed();
    fs::create_dir(&args[0])?;
    write_stl(result.mesh(), &Path::new(&args[0]).join("shell.stl"))?;
    write_stl(result.cavity(), &Path::new(&args[0]).join("cavity.stl"))?;
    let cert = result.certificate();
    let record = serde_json::json!({"backend":result.backend_id(),"elapsed_ms":elapsed.as_secs_f64()*1000.,"vertices":result.mesh().vertices().len(),"triangles":result.mesh().triangles().len(),"work_units":result.work_units(),"requested_bytes":result.requested_bytes(),"wall_offset_error_mm":cert.wall_offset_error_mm,"source_volume_mm3":[cert.source_volume.minimum_mm3,cert.source_volume.maximum_mm3],"cavity_volume_mm3":[cert.cavity_volume.minimum_mm3,cert.cavity_volume.maximum_mm3],"material_volume_mm3":[cert.material_volume.minimum_mm3,cert.material_volume.maximum_mm3],"stl":"ASCII reference with round-trip f64 coordinates"});
    let geometry = serde_json::json!({"schema":1,"source":{"vertices":source.vertices().iter().map(|v|v.coordinates()).collect::<Vec<_>>(),"triangles":source.triangles()},"cavity":{"vertices":result.cavity().vertices().iter().map(|v|v.coordinates()).collect::<Vec<_>>(),"triangles":result.cavity().triangles()},"shell":{"vertices":result.mesh().vertices().iter().map(|v|v.coordinates()).collect::<Vec<_>>(),"triangles":result.mesh().triangles()}});
    fs::write(
        Path::new(&args[0]).join("geometry.json"),
        serde_json::to_vec(&geometry)?,
    )?;
    fs::write(
        Path::new(&args[0]).join("report.json"),
        serde_json::to_vec_pretty(&record)?,
    )?;
    println!("{record}");
    Ok(())
}
