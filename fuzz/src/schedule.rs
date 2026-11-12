//! Bounded layer scheduling checks with independent native-FMA and integer oracles.

use karasu_core::slicing::schedule::{
    build_layer_schedule, LayerPlane, ScheduleEnd, ScheduleError, ScheduleLimits,
    UniformScheduleSpec,
};

fn word<const N: usize>(data: &[u8], offset: usize) -> [u8; N] {
    std::array::from_fn(|i| data.get(offset + i).copied().unwrap_or(0))
}

fn byte(data: &[u8], offset: usize) -> u8 {
    data.get(offset).copied().unwrap_or(0)
}

/// Decode at most 26 bytes and exercise the public builder twice under a 64-plane payload cap.
///
/// Bytes 0..24 are raw LE binary64 first/height/terminal values; byte 24 selects six modes:
/// raw Count/Before/Through, zero-first Count, or bounded dyadic Before/Through. Byte 25 is
/// Count modulo 66. Dyadic modes use signed LE i16 words at 0 and 4 modulo 64, an unsigned
/// word at 2 modulo 64 plus one, and a common 1/16 mm unit. Missing bytes are zero.
/// Returns successful schedule acceptance for deterministic, uninstrumented seed replay.
pub fn check_layer_schedule(data: &[u8]) -> bool {
    let mode = byte(data, 24) % 6;
    let count = u64::from(byte(data, 25) % 66);
    let first_units = i32::from(i16::from_le_bytes(word(data, 0))) % 64;
    let height_units = i32::from(u16::from_le_bytes(word(data, 2)) % 64) + 1;
    let terminal_units = i32::from(i16::from_le_bytes(word(data, 4))) % 64;
    let dyadic = mode >= 4;
    let first = if dyadic {
        f64::from(first_units) / 16.0
    } else if mode == 3 {
        0.0
    } else {
        f64::from_le_bytes(word(data, 0))
    };
    let height = if dyadic {
        f64::from(height_units) / 16.0
    } else {
        f64::from_le_bytes(word(data, 8))
    };
    let terminal = if dyadic {
        f64::from(terminal_units) / 16.0
    } else {
        f64::from_le_bytes(word(data, 16))
    };
    let end = match mode {
        0 | 3 => ScheduleEnd::Count(count),
        1 | 4 => ScheduleEnd::Before(terminal),
        _ => ScheduleEnd::Through(terminal),
    };
    let spec = UniformScheduleSpec::try_new(first, height, end);
    assert_eq!(spec, UniformScheduleSpec::try_new(first, height, end));
    let Ok(spec) = spec else { return false };
    let limits = ScheduleLimits {
        max_layers: 64,
        max_payload_bytes: 64 * std::mem::size_of::<LayerPlane>(),
    };
    let result = build_layer_schedule(spec, limits);
    assert_eq!(result, build_layer_schedule(spec, limits));
    if let Ok(schedule) = &result {
        assert_eq!(schedule.spec(), spec);
        assert!(schedule.planes().len() <= 64);
        for (index, plane) in schedule.planes().iter().enumerate() {
            assert_eq!(plane.index, index as u64);
            assert!(plane.z_mm.is_finite());
            assert_eq!(plane.nominal_height_mm.to_bits(), height.to_bits());
            // Indices <=63 are exactly representable: native FMA is a separate
            // single-rounding implementation of the exact expression a+i*h.
            let expected = f64::from(u32::try_from(index).unwrap()).mul_add(height, first);
            assert_eq!(
                plane.z_mm.to_bits(),
                if expected == 0.0 {
                    0
                } else {
                    expected.to_bits()
                }
            );
            if index > 0 {
                assert!(schedule.planes()[index - 1].z_mm < plane.z_mm);
            }
        }
        if let ScheduleEnd::Count(count) = end {
            assert_eq!(schedule.planes().len() as u64, count);
        }
    }
    if matches!(end, ScheduleEnd::Count(65)) {
        assert!(matches!(
            result,
            Err(ScheduleError::LayerLimit { limit: 64 })
        ));
    }
    if dyadic {
        // Integer membership is independent of binary64 decoding/search. The
        // generated range is at most 127 layers, with exact published coordinates.
        let expected: Vec<_> = (0_i32..128)
            .map(|i| first_units + i * height_units)
            .take_while(|&z| z < terminal_units || (mode == 5 && z == terminal_units))
            .map(|z| f64::from(z) / 16.0)
            .collect();
        if expected.len() > 64 {
            assert!(matches!(
                result,
                Err(ScheduleError::LayerLimit { limit: 64 })
            ));
        } else {
            let schedule = result.as_ref().expect("bounded exact dyadic schedule");
            assert_eq!(
                schedule.planes().iter().map(|p| p.z_mm).collect::<Vec<_>>(),
                expected
            );
        }
    }
    if mode == 3 && count <= 64 {
        let expected: Vec<_> = (0..u32::try_from(count).unwrap())
            .map(|i| f64::from(i) * height)
            .collect();
        if let Some(index) = expected.iter().position(|z| !z.is_finite()) {
            assert!(
                matches!(result, Err(ScheduleError::PlaneOutOfRange { index: actual }) if actual == index as u64)
            );
        } else {
            let schedule = result
                .as_ref()
                .expect("zero-first bounded count has strictly increasing rounded coordinates");
            assert_eq!(
                schedule
                    .planes()
                    .iter()
                    .map(|p| p.z_mm.to_bits())
                    .collect::<Vec<_>>(),
                expected.iter().map(|z| z.to_bits()).collect::<Vec<_>>()
            );
        }
    }
    result.is_ok()
}
