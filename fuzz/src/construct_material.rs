//! Bounded exact construction with independent tiny-coordinate occupancy probes.
use karasu_core::{
    cancellation::{Cancellation, Checkpoint},
    slicing::construct::{
        union_material_rings, union_material_rings_with_cancellation, ConstructLimits, MaterialRing,
    },
};
fn byte(data: &[u8], index: usize) -> u8 {
    data.get(index).copied().unwrap_or(0)
}
fn scalar(negative: bool, limbs: [u64; 8], denominator: [u64; 8]) -> f64 {
    let number = |words: [u64; 8]| {
        words
            .iter()
            .rev()
            .fold(0., |sum, &word| sum * 18446744073709551616. + word as f64)
    };
    let value = number(limbs) / number(denominator);
    if negative {
        -value
    } else {
        value
    }
}
// Probe arithmetic is independent of constructor predicates. Values are tiny; skip probes on or
// within 1e-9 of a represented edge, so floating conversion cannot select boundary membership.
fn inside(points: &[[f64; 2]], query: [f64; 2]) -> Option<bool> {
    let mut inside = false;
    for (&first, &second) in points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
    {
        let cross = (second[0] - first[0]) * (query[1] - first[1])
            - (second[1] - first[1]) * (query[0] - first[0]);
        if cross.abs() < 1e-9
            && query[0] >= first[0].min(second[0]) - 1e-9
            && query[0] <= first[0].max(second[0]) + 1e-9
            && query[1] >= first[1].min(second[1]) - 1e-9
            && query[1] <= first[1].max(second[1]) + 1e-9
        {
            return None;
        }
        if (first[1] > query[1]) != (second[1] > query[1])
            && query[0]
                < first[0] + (query[1] - first[1]) * (second[0] - first[0]) / (second[1] - first[1])
        {
            inside = !inside;
        }
    }
    Some(inside)
}
/// Decode two to four simple rectangles/triangles; check occupancy, provenance and determinism.
/// Returns success for uninstrumented replay; configured bounded inputs are expected to construct.
pub fn check_construct_material(data: &[u8]) -> bool {
    let count = 2 + usize::from(byte(data, 0) % 3);
    let points: Vec<Vec<[i64; 2]>> = (0..count)
        .map(|ring| {
            let offset = 1 + ring * 5;
            let x = i64::from(byte(data, offset) % 25) - 12;
            let y = i64::from(byte(data, offset + 1) % 25) - 12;
            let right = x + i64::from(byte(data, offset + 2) % 12) + 1;
            let top = y + i64::from(byte(data, offset + 3) % 12) + 1;
            if byte(data, offset + 4) & 1 == 0 {
                vec![[x, y], [right, y], [right, top], [x, top]]
            } else {
                vec![[x, y], [right, y], [x, top]]
            }
        })
        .collect();
    let labels: Vec<_> = (0..count)
        .map(|ring| u64::from(byte(data, 22 + ring) % 3))
        .collect();
    let rings: Vec<_> = points
        .iter()
        .enumerate()
        .map(|(source_path, vertices)| MaterialRing {
            vertices,
            material: labels[source_path],
            source_path,
        })
        .collect();
    let limits = ConstructLimits {
        max_input_edges: 16,
        max_pairs: 256,
        max_vertices: 256,
        max_atoms: 256,
        max_work: 1_000_000,
        max_payload_bytes: 1 << 20,
    };
    let result =
        union_material_rings(&rings, limits).expect("bounded simple polygons must construct");
    assert_eq!(result, union_material_rings(&rings, limits).unwrap());
    let decoded: Vec<_> = result
        .vertices()
        .iter()
        .map(|vertex| {
            let (negative_x, x) = vertex.numerator_x();
            let (negative_y, y) = vertex.numerator_y();
            [
                scalar(negative_x, x, vertex.denominator()),
                scalar(negative_y, y, vertex.denominator()),
            ]
        })
        .collect();
    let output: Vec<Vec<_>> = (0..result.rings().len())
        .map(|ring| {
            result
                .ring_edges(ring)
                .unwrap()
                .iter()
                .map(|&edge| decoded[result.edges()[edge].endpoints[0]])
                .collect()
        })
        .collect();
    let inputs: Vec<Vec<_>> = points
        .iter()
        .map(|ring| {
            ring.iter()
                .map(|point| [point[0] as f64, point[1] as f64])
                .collect()
        })
        .collect();
    for probe in 0..16 {
        let query = [
            f64::from(byte(data, probe) % 49) / 2. - 12. + 1. / 7.,
            f64::from(byte(data, probe + 9) % 49) / 2. - 12. + 1. / 11.,
        ];
        let mut parity = [false; 3];
        let mut valid = true;
        for (ring, &label) in inputs.iter().zip(&labels) {
            if let Some(value) = inside(ring, query) {
                parity[label as usize] ^= value;
            } else {
                valid = false;
            }
        }
        let mut observed = false;
        for ring in &output {
            if let Some(value) = inside(ring, query) {
                observed ^= value;
            } else {
                valid = false;
            }
        }
        if valid {
            assert_eq!(observed, parity.into_iter().any(|value| value));
        }
    }
    for atom in 0..result.atoms().len() {
        let supports = result.atom_supports(atom).unwrap();
        assert!(!supports.is_empty());
        assert!(supports
            .iter()
            .all(|support| support.path < count && support.edge < points[support.path].len()));
    }
    for ring in 0..result.rings().len() {
        for &edge in result.ring_edges(ring).unwrap() {
            assert!(edge < result.edges().len());
        }
    }
    assert!(result.payload_bytes() <= limits.max_payload_bytes);
    let mut tight = limits;
    tight.max_input_edges = usize::from(byte(data, 27) % 17);
    assert_eq!(
        union_material_rings(&rings, tight),
        union_material_rings(&rings, tight)
    );
    let mut predicate = |point| point == Checkpoint::ConstructComplete;
    assert!(union_material_rings_with_cancellation(
        &rings,
        limits,
        &mut Cancellation::new(&mut predicate)
    )
    .is_err());
    true
}
