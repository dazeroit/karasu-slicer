//! Small valid solids and adversarial represented-coordinate hollowing requests.
use karasu_core::{
    cancellation::Cancellation,
    geometry::{ObjectTransform, Point3, Rotation3, Vector3},
    mesh::TriangleMesh,
    preparation::{
        hollow::{hollow_mesh, HollowLimits},
        HollowSpec, Operation, OperationKind, PlanLimits, PreparationPlan, SamplingSpec,
        SourceBinding,
    },
};

const MAX_BYTES: usize = 2048;
const ERROR_MM: f64 = 1e-5;

fn byte(data: &[u8], offset: usize) -> u8 {
    data.get(offset).copied().unwrap_or(0)
}

fn raw_number(data: &[u8], offset: usize) -> f64 {
    let bits = u64::from_le_bytes(std::array::from_fn(|index| byte(data, offset + index)));
    match byte(data, offset) % 8 {
        0 => f64::from_bits(bits),
        1 => f64::NAN,
        2 => f64::INFINITY,
        3 => f64::MAX,
        4 => f64::from_bits(1),
        _ => {
            f64::from(i16::from_le_bytes([
                byte(data, offset),
                byte(data, offset + 1),
            ])) / 32.0
        }
    }
}

fn source(data: &[u8]) -> Result<TriangleMesh, karasu_core::mesh::MeshError> {
    let mode = byte(data, 0) % 4;
    if mode == 0 {
        let count = usize::from(byte(data, 1) % 17);
        let vertices = (0..count)
            .map(|index| {
                Point3::new(
                    raw_number(data, 8 + index * 24),
                    raw_number(data, 16 + index * 24),
                    raw_number(data, 24 + index * 24),
                )
            })
            .collect();
        let triangles = (0..usize::from(byte(data, 2) % 33))
            .map(|index| {
                std::array::from_fn(|corner| u32::from(byte(data, 400 + index * 3 + corner) % 32))
            })
            .collect();
        return TriangleMesh::try_new(vertices, triangles);
    }
    if mode == 1 {
        return TriangleMesh::try_new(
            vec![
                Point3::new(0., 0., 0.),
                Point3::new(8., 0., 0.),
                Point3::new(0., 8., 0.),
                Point3::new(0., 0., 8.),
            ],
            vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
        );
    }
    let vertices = [
        [0., 0., 0.],
        [8., 0., 0.],
        [8., 8., 0.],
        [0., 8., 0.],
        [0., 0., 8.],
        [8., 0., 8.],
        [8., 8., 8.],
        [0., 8., 8.],
    ]
    .map(|[x, y, z]| Point3::new(x, y, z))
    .to_vec();
    let mut triangles = vec![
        [0, 2, 1],
        [0, 3, 2],
        [4, 5, 6],
        [4, 6, 7],
        [0, 1, 5],
        [0, 5, 4],
        [1, 2, 6],
        [1, 6, 5],
        [2, 3, 7],
        [2, 7, 6],
        [3, 0, 4],
        [3, 4, 7],
    ];
    if mode == 3 {
        match byte(data, 1) % 4 {
            0 => {
                triangles.pop();
            }
            1 => triangles.push(triangles[0]),
            2 => triangles[0].swap(0, 1),
            _ => triangles[0][0] = 31,
        }
    }
    TriangleMesh::try_new(vertices, triangles)
}

fn placement(data: &[u8]) -> ObjectTransform {
    let translation =
        std::array::from_fn::<_, 3, _>(|axis| f64::from(byte(data, 3 + axis) % 33) / 4. - 4.);
    let scale =
        std::array::from_fn::<_, 3, _>(|axis| 0.5 + f64::from(byte(data, 6 + axis) % 7) / 4.);
    ObjectTransform::new(
        Vector3::new(translation[0], translation[1], translation[2]),
        Rotation3::identity(),
        Vector3::new(scale[0], scale[1], scale[2]),
        Point3::new(0., 0., 0.),
    )
    .unwrap()
}

fn signed_volume(mesh: &TriangleMesh) -> f64 {
    mesh.triangles()
        .iter()
        .map(|face| {
            let [a, b, c] = face.map(|index| mesh.vertices()[index as usize].coordinates());
            (a[0] * (b[1] * c[2] - b[2] * c[1])
                + a[1] * (b[2] * c[0] - b[0] * c[2])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.
        })
        .sum()
}

/// Exercise bounded public hollowing; typed invalid/unsupported/resource outcomes are expected.
///
/// Successful results must retain the source and exact plan, contain two closed surfaces, carry
/// finite positive volume certificates, and preserve the declared exterior/cavity winding contract.
/// This tests output contracts, not a substitute implementation of halfspace construction.
pub fn check_hollow_mesh(data: &[u8]) -> bool {
    if data.len() > MAX_BYTES {
        return false;
    }
    let Ok(source) = source(data) else {
        return false;
    };
    let transform = placement(data);
    let thickness_mm = match byte(data, 9) % 16 {
        13 => f64::NAN,
        14 => -1.,
        15 => f64::INFINITY,
        value => 0.05 + f64::from(value) / 4.,
    };
    let Ok(plan) = PreparationPlan::new(
        SourceBinding {
            original_sha256: [55; 32],
            placement: transform,
        },
        SamplingSpec {
            cell_mm: 0.1,
            distance_error_mm: ERROR_MM,
            max_cells: 1_000_000,
        },
        vec![Operation {
            id: 1,
            kind: OperationKind::Hollow(HollowSpec { thickness_mm }),
        }],
        PlanLimits::default(),
        &mut Cancellation::never(),
    ) else {
        return false;
    };
    let original_vertices = source.vertices().to_vec();
    let original_faces = source.triangles().to_vec();
    let original_plan = plan.canonical_record().to_vec();
    let limits = HollowLimits {
        max_input_vertices: 16,
        max_input_triangles: 32,
        max_planes: 32,
        max_output_vertices: 256,
        max_output_triangles: 512,
        max_work: 200_000,
        max_payload_bytes: 2 * 1024 * 1024,
    };
    let result = hollow_mesh(&source, &plan, limits, &mut Cancellation::never());
    assert_eq!(source.vertices(), original_vertices);
    assert_eq!(source.triangles(), original_faces);
    assert_eq!(plan.canonical_record(), original_plan);
    let Ok(result) = result else {
        return false;
    };
    assert_eq!(result.plan_record(), original_plan);
    assert!(result.work_units() <= limits.max_work);
    assert!(result.requested_bytes() <= limits.max_payload_bytes);
    let certificate = result.certificate();
    assert!(certificate.wall_offset_error_mm.is_finite());
    assert!(certificate.wall_offset_error_mm >= 0. && certificate.wall_offset_error_mm <= ERROR_MM);
    for bounds in [
        certificate.source_volume,
        certificate.cavity_volume,
        certificate.material_volume,
    ] {
        assert!(bounds.minimum_mm3.is_finite() && bounds.maximum_mm3.is_finite());
        assert!(bounds.minimum_mm3 > 0. && bounds.minimum_mm3 <= bounds.maximum_mm3);
    }
    let mesh = result.mesh();
    let cavity = result.cavity();
    assert_eq!(
        mesh.vertices().len(),
        source.vertices().len() + cavity.vertices().len()
    );
    assert_eq!(
        mesh.triangles().len(),
        source.triangles().len() + cavity.triangles().len()
    );
    assert_eq!(&mesh.triangles()[..original_faces.len()], original_faces);
    for (index, &point) in source.vertices().iter().enumerate() {
        assert_eq!(
            mesh.vertices()[index],
            transform.transform_point(point).unwrap()
        );
    }
    for output in [mesh, cavity] {
        assert!(output
            .vertices()
            .iter()
            .all(|point| point.coordinates().iter().all(|value| value.is_finite())));
        let topology = output
            .analyze_topology()
            .expect("small successful mesh topology");
        assert!(topology.summary().closed_combinatorial_surface);
        assert_eq!(
            topology.summary().edge_components,
            if std::ptr::eq(output, mesh) { 2 } else { 1 }
        );
    }
    let cavity_volume = signed_volume(cavity);
    let material_volume = signed_volume(mesh);
    assert!(cavity_volume > 0. && material_volume > 0.);
    let source_volume = certificate.source_volume.minimum_mm3;
    assert!((source_volume - cavity_volume - material_volume).abs() < 1e-8 * source_volume.max(1.));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seed_solids_and_invalid_inputs_have_expected_outcomes() {
        assert!(check_hollow_mesh(&[2]));
        assert!(check_hollow_mesh(&[1]));
        assert!(!check_hollow_mesh(&[3]));
        assert!(!check_hollow_mesh(&[0, 16, 32]));
        let mut nonfinite_thickness = vec![2; 10];
        nonfinite_thickness[9] = 13;
        assert!(!check_hollow_mesh(&nonfinite_thickness));
    }
}
