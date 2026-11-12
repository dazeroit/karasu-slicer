//! Immutable M3-to-raster boundary. Approximation is confined to explicit raster projection.
use super::{
    cap, reserve, Edge, LcdProfile, OutsidePolicy, RasterConfig, RasterError, RasterLimits,
};
use crate::{
    cancellation::{Cancellation, Checkpoint},
    slicing::{
        construct::ExactPoint, engine::RegionLayer, normalize::CoordinateAgreement,
        regions::RegionCompleteness, winding::WindingLayer,
    },
};

pub(super) fn edge_count(
    layer: &RegionLayer,
    limits: RasterLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<usize, RasterError> {
    if let Some(material) = layer.material_union() {
        return Ok(material.edges().len());
    }
    if layer.completeness() != RegionCompleteness::Complete {
        return Err(RasterError::UnresolvedGeometry);
    }
    let mut count = 0usize;
    for (i, boundary) in layer.boundaries().iter().enumerate() {
        cancel.poll(Checkpoint::RasterInput, i)?;
        let n = layer
            .path_vertices(boundary.path)
            .ok_or(RasterError::InvalidGeometry)?
            .len();
        count = count.checked_add(n).ok_or(RasterError::SizeOverflow)?;
        cap(count, limits.max_edges, "edges")?;
        cap(
            count.checked_mul(3).ok_or(RasterError::SizeOverflow)?,
            limits.max_work,
            "work",
        )?;
    }
    Ok(count)
}

pub(super) fn edges(
    layer: &RegionLayer,
    profile: LcdProfile,
    config: RasterConfig,
    limits: RasterLimits,
    count: usize,
    cancel: &mut Cancellation<'_>,
) -> Result<Vec<Edge>, RasterError> {
    cap(
        count.checked_mul(3).ok_or(RasterError::SizeOverflow)?,
        limits.max_work,
        "work",
    )?;
    cancel.check(Checkpoint::RasterInput)?;
    let mut result = reserve(count)?;
    if let Some(material) = layer.material_union() {
        let grid = layer.grid_spec();
        let (exponent, origin) = material
            .dyadic_exponent_mm()
            .map_or((grid.step_exponent(), grid.origin_xy_mm()), |e| {
                (e, [0.; 2])
            });
        for (i, edge) in material.edges().iter().enumerate() {
            cancel.poll(Checkpoint::RasterInput, i)?;
            let [a, b] = edge.endpoints;
            let a = *material
                .vertices()
                .get(a)
                .ok_or(RasterError::InvalidGeometry)?;
            let b = *material
                .vertices()
                .get(b)
                .ok_or(RasterError::InvalidGeometry)?;
            let world_a = rational_world(a, exponent, origin, profile.pitch_mm())?;
            let world_b = rational_world(b, exponent, origin, profile.pitch_mm())?;
            push(&mut result, world_a, world_b, profile, config)?;
        }
    } else {
        for boundary in layer.boundaries() {
            let vertices = layer
                .path_vertices(boundary.path)
                .ok_or(RasterError::InvalidGeometry)?;
            if vertices.len() < 3 {
                return Err(RasterError::InvalidGeometry);
            }
            for (i, &a) in vertices.iter().enumerate() {
                cancel.poll(Checkpoint::RasterInput, result.len())?;
                let b = vertices[(i + 1) % vertices.len()];
                let xy = |id: usize| {
                    let vertex = layer
                        .vertices()
                        .get(id)
                        .ok_or(RasterError::InvalidGeometry)?;
                    match vertex.coordinates {
                        CoordinateAgreement::Agreed(p) => Ok([p.x, p.y]),
                        CoordinateAgreement::Conflict => Err(RasterError::UnresolvedGeometry),
                    }
                };
                push(&mut result, xy(a)?, xy(b)?, profile, config)?;
            }
        }
    }
    cancel.check(Checkpoint::RasterInput)?;
    Ok(result)
}
/// Directed segment count of a winding layer, checked against the edge and work limits.
pub(super) fn winding_edge_count(
    layer: &WindingLayer,
    limits: RasterLimits,
) -> Result<usize, RasterError> {
    let count = layer.segments().len();
    cap(count, limits.max_edges, "edges")?;
    cap(
        count.checked_mul(3).ok_or(RasterError::SizeOverflow)?,
        limits.max_work,
        "work",
    )?;
    Ok(count)
}

/// Pixel-space directed edges of a winding layer, in segment order, reserving `count` edges.
///
/// Endpoints follow the outside policy like strict boundaries. A segment whose endpoints project
/// to one pixel-space point is dropped: it is horizontal there and changes no crossing, and its
/// neighbours share that point, so the section stays closed.
#[allow(clippy::float_cmp)] // Exact collapse only.
pub(super) fn winding_edges(
    layer: &WindingLayer,
    profile: LcdProfile,
    config: RasterConfig,
    count: usize,
    cancel: &mut Cancellation<'_>,
) -> Result<Vec<Edge>, RasterError> {
    segment_edges(layer.segments(), profile, config, count, cancel)
}

/// Pixel-space directed edges of world `segments`, as [`winding_edges`] makes a layer's: for the
/// segments a cut leaves (TD-123), reserving `count` edges.
#[allow(clippy::float_cmp)] // Exact collapse only.
pub(super) fn segment_edges(
    segments: &[[[f64; 2]; 2]],
    profile: LcdProfile,
    config: RasterConfig,
    count: usize,
    cancel: &mut Cancellation<'_>,
) -> Result<Vec<Edge>, RasterError> {
    cancel.check(Checkpoint::RasterInput)?;
    let mut result = reserve(count)?;
    for (i, &[from, to]) in segments.iter().enumerate() {
        cancel.poll(Checkpoint::RasterInput, i)?;
        let (a, b) = (profile.world_to_pixel(from)?, profile.world_to_pixel(to)?);
        if config.outside == OutsidePolicy::Reject {
            outside(profile, a)?;
            outside(profile, b)?;
        }
        if a != b {
            result.push(Edge { a, b });
        }
    }
    cancel.check(Checkpoint::RasterInput)?;
    Ok(result)
}

/// Total vertex count of closed cut rings: the edge capacity [`ring_edges`] reserves. Checked
/// against the edge limit; the caller includes it in the payload preflight before building.
pub(super) fn ring_edge_count(
    rings: &[Vec<[f64; 2]>],
    limits: RasterLimits,
) -> Result<usize, RasterError> {
    let count = rings
        .iter()
        .try_fold(0usize, |n, ring| n.checked_add(ring.len()))
        .ok_or(RasterError::SizeOverflow)?;
    cap(count, limits.max_edges, "edges")?;
    Ok(count)
}

/// Pixel-space edges of closed world-space cut rings (mm), in ring order, reserving `count`
/// (from [`ring_edge_count`]) edges.
///
/// Cut geometry only removes material, so vertices outside the panel are kept (the scanline
/// clamps spans to the panel) and edges that collapse to one pixel-space point are dropped: they
/// change no crossing. Every ring is traversed counter-clockwise in world XY (a clockwise ring
/// is walked backwards), so overlapping rings add their windings and the cut is their union
/// (TD-089).
#[allow(clippy::float_cmp)] // Exact collapse only.
pub(super) fn ring_edges(
    rings: &[Vec<[f64; 2]>],
    profile: LcdProfile,
    count: usize,
    cancel: &mut Cancellation<'_>,
) -> Result<Vec<Edge>, RasterError> {
    let mut result = reserve(count)?;
    for (index, ring) in rings.iter().enumerate() {
        cancel.poll(Checkpoint::RasterInput, index)?;
        let n = ring.len();
        // Twice the signed area, by the shoelace formula.
        let area: f64 = (0..n)
            .map(|k| {
                let (a, b) = (ring[k], ring[(k + 1) % n]);
                a[0] * b[1] - b[0] * a[1]
            })
            .sum();
        for k in 0..n {
            let (a, b) = if area < 0.0 {
                (ring[(n - k) % n], ring[n - 1 - k])
            } else {
                (ring[k], ring[(k + 1) % n])
            };
            let (pa, pb) = (profile.world_to_pixel(a)?, profile.world_to_pixel(b)?);
            if pa != pb {
                result.push(Edge { a: pa, b: pb });
            }
        }
    }
    cancel.check(Checkpoint::RasterInput)?;
    Ok(result)
}

#[allow(clippy::float_cmp)] // Detect exact collapse; approximate equality would erase valid small edges.
fn push(
    output: &mut Vec<Edge>,
    a: [f64; 2],
    b: [f64; 2],
    profile: LcdProfile,
    config: RasterConfig,
) -> Result<(), RasterError> {
    let a = profile.world_to_pixel(a)?;
    let b = profile.world_to_pixel(b)?;
    if a == b {
        return Err(RasterError::NumericRange);
    }
    if config.outside == OutsidePolicy::Reject {
        outside(profile, a)?;
        outside(profile, b)?;
    }
    output.push(Edge { a, b });
    Ok(())
}
/// [`RasterError::OutsidePanel`] for a pixel-space point beyond the panel.
fn outside(profile: LcdProfile, p: [f64; 2]) -> Result<(), RasterError> {
    for (axis, &value) in p.iter().enumerate() {
        if value < 0. || value > f64::from(profile.dimensions()[axis]) {
            return Err(RasterError::OutsidePanel);
        }
    }
    Ok(())
}
// A bounded Horner conversion has less than 16 binary64 ulps of relative error. The numerical
// domain check budgets this approximation together with physical projection and interpolation.
#[allow(clippy::cast_precision_loss)] // Explicit, documented lossy raster boundary, never M3 geometry.
fn magnitude(words: [u64; 8]) -> f64 {
    words.into_iter().rev().fold(0., |value, word| {
        value * 18_446_744_073_709_551_616. + word as f64
    })
}
fn scale(mut value: f64, mut exponent: i16) -> f64 {
    // Multiplying a subnormal power first would discard useful significand bits prematurely.
    while exponent < -512 {
        value *= 2_f64.powi(-512);
        exponent += 512;
    }
    while exponent > 512 {
        value *= 2_f64.powi(512);
        exponent -= 512;
    }
    value * 2_f64.powi(i32::from(exponent))
}
fn rational_world(
    point: ExactPoint,
    exponent: i16,
    origin: [f64; 2],
    pitch: [f64; 2],
) -> Result<[f64; 2], RasterError> {
    let denominator = magnitude(point.denominator());
    let mut world = [0.; 2];
    for (axis, (negative, words)) in [point.numerator_x(), point.numerator_y()]
        .into_iter()
        .enumerate()
    {
        let numerator = magnitude(words);
        let value = scale(numerator / denominator, exponent);
        if !value.is_finite()
            || (numerator != 0. && value == 0.)
            || origin[axis].abs() / pitch[axis] + value.abs() / pitch[axis] > super::NUMERIC_DOMAIN
        {
            return Err(RasterError::NumericRange);
        }
        world[axis] = origin[axis] + if negative { -value } else { value };
        if !world[axis].is_finite() {
            return Err(RasterError::NumericRange);
        }
    }
    Ok(world)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scale_retains_subnormal_range() {
        assert_eq!(
            scale(2_f64.powi(52), -1074).to_bits(),
            2_f64.powi(-1022).to_bits()
        );
        assert_eq!(scale(1., -1074).to_bits(), 1);
        assert_eq!(scale(0.5, 1023), 2_f64.powi(1022));
    }

    #[test]
    fn cut_rings_are_walked_counter_clockwise() {
        // A clockwise square comes out as the counter-clockwise one, so overlapping rings add
        // their windings (TD-089).
        let profile = LcdProfile::try_new(
            [10, 10],
            [0., 0.],
            [10., 10.],
            crate::raster::Orientation::default(),
        )
        .unwrap();
        let clockwise = vec![[1., 1.], [1., 3.], [3., 3.], [3., 1.]];
        let counter: Vec<[f64; 2]> = clockwise.iter().rev().copied().collect();
        let mut cancel = Cancellation::never();
        let a = ring_edges(&[clockwise], profile, 4, &mut cancel).unwrap();
        let b = ring_edges(&[counter], profile, 4, &mut cancel).unwrap();
        let area = |edges: &[Edge]| -> f64 {
            edges
                .iter()
                .map(|e| e.a[0] * e.b[1] - e.b[0] * e.a[1])
                .sum()
        };
        assert!(area(&a) > 0.0 && area(&b) > 0.0);
        assert_eq!(area(&a), area(&b));
    }
}
