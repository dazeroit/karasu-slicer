//! Bounded binary and line-oriented ASCII STL import with explicit unit and detection policies.
//!
//! Auto detection gives exact binary record-length agreement precedence over `solid` text. Otherwise
//! an ASCII `solid` opener selects strict ASCII; binary-looking data selects binary validation.
//! Select a format explicitly to disambiguate unusual/malformed data. No parser retries another
//! grammar after a selected parser fails. Binary trailing bytes are rejected, including whitespace.
//!
//! ```
//! use karasu_core::mesh::stl::{import_stl_bytes, StlOptions, StlUnitPolicy};
//! let options = StlOptions::new(StlUnitPolicy::AssumeMillimeters);
//! let result = import_stl_bytes(b"solid empty\nendsolid empty\n", options)?;
//! assert_eq!(result.mesh.statistics().triangle_count, 0);
//! assert_eq!(result.mesh.source_bounds(), None);
//! # Ok::<(), karasu_core::mesh::StlError>(())
//! ```

use super::weld::{weld, WeldError};
use super::{
    diagnostics, FaceNormal, ImportResource, MeshDiagnostic, MeshError, StlError, TriangleMesh,
};
use crate::cancellation::{Cancellation, Checkpoint};
use crate::geometry::{Point3, Vector3};
use std::{fs::File, io::Read, path::Path};

/// STL contains no standard physical-unit declaration. This choice is recorded, not guessed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StlUnitPolicy {
    /// Interpret one file coordinate unit as one millimeter without scaling coordinates.
    AssumeMillimeters,
}

/// Detection policy or the actual successfully parsed representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StlFormat {
    /// Apply the documented structural precedence, without header-only guessing.
    Auto,
    /// Standard 80-byte header, count, and 50-byte records.
    Binary,
    /// One solid using the documented line-oriented ASCII grammar.
    Ascii,
}

/// Resource caps checked before growing import buffers. They are not a total resident-memory cap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImportLimits {
    /// Maximum encoded file size; reader imports consume at most one additional detection byte.
    pub max_input_bytes: usize,
    /// Maximum facet count, including defective facets.
    pub max_triangles: usize,
    /// Maximum exactly distinct positions; indices must also fit `u32`.
    pub max_vertices: usize,
    /// Maximum ASCII line bytes, excluding the LF separator (including a CR if present).
    pub max_ascii_line_bytes: usize,
    /// Maximum bytes per numeric token before floating-point parsing.
    pub max_numeric_token_bytes: usize,
}

impl Default for ImportLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 1280 * 1024 * 1024,
            max_triangles: 25_000_000,
            max_vertices: 75_000_000,
            max_ascii_line_bytes: 4096,
            max_numeric_token_bytes: 128,
        }
    }
}

/// What an import does with facets that have a non-finite (NaN or infinite) corner coordinate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NonFinitePolicy {
    /// Refuse the file at the first such coordinate ([`StlError::NonFiniteCoordinate`]).
    #[default]
    Refuse,
    /// Leave those facets out and report them in [`ImportedStl::dropped`]. Their positions are
    /// unknown, so nothing else can be made of them; whoever asks for this reports the loss
    /// (TD-092 r9).
    Drop,
}

/// At most this many runs of dropped facets are listed in [`DroppedFacets::ranges`].
pub const MAX_DROPPED_RUNS: usize = 16;

/// Facets left out for a non-finite corner coordinate ([`NonFinitePolicy::Drop`]), by source
/// facet number.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DroppedFacets {
    /// Facets left out.
    pub facets: usize,
    /// The first non-finite coordinate as `[facet, corner, axis]`, as a refusal names it.
    pub first: Option<[usize; 3]>,
    /// Runs of consecutive dropped facets.
    pub runs: usize,
    /// The first [`MAX_DROPPED_RUNS`] runs, as (first facet, facet count).
    pub ranges: Vec<(usize, usize)>,
    /// The facet after the last one dropped.
    next: usize,
}

impl DroppedFacets {
    fn add(&mut self, facet: usize, corner: usize, axis: usize) -> Result<(), StlError> {
        let continues = self.facets > 0 && self.next == facet;
        self.facets += 1;
        self.next = facet + 1;
        self.first.get_or_insert([facet, corner, axis]);
        if continues {
            // Only a listed run grows; runs past the listed ones are counted.
            if let Some(last) = self.ranges.last_mut().filter(|r| r.0 + r.1 == facet) {
                last.1 += 1;
            }
        } else {
            self.runs += 1;
            if self.ranges.len() < MAX_DROPPED_RUNS {
                reserve(&mut self.ranges, 1, "dropped facet runs")?;
                self.ranges.push((facet, 1));
            }
        }
        Ok(())
    }

    /// The refusal [`NonFinitePolicy::Refuse`] gives for the same file.
    #[must_use]
    pub fn refusal(&self) -> Option<StlError> {
        self.first
            .map(|[facet, corner, axis]| StlError::NonFiniteCoordinate {
                facet,
                corner,
                axis,
            })
    }
}

/// Explicit import policy; no unit assumption is selected by a hidden default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StlOptions {
    /// Unit interpretation, retained in the returned metadata.
    pub units: StlUnitPolicy,
    /// Auto detection or caller-specified grammar.
    pub format: StlFormat,
    /// Input and geometry limits.
    pub limits: ImportLimits,
    /// Facets with a non-finite corner coordinate: refused or dropped.
    pub non_finite: NonFinitePolicy,
}

impl StlOptions {
    /// Selects units explicitly, with auto detection, bounded default limits, and refusal of
    /// non-finite coordinates.
    #[must_use]
    pub fn new(units: StlUnitPolicy) -> Self {
        Self {
            units,
            format: StlFormat::Auto,
            limits: ImportLimits::default(),
            non_finite: NonFinitePolicy::Refuse,
        }
    }
}

/// Source header information, kept separately from geometry and never used as a unit declaration.
#[derive(Debug, PartialEq, Eq)]
pub enum StlHeader {
    /// Original opaque header bytes; may contain `solid` or non-text data.
    Binary([u8; 80]),
    /// Optional labels; differing labels are accepted, and neither is interpreted as geometry.
    Ascii {
        /// Text after `solid`, with surrounding whitespace trimmed.
        name: String,
        /// Text after `endsolid`, with surrounding whitespace trimmed.
        end_name: String,
    },
}

/// One original facet's non-topological data, aligned with mesh triangle order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FacetMetadata {
    /// Supplied components promoted/parsed as `f64`, including zero/non-finite values.
    /// Never defines winding; NaN payload/signaling-bit preservation is not promised.
    pub stored_normal: Vector3,
    /// Original binary attribute bits; `None` for ASCII. No extra attribute payload is supported.
    pub attribute: Option<u16>,
}

/// Original file metadata. Facet records cost `size_of::<FacetMetadata>()` each, separately from
/// the mesh; raw STL corner records and raw input bytes are not copied into the result.
#[derive(Debug, PartialEq)]
pub struct StlMetadata {
    /// Actual parsed header/format.
    pub header: StlHeader,
    /// Caller-selected unit interpretation.
    pub units: StlUnitPolicy,
    /// Normal/attribute values in source facet order.
    pub facets: Vec<FacetMetadata>,
}

/// Fully parsed geometry and non-destructive observations; no partially parsed mesh is returned.
#[derive(Debug, PartialEq)]
pub struct ImportedStl {
    /// Immutable indexed source mesh in millimeters.
    pub mesh: TriangleMesh,
    /// Original non-geometric data.
    pub metadata: StlMetadata,
    /// Geometry diagnostics first in triangle order, then file metadata diagnostics in facet order.
    pub diagnostics: Vec<MeshDiagnostic>,
    /// Facets left out under [`NonFinitePolicy::Drop`]; mesh triangles, facet metadata and
    /// diagnostics then follow the kept facets' order.
    pub dropped: DroppedFacets,
}

/// Imports a file without modifying it. The path is not copied into error messages or metadata.
///
/// # Errors
/// Returns the underlying I/O cause, a resource/format error, or contextual mesh validation error.
pub fn import_stl_path(
    path: impl AsRef<Path>,
    options: StlOptions,
) -> Result<ImportedStl, StlError> {
    import_stl_path_with_cancellation(path, options, &mut Cancellation::never())
}

/// Imports a path with cooperative cancellation checked before opening it.
/// Blocking file-open/read calls cannot be interrupted by the predicate.
///
/// # Errors
/// Returns typed cancellation or the ordinary import failures; no partial mesh escapes.
pub fn import_stl_path_with_cancellation(
    path: impl AsRef<Path>,
    options: StlOptions,
    cancellation: &mut Cancellation<'_>,
) -> Result<ImportedStl, StlError> {
    cancellation.check(Checkpoint::ImportStart)?;
    import_stl_reader_with_cancellation(File::open(path)?, options, cancellation)
}

/// Reads into a size-bounded buffer, then uses the same parser as byte-source imports.
/// The ordinary entry point never requests cancellation; see [`import_stl_reader_with_cancellation`]
/// for cooperative checks. Never reads more than `max_input_bytes + 1` bytes.
///
/// # Errors
/// Returns I/O, allocation, limit, or parsing errors without a partial result.
pub fn import_stl_reader(reader: impl Read, options: StlOptions) -> Result<ImportedStl, StlError> {
    import_stl_reader_with_cancellation(reader, options, &mut Cancellation::never())
}

/// Imports a reader with cooperative cancellation around each at-most-8-KiB read and in parsing.
/// A blocked read cannot be forcibly interrupted; consumed bytes cannot be rolled back.
/// `Interrupted` remains retryable, not cancellation. See [`crate::cancellation`].
///
/// # Errors
/// Returns typed cancellation or the ordinary import failures without partial success.
pub fn import_stl_reader_with_cancellation(
    mut reader: impl Read,
    options: StlOptions,
    cancellation: &mut Cancellation<'_>,
) -> Result<ImportedStl, StlError> {
    cancellation.check(Checkpoint::ImportStart)?;
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        cancellation.check(Checkpoint::Read)?;
        let remaining = options.limits.max_input_bytes - bytes.len();
        let size = remaining.min(buffer.len() - 1) + 1;
        let result = reader.read(&mut buffer[..size]);
        cancellation.check(Checkpoint::Read)?;
        let count = match result {
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if count == 0 {
            break;
        }
        check_limit(
            bytes
                .len()
                .checked_add(count)
                .ok_or(StlError::SizeOverflow)?,
            options.limits.max_input_bytes,
            ImportResource::InputBytes,
        )?;
        reserve(&mut bytes, count, "STL reader buffer")?;
        bytes.extend_from_slice(&buffer[..count]);
    }
    import_stl_bytes_with_cancellation(&bytes, options, cancellation)
}

/// Imports borrowed encoded bytes. Exact vertex identity uses finite `f64` coordinate bits after
/// signed-zero normalization, in first-occurrence order. No tolerance-based welding occurs.
///
/// # Errors
/// Returns structured malformed-input, non-finite-coordinate, size/limit, or allocation errors.
pub fn import_stl_bytes(bytes: &[u8], options: StlOptions) -> Result<ImportedStl, StlError> {
    import_stl_bytes_with_cancellation(bytes, options, &mut Cancellation::never())
}

/// Imports borrowed bytes with cancellation through detection, parsing, construction and diagnostics.
/// Pre-cancellation wins before validation; a final checkpoint precedes successful publication.
/// See [`crate::cancellation`] for phase-level limits and error precedence.
///
/// # Errors
/// Returns [`StlError::Cancelled`] or the ordinary import failure. Input bytes are never changed.
pub fn import_stl_bytes_with_cancellation(
    bytes: &[u8],
    options: StlOptions,
    cancellation: &mut Cancellation<'_>,
) -> Result<ImportedStl, StlError> {
    cancellation.check(Checkpoint::ImportStart)?;
    check_limit(
        bytes.len(),
        options.limits.max_input_bytes,
        ImportResource::InputBytes,
    )?;
    if bytes.is_empty() {
        return Err(StlError::EmptyInput);
    }
    let format = if options.format == StlFormat::Auto {
        detect(bytes, cancellation)?
    } else {
        options.format
    };
    let imported = match format {
        StlFormat::Binary => binary(bytes, options, cancellation),
        StlFormat::Ascii => ascii(bytes, options, cancellation),
        StlFormat::Auto => Err(StlError::UnrecognizedFormat),
    }?;
    cancellation.check(Checkpoint::ImportComplete)?;
    Ok(imported)
}

fn check_limit(actual: usize, limit: usize, resource: ImportResource) -> Result<(), StlError> {
    if actual > limit {
        Err(StlError::LimitExceeded { resource, limit })
    } else {
        Ok(())
    }
}

fn reserve<T>(buffer: &mut Vec<T>, count: usize, context: &'static str) -> Result<(), MeshError> {
    buffer
        .try_reserve(count)
        .map_err(|_| MeshError::Allocation { context })
}

fn binary_size(count: u32) -> Result<usize, StlError> {
    usize::try_from(count)
        .ok()
        .and_then(|n| n.checked_mul(50))
        .and_then(|n| n.checked_add(84))
        .ok_or(StlError::SizeOverflow)
}

fn count_at_header(bytes: &[u8]) -> u32 {
    // Callers validate at least 84 bytes before reading fixed fields.
    u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]])
}

fn text_bytes(
    bytes: &[u8],
    cancellation: &mut Cancellation<'_>,
    point: Checkpoint,
) -> Result<bool, StlError> {
    for chunk in bytes.chunks(8192) {
        cancellation.check(point)?;
        if !chunk.is_ascii() || chunk.contains(&0) {
            return Ok(false);
        }
    }
    cancellation.check(point)?;
    Ok(true)
}

fn detect(bytes: &[u8], cancellation: &mut Cancellation<'_>) -> Result<StlFormat, StlError> {
    cancellation.check(Checkpoint::Detection)?;
    if bytes.len() >= 84
        && binary_size(count_at_header(bytes)).is_ok_and(|size| size == bytes.len())
    {
        return Ok(StlFormat::Binary);
    }
    let is_text = text_bytes(bytes, cancellation, Checkpoint::Detection)?;
    if is_text {
        let text = std::str::from_utf8(bytes).map_err(|_| StlError::UnrecognizedFormat)?;
        if label(
            text.trim_start_matches(|c: char| c.is_ascii_whitespace()),
            "solid",
        )
        .is_some()
        {
            return Ok(StlFormat::Ascii);
        }
    }
    if bytes.len() >= 84 || !is_text {
        return Ok(StlFormat::Binary);
    }
    Err(StlError::UnrecognizedFormat)
}

/// Facet records collected before welding. ASCII facets keep their corner positions here; binary
/// facets are decoded from the input when welded.
struct Builder {
    corners: Vec<Point3>,
    facets: Vec<FacetMetadata>,
    options: StlOptions,
    dropped: DroppedFacets,
}

impl Builder {
    fn new(options: StlOptions) -> Self {
        Self {
            corners: Vec::new(),
            facets: Vec::new(),
            options,
            dropped: DroppedFacets::default(),
        }
    }

    fn facet(
        &mut self,
        points: [Point3; 3],
        metadata: FacetMetadata,
        cancellation: &mut Cancellation<'_>,
    ) -> Result<(), StlError> {
        // The source facet number counts dropped facets too.
        let facet = self.facets.len() + self.dropped.facets;
        cancellation.poll(Checkpoint::Deduplication, facet)?;
        check_limit(
            facet.checked_add(1).ok_or(StlError::SizeOverflow)?,
            self.options.limits.max_triangles,
            ImportResource::Triangles,
        )?;
        for (corner, point) in points.into_iter().enumerate() {
            if let Some(axis) = non_finite_axis(point) {
                if self.options.non_finite == NonFinitePolicy::Refuse {
                    return Err(StlError::NonFiniteCoordinate {
                        facet,
                        corner,
                        axis,
                    });
                }
                return self.dropped.add(facet, corner, axis);
            }
        }
        reserve(&mut self.corners, 3, "facet corners")?;
        reserve(&mut self.facets, 1, "facet metadata")?;
        self.corners.extend(points);
        self.facets.push(metadata);
        Ok(())
    }

    fn finish(
        self,
        header: StlHeader,
        cancellation: &mut Cancellation<'_>,
    ) -> Result<ImportedStl, StlError> {
        let corners = self.corners;
        assemble(
            self.options,
            header,
            self.facets,
            self.dropped,
            &|corner| corners[corner],
            cancellation,
        )
    }
}

/// The first axis of `point` with a non-finite coordinate.
fn non_finite_axis(point: Point3) -> Option<usize> {
    point.coordinates().iter().position(|v| !v.is_finite())
}

/// Weld the facets' corners (exact positions, first-occurrence order; TD-058), build the mesh and
/// collect geometry, stored-normal and attribute diagnostics.
fn assemble(
    options: StlOptions,
    header: StlHeader,
    facets: Vec<FacetMetadata>,
    dropped: DroppedFacets,
    corner: &(dyn Fn(usize) -> Point3 + Sync),
    cancellation: &mut Cancellation<'_>,
) -> Result<ImportedStl, StlError> {
    cancellation.check(Checkpoint::Deduplication)?;
    let corners = facets.len().checked_mul(3).ok_or(StlError::SizeOverflow)?;
    let welded = weld(corners, corner, options.limits.max_vertices).map_err(|e| match e {
        WeldError::TooManyVertices => StlError::LimitExceeded {
            resource: ImportResource::Vertices,
            limit: options.limits.max_vertices,
        },
        WeldError::Allocation => StlError::Mesh(MeshError::Allocation {
            context: "vertex welding",
        }),
    })?;
    cancellation.check(Checkpoint::Deduplication)?;
    let mut triangles = Vec::new();
    reserve(&mut triangles, facets.len(), "mesh triangles")?;
    triangles.extend(welded.corner_vertex.as_chunks::<3>().0.iter().copied());
    drop(welded.corner_vertex);
    let mesh = TriangleMesh::try_new_with_cancellation(welded.vertices, triangles, cancellation)?;
    let mut observations = mesh.diagnostics_with_cancellation(cancellation)?;
    // Stored-normal findings per facet as one code byte, computed on all cores: 1 non-finite,
    // 2 zero, 3 opposing the computed normal, 0 none.
    let mut codes = Vec::new();
    reserve(&mut codes, facets.len(), "facet diagnostics")?;
    codes.resize(facets.len(), 0u8);
    crate::parallel::fill_with(&mut codes, |triangle| {
        let supplied = facets[triangle].stored_normal;
        if !supplied.components().iter().all(|v| v.is_finite()) {
            1
        } else if let Some(n) = supplied.unit() {
            match mesh.triangle_normal(triangle) {
                Some(FaceNormal::Unit(g)) if n.x * g.x + n.y * g.y + n.z * g.z < 0.0 => 3,
                _ => 0,
            }
        } else {
            2
        }
    });
    for (triangle, (&code, facet)) in codes.iter().zip(&facets).enumerate() {
        cancellation.poll(Checkpoint::ImportDiagnostics, triangle)?;
        let normal = match code {
            1 => Some(MeshDiagnostic::NonFiniteStoredNormal { triangle }),
            2 => Some(MeshDiagnostic::ZeroStoredNormal { triangle }),
            3 => Some(MeshDiagnostic::OpposingStoredNormal { triangle }),
            _ => None,
        };
        let attribute = facet
            .attribute
            .filter(|&v| v != 0)
            .map(|value| MeshDiagnostic::NonzeroAttribute { triangle, value });
        for diagnostic in [normal, attribute].into_iter().flatten() {
            diagnostics::push(&mut observations, diagnostic)?;
        }
    }
    drop(codes);
    Ok(ImportedStl {
        mesh,
        metadata: StlMetadata {
            header,
            units: options.units,
            facets,
        },
        diagnostics: observations,
        dropped,
    })
}

fn binary(
    bytes: &[u8],
    options: StlOptions,
    cancellation: &mut Cancellation<'_>,
) -> Result<ImportedStl, StlError> {
    cancellation.check(Checkpoint::BinaryParsing)?;
    if bytes.len() < 80 {
        return Err(StlError::Truncated {
            expected: 80,
            actual: bytes.len(),
        });
    }
    if bytes.len() < 84 {
        return Err(StlError::Truncated {
            expected: 84,
            actual: bytes.len(),
        });
    }
    let count = count_at_header(bytes);
    check_limit(
        usize::try_from(count).map_err(|_| StlError::SizeOverflow)?,
        options.limits.max_triangles,
        ImportResource::Triangles,
    )?;
    let expected = binary_size(count)?;
    if bytes.len() < expected {
        return Err(StlError::Truncated {
            expected,
            actual: bytes.len(),
        });
    }
    if bytes.len() > expected {
        return Err(StlError::TrailingBytes {
            expected,
            actual: bytes.len(),
        });
    }
    let mut header = [0; 80];
    header.copy_from_slice(&bytes[..80]);
    let records = bytes[84..].as_chunks::<50>().0;
    let scalar = |record: &[u8; 50], offset: usize| {
        f64::from(f32::from_le_bytes([
            record[offset],
            record[offset + 1],
            record[offset + 2],
            record[offset + 3],
        ]))
    };
    let point = |record: &[u8; 50], offset: usize| {
        Point3::new(
            scalar(record, offset),
            scalar(record, offset + 4),
            scalar(record, offset + 8),
        )
    };
    // The first non-finite coordinate in facet, corner and axis order, searched on all cores.
    let finite_record = |record: &[u8; 50]| {
        (12..48)
            .step_by(4)
            .all(|offset| scalar(record, offset).is_finite())
    };
    cancellation.check(Checkpoint::BinaryParsing)?;
    let first_bad = crate::parallel::first_index(records.len(), |i| !finite_record(&records[i]));
    let first_axis = |record: &[u8; 50]| {
        (0..3).find_map(|corner| {
            non_finite_axis(point(record, 12 + 12 * corner)).map(|axis| (corner, axis))
        })
    };
    let (kept, dropped) = match first_bad {
        None => (None, DroppedFacets::default()),
        Some(index) => kept_records(
            records,
            index,
            options.non_finite,
            &first_axis,
            cancellation,
        )?,
    };
    let source = |facet: usize| kept.as_ref().map_or(facet, |k| k[facet] as usize);
    let count = kept.as_ref().map_or(records.len(), Vec::len);
    // The validated record count sizes the facet buffer exactly.
    let mut facets = Vec::new();
    reserve(&mut facets, count, "facet metadata")?;
    facets.resize(
        count,
        FacetMetadata {
            stored_normal: Vector3::new(0., 0., 0.),
            attribute: None,
        },
    );
    crate::parallel::fill_with(&mut facets, |index| {
        let record = &records[source(index)];
        FacetMetadata {
            stored_normal: Vector3::new(scalar(record, 0), scalar(record, 4), scalar(record, 8)),
            attribute: Some(u16::from_le_bytes([record[48], record[49]])),
        }
    });
    cancellation.check(Checkpoint::BinaryParsing)?;
    // Corner positions are decoded again from the input while welding, never copied.
    assemble(
        options,
        StlHeader::Binary(header),
        facets,
        dropped,
        &|corner| point(&records[source(corner / 3)], 12 + 12 * (corner % 3)),
        cancellation,
    )
}

/// With [`NonFinitePolicy::Drop`], the source records of the kept facets in order and the dropped
/// ones; with [`NonFinitePolicy::Refuse`], the refusal at `first_bad`, the first record with a
/// non-finite corner. `first_axis` gives a record's first non-finite corner and axis.
fn kept_records(
    records: &[[u8; 50]],
    first_bad: usize,
    policy: NonFinitePolicy,
    first_axis: &impl Fn(&[u8; 50]) -> Option<(usize, usize)>,
    cancellation: &mut Cancellation<'_>,
) -> Result<(Option<Vec<u32>>, DroppedFacets), StlError> {
    if policy == NonFinitePolicy::Refuse {
        let (corner, axis) = first_axis(&records[first_bad]).unwrap_or((0, 0));
        return Err(StlError::NonFiniteCoordinate {
            facet: first_bad,
            corner,
            axis,
        });
    }
    let mut dropped = DroppedFacets::default();
    let mut kept = Vec::new();
    reserve(&mut kept, records.len(), "kept facets")?;
    for (index, record) in records.iter().enumerate() {
        cancellation.poll(Checkpoint::BinaryParsing, index)?;
        match first_axis(record) {
            // The validated count is at most `max_triangles`, far below `u32::MAX`.
            None => kept.push(u32::try_from(index).map_err(|_| StlError::SizeOverflow)?),
            Some((corner, axis)) => dropped.add(index, corner, axis)?,
        }
    }
    Ok((Some(kept), dropped))
}

fn label<'a>(text: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = text.strip_prefix(keyword)?;
    if rest.is_empty() || rest.starts_with(char::is_whitespace) {
        Some(rest.trim())
    } else {
        None
    }
}

struct AsciiLines<'a> {
    lines: std::iter::Enumerate<std::str::Lines<'a>>,
    last_line: usize,
}

impl<'a> AsciiLines<'a> {
    fn next(
        &mut self,
        cancellation: &mut Cancellation<'_>,
    ) -> Result<Option<(usize, &'a str)>, StlError> {
        for (index, text) in self.lines.by_ref() {
            cancellation.poll(Checkpoint::AsciiParsing, index)?;
            self.last_line = index + 1;
            let text = text.trim();
            if !text.is_empty() {
                return Ok(Some((index + 1, text)));
            }
        }
        Ok(None)
    }

    fn required(
        &mut self,
        expected: &'static str,
        cancellation: &mut Cancellation<'_>,
    ) -> Result<(usize, &'a str), StlError> {
        self.next(cancellation)?.ok_or(StlError::AsciiSyntax {
            line: self.last_line + 1,
            expected,
        })
    }

    fn keywords(
        &mut self,
        words: &'static str,
        cancellation: &mut Cancellation<'_>,
    ) -> Result<(), StlError> {
        let (line, text) = self.required(words, cancellation)?;
        if text.split_whitespace().eq(words.split_whitespace()) {
            Ok(())
        } else {
            Err(StlError::AsciiSyntax {
                line,
                expected: words,
            })
        }
    }
}

fn number(token: Option<&str>, line: usize, limits: ImportLimits) -> Result<f64, StlError> {
    let token = token.ok_or(StlError::AsciiSyntax {
        line,
        expected: "three numeric components",
    })?;
    check_limit(
        token.len(),
        limits.max_numeric_token_bytes,
        ImportResource::NumericTokenBytes,
    )?;
    let value = token
        .parse::<f64>()
        .map_err(|_| StlError::InvalidNumber { line })?;
    let significand = token.split(['e', 'E']).next().unwrap_or("");
    if value == 0.0 && significand.bytes().any(|b| matches!(b, b'1'..=b'9')) {
        return Err(StlError::InvalidNumber { line }); // Do not silently underflow nonzero decimal coordinates to zero.
    }
    Ok(value)
}

fn vector_record(
    text: &str,
    line: usize,
    prefix: &'static str,
    limits: ImportLimits,
) -> Result<[f64; 3], StlError> {
    let mut tokens = text.split_whitespace();
    for expected in prefix.split_whitespace() {
        if tokens.next() != Some(expected) {
            return Err(StlError::AsciiSyntax {
                line,
                expected: prefix,
            });
        }
    }
    let values = [
        number(tokens.next(), line, limits)?,
        number(tokens.next(), line, limits)?,
        number(tokens.next(), line, limits)?,
    ];
    if tokens.next().is_some() {
        return Err(StlError::AsciiSyntax {
            line,
            expected: "exactly three numeric components",
        });
    }
    Ok(values)
}

fn owned_label(text: &str) -> Result<String, MeshError> {
    let mut result = String::new();
    result
        .try_reserve(text.len())
        .map_err(|_| MeshError::Allocation {
            context: "ASCII solid label",
        })?;
    result.push_str(text);
    Ok(result)
}

fn ascii(
    bytes: &[u8],
    options: StlOptions,
    cancellation: &mut Cancellation<'_>,
) -> Result<ImportedStl, StlError> {
    cancellation.check(Checkpoint::AsciiParsing)?;
    if !text_bytes(bytes, cancellation, Checkpoint::AsciiParsing)? {
        return Err(StlError::AsciiSyntax {
            line: 1,
            expected: "ASCII text without NUL bytes",
        });
    }
    let text = std::str::from_utf8(bytes).map_err(|_| StlError::AsciiSyntax {
        line: 1,
        expected: "ASCII text",
    })?;
    for (index, raw) in text.split('\n').enumerate() {
        cancellation.poll(Checkpoint::AsciiParsing, index)?;
        check_limit(
            raw.len(),
            options.limits.max_ascii_line_bytes,
            ImportResource::AsciiLineBytes,
        )?;
    }
    let mut lines = AsciiLines {
        lines: text.lines().enumerate(),
        last_line: 0,
    };
    let (line, opener) = lines.required("solid [name]", cancellation)?;
    let name = owned_label(label(opener, "solid").ok_or(StlError::AsciiSyntax {
        line,
        expected: "solid [name]",
    })?)?;
    let mut builder = Builder::new(options);
    loop {
        let (line, record) = lines.required("facet normal or endsolid [name]", cancellation)?;
        if let Some(end_name) = label(record, "endsolid") {
            let end_name = owned_label(end_name)?;
            if let Some((line, _)) = lines.next(cancellation)? {
                return Err(StlError::AsciiSyntax {
                    line,
                    expected: "end of input after endsolid",
                });
            }
            return builder.finish(StlHeader::Ascii { name, end_name }, cancellation);
        }
        let [x, y, z] = vector_record(record, line, "facet normal", options.limits)?;
        let stored_normal = Vector3::new(x, y, z);
        lines.keywords("outer loop", cancellation)?;
        let mut points = [Point3::new(0.0, 0.0, 0.0); 3];
        for point in &mut points {
            let (line, record) = lines.required("vertex x y z", cancellation)?;
            let [x, y, z] = vector_record(record, line, "vertex", options.limits)?;
            *point = Point3::new(x, y, z);
        }
        lines.keywords("endloop", cancellation)?;
        lines.keywords("endfacet", cancellation)?;
        builder.facet(
            points,
            FacetMetadata {
                stored_normal,
                attribute: None,
            },
            cancellation,
        )?;
    }
}
