use crate::geometry::{Mesh, Vec3};

const HEADER_LENGTH: usize = 80;
const TRIANGLE_RECORD_LENGTH: usize = 50;

pub(crate) fn binary_stl(mesh: &Mesh, fixture_id: &str) -> Result<Vec<u8>, String> {
    let triangle_count = u32::try_from(mesh.triangles.len())
        .map_err(|_| "binary STL supports at most u32::MAX triangles".to_owned())?;
    let records_size = TRIANGLE_RECORD_LENGTH
        .checked_mul(mesh.triangles.len())
        .ok_or_else(|| "binary STL size overflow".to_owned())?;
    let capacity = HEADER_LENGTH
        .checked_add(4)
        .and_then(|size| size.checked_add(records_size))
        .ok_or_else(|| "binary STL size overflow".to_owned())?;
    let mut output = Vec::with_capacity(capacity);

    // Version-1 corpus identity is intentionally stable across the Karasu product rename.
    let header = format!("Resin Slicer {fixture_id} deterministic reference fixture");
    let mut padded_header = [0_u8; HEADER_LENGTH];
    let header_bytes = header.as_bytes();
    let copy_length = header_bytes.len().min(HEADER_LENGTH);
    padded_header[..copy_length].copy_from_slice(&header_bytes[..copy_length]);
    output.extend_from_slice(&padded_header);
    output.extend_from_slice(&triangle_count.to_le_bytes());

    for triangle in &mesh.triangles {
        let a = mesh.vertices[triangle[0]];
        let b = mesh.vertices[triangle[1]];
        let c = mesh.vertices[triangle[2]];
        let cross = b.sub(a).cross(c.sub(a));
        let normal = if cross.length() == 0.0 {
            Vec3::new(0.0, 0.0, 0.0)
        } else {
            cross.normalized()
        };
        write_vector(&mut output, normal);
        write_vector(&mut output, a);
        write_vector(&mut output, b);
        write_vector(&mut output, c);
        output.extend_from_slice(&0_u16.to_le_bytes());
    }

    debug_assert_eq!(output.len(), capacity);
    Ok(output)
}

fn write_vector(output: &mut Vec<u8>, vector: Vec3) {
    for coordinate in [vector.x, vector.y, vector.z] {
        output.extend_from_slice(&quantize_coordinate(coordinate).to_le_bytes());
    }
}

#[allow(clippy::cast_possible_truncation)]
pub(crate) fn quantize_coordinate(coordinate: f64) -> f32 {
    // Binary STL requires f32 coordinates; finiteness is validated before serialization.
    coordinate as f32
}

#[cfg(test)]
mod tests {
    use crate::geometry::{box_mesh, Vec3};

    use super::{binary_stl, HEADER_LENGTH, TRIANGLE_RECORD_LENGTH};

    #[test]
    fn binary_structure_has_expected_length_and_count() {
        let mesh = box_mesh(Vec3::new(-1.0, -1.0, 0.0), Vec3::new(1.0, 1.0, 2.0), false);
        let bytes = binary_stl(&mesh, "TEST").expect("serialization should succeed");
        assert_eq!(
            bytes.len(),
            HEADER_LENGTH + 4 + TRIANGLE_RECORD_LENGTH * mesh.triangles.len()
        );
        assert_eq!(
            u32::from_le_bytes(bytes[80..84].try_into().expect("four count bytes")),
            12
        );
        assert!(bytes[0..80].starts_with(b"Resin Slicer TEST"));
    }

    #[test]
    fn binary_serialization_is_deterministic() {
        let mesh = box_mesh(Vec3::new(-1.0, -1.0, 0.0), Vec3::new(1.0, 1.0, 2.0), false);
        assert_eq!(
            binary_stl(&mesh, "TEST").expect("first serialization"),
            binary_stl(&mesh, "TEST").expect("second serialization")
        );
    }
}
