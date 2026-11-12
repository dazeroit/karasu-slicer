//! Wavefront OBJ polygon geometry (TD-057).
//!
//! **Geometry:**
//! - `v x y z` (an optional fourth `w` must be 1; six values carry a colour, which is ignored)
//!   and `f` with any of the `v`, `v/vt`, `v/vt/vn` and `v//vn` forms.
//! - Indices are 1-based; negative indices count back from the last vertex read. Positive
//!   indices may refer to vertices defined later.
//!
//! **Ignored and counted:** texture coordinates (`vt`), normals (`vn`), parameter vertices
//! (`vp`), groups and objects (`o`, `g`), smoothing (`s`), materials (`usemtl`, `mtllib`; no
//! material file is opened), and lines and points (`l`, `p`). All objects form one mesh.
//!
//! **Errors:** free-form curves and surfaces would silently lose geometry, so they are errors, as
//! are unknown statements.
//!
//! **Syntax:**
//! - `#` starts a comment; a trailing `\` continues a line.
//! - CRLF and trailing NUL padding are accepted. The text must be UTF-8.
//!
//! **Triangulation:**
//! - Polygons with more than three vertices are split by ear clipping in the plane of their
//!   Newell normal.
//! - It tries the ear at the second remaining vertex first, so a convex polygon gives the
//!   common fan `(0, i, i + 1)`, and concave polygons are split correctly.
//! - A polygon without a valid ear (self-intersecting in projection) is an error, naming its line.
//!
//! **Units:** OBJ declares no unit, so one must be assumed.

use super::{
    ImportNote, ModelFormat, ModelImportError, ModelImportOptions, ModelLimits, SourceModel,
    WeldingBuilder,
};
use crate::cancellation::{Cancellation, Checkpoint};
use crate::geometry::Point3;
use crate::mesh::ImportResource;
use std::{borrow::Cow, collections::BTreeMap, error::Error, fmt};

/// Why one OBJ statement is invalid.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ObjProblem {
    /// An unknown statement, or free-form geometry this importer does not evaluate.
    Unsupported(String),
    /// A vertex with a number of values other than 3, 4 (`w`) or 6 (colour).
    VertexArity(usize),
    /// A vertex weight `w` other than 1 (rational geometry).
    RationalWeight,
    /// A malformed number.
    InvalidNumber,
    /// A NaN or infinite coordinate.
    NonFinite,
    /// A face with fewer than three vertices.
    FaceArity(usize),
    /// A malformed or zero vertex index.
    InvalidIndex,
    /// An index outside the vertices of the file.
    IndexOutOfRange,
    /// A polygon with more vertices than the limit.
    PolygonTooLarge {
        /// The limit.
        limit: usize,
    },
    /// A polygon that cannot be split into triangles (self-intersecting in projection).
    Untriangulable,
}

/// Why an OBJ file cannot be read.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ObjError {
    /// The text is not UTF-8.
    NotUtf8 {
        /// 1-based line.
        line: usize,
    },
    /// A line (after joining continuations) is longer than the limit.
    LineTooLong {
        /// 1-based line.
        line: usize,
        /// The limit in bytes.
        limit: usize,
    },
    /// An invalid statement.
    Statement {
        /// 1-based line where the statement starts.
        line: usize,
        /// What is wrong.
        problem: ObjProblem,
    },
    /// The file holds no face.
    NoFaces,
}

impl fmt::Display for ObjError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotUtf8 { line } => write!(f, "OBJ line {line} is not UTF-8"),
            Self::LineTooLong { line, limit } => {
                write!(f, "OBJ line {line} is longer than {limit} bytes")
            }
            Self::Statement { line, problem } => {
                write!(f, "OBJ line {line}: ")?;
                match problem {
                    ObjProblem::Unsupported(keyword) => {
                        write!(f, "unsupported statement '{keyword}'")
                    }
                    ObjProblem::VertexArity(n) => write!(
                        f,
                        "a vertex needs 3 coordinates (optionally w, or an RGB colour), not {n} values"
                    ),
                    ObjProblem::RationalWeight => {
                        f.write_str("vertex weight w other than 1 (rational geometry)")
                    }
                    ObjProblem::InvalidNumber => f.write_str("malformed number"),
                    ObjProblem::NonFinite => f.write_str("coordinate is not finite"),
                    ObjProblem::FaceArity(n) => {
                        write!(f, "a face needs at least 3 vertices, not {n}")
                    }
                    ObjProblem::InvalidIndex => f.write_str("malformed or zero vertex index"),
                    ObjProblem::IndexOutOfRange => f.write_str("vertex index out of range"),
                    ObjProblem::PolygonTooLarge { limit } => {
                        write!(f, "polygon has more than {limit} vertices")
                    }
                    ObjProblem::Untriangulable => {
                        f.write_str("polygon cannot be split into triangles (self-intersecting)")
                    }
                }
            }
            Self::NoFaces => f.write_str("OBJ file has no faces"),
        }
    }
}

impl Error for ObjError {}

/// The Wavefront OBJ format.
#[derive(Clone, Copy, Debug, Default)]
pub struct ObjFormat;

impl ModelFormat for ObjFormat {
    fn id(&self) -> &'static str {
        "obj"
    }
    fn name(&self) -> &'static str {
        "OBJ"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["obj"]
    }
    fn declares_units(&self) -> bool {
        false
    }
    fn parse(
        &self,
        bytes: &[u8],
        options: &ModelImportOptions,
        cancellation: &mut Cancellation<'_>,
    ) -> Result<SourceModel, ModelImportError> {
        parse(bytes, &options.limits, cancellation)
    }
}

fn statement(line: usize, problem: ObjProblem) -> ModelImportError {
    ModelImportError::Obj(ObjError::Statement { line, problem })
}

/// Logical lines: continuations joined, comments kept (stripped by the caller), 1-based start line.
/// Logical lines of `text`: continuations joined, comments kept (stripped by the caller), each
/// with its 1-based start line counted from `first_line + 1`. Lines without continuation borrow
/// the text.
fn logical_lines(
    text: &[u8],
    first_line: usize,
    max_line: usize,
) -> impl Iterator<Item = Result<(usize, Cow<'_, str>), ModelImportError>> + '_ {
    let mut physical = text.split(|&b| b == b'\n').enumerate().peekable();
    std::iter::from_fn(move || {
        let (index, first) = physical.next()?;
        let start = first_line + index + 1;
        let too_long = || {
            Some(Err(ModelImportError::Obj(ObjError::LineTooLong {
                line: start,
                limit: max_line,
            })))
        };
        let trimmed = first.strip_suffix(b"\r").unwrap_or(first);
        let text = match trimmed.strip_suffix(b"\\") {
            Some(_) if physical.peek().is_some() => {
                let mut joined: Vec<u8> = Vec::new();
                let mut current = trimmed;
                loop {
                    if joined.len() + current.len() > max_line {
                        return too_long();
                    }
                    match current.strip_suffix(b"\\") {
                        Some(head) if physical.peek().is_some() => {
                            joined.extend_from_slice(head);
                            joined.push(b' ');
                            let next = physical.next().map_or(&[][..], |(_, l)| l);
                            current = next.strip_suffix(b"\r").unwrap_or(next);
                        }
                        _ => {
                            joined.extend_from_slice(current);
                            break;
                        }
                    }
                }
                String::from_utf8(joined).map(Cow::Owned).map_err(|_| ())
            }
            _ => {
                if trimmed.len() > max_line {
                    return too_long();
                }
                std::str::from_utf8(trimmed)
                    .map(Cow::Borrowed)
                    .map_err(|_| ())
            }
        };
        Some(
            text.map(|text| (start, text))
                .map_err(|()| ModelImportError::Obj(ObjError::NotUtf8 { line: start })),
        )
    })
}

fn number(token: &str, line: usize, limits: &ModelLimits) -> Result<f64, ModelImportError> {
    if token.len() > limits.input.max_numeric_token_bytes {
        return Err(ModelImportError::LimitExceeded {
            resource: ImportResource::NumericTokenBytes,
            limit: limits.input.max_numeric_token_bytes,
        });
    }
    // Rust also parses "inf" and "nan": only plain decimal notation is accepted.
    if !token
        .bytes()
        .all(|b| b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.' | b'e' | b'E'))
    {
        return Err(statement(line, ObjProblem::InvalidNumber));
    }
    let value: f64 = token
        .parse()
        .map_err(|_| statement(line, ObjProblem::InvalidNumber))?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(statement(line, ObjProblem::NonFinite))
    }
}

/// Statements ignored because they carry no solid geometry.
const IGNORED: [&str; 10] = [
    "vt", "vn", "vp", "o", "g", "s", "usemtl", "mtllib", "l", "p",
];
/// Free-form geometry statements: refused rather than silently dropped.
const FREE_FORM: [&str; 16] = [
    "cstype", "deg", "bmat", "step", "curv", "curv2", "surf", "parm", "trim", "hole", "scrv", "sp",
    "end", "con", "mg", "bevel",
];

/// One parsed face: its source line and the range of its corner indices.
struct Face {
    line: usize,
    start: usize,
    end: usize,
}

/// The statements of one chunk of lines, parsed independently of the others.
#[derive(Default)]
struct Chunk {
    positions: Vec<Point3>,
    /// Corner indices: positive 1-based file indices, or negative ones with the number of this
    /// chunk's vertices read before their line, resolved once earlier chunks are counted.
    corners: Vec<(i64, u32)>,
    faces: Vec<Face>,
    ignored: BTreeMap<String, usize>,
    colours: usize,
    /// The first error of the chunk and its line; parsing of the chunk stops there.
    error: Option<(usize, ModelImportError)>,
}

/// Chunk starts at line boundaries that do not split a continued line.
fn chunk_starts(text: &[u8], chunks: usize) -> Vec<usize> {
    let mut starts = vec![0];
    for k in 1..chunks {
        let mut at = (text.len() * k / chunks).max(*starts.last().unwrap_or(&0));
        loop {
            let Some(newline) = text[at..].iter().position(|&b| b == b'\n') else {
                at = text.len();
                break;
            };
            let end = at + newline;
            let line = &text[..end];
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            at = end + 1;
            if !line.ends_with(b"\\") {
                break;
            }
        }
        if at < text.len() && at > *starts.last().unwrap_or(&0) {
            starts.push(at);
        }
    }
    starts
}

/// Parse the lines of one chunk, which starts after `first_line` lines.
fn parse_chunk(text: &[u8], first_line: usize, limits: &ModelLimits) -> Chunk {
    let mut chunk = Chunk::default();
    let mut current = first_line;
    let result = (|| -> Result<(), ModelImportError> {
        for line in logical_lines(text, first_line, limits.input.max_ascii_line_bytes) {
            let (line, content) = line?;
            current = line;
            parse_statement(&mut chunk, line, &content, limits)?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        // Line errors carry their own line; others belong to the last line read.
        let line = match &error {
            ModelImportError::Obj(
                ObjError::Statement { line, .. }
                | ObjError::LineTooLong { line, .. }
                | ObjError::NotUtf8 { line },
            ) => *line,
            _ => current,
        };
        chunk.error = Some((line, error));
    }
    chunk
}

fn allocation(context: &'static str) -> ModelImportError {
    ModelImportError::Mesh(crate::mesh::MeshError::Allocation { context })
}

/// Parse one logical line into `chunk`.
fn parse_statement(
    chunk: &mut Chunk,
    line: usize,
    content: &str,
    limits: &ModelLimits,
) -> Result<(), ModelImportError> {
    let content = content.split('#').next().unwrap_or("");
    let mut tokens = content.split_ascii_whitespace();
    let Some(keyword) = tokens.next() else {
        return Ok(());
    };
    match keyword {
        "v" => {
            let mut values = [""; 7];
            let mut count = 0;
            for token in tokens {
                if count < values.len() {
                    values[count] = token;
                }
                count += 1;
            }
            match count {
                3 | 6 => {}
                4 => {
                    // Exactly one: any other weight belongs to rational geometry.
                    #[allow(clippy::float_cmp)]
                    if number(values[3], line, limits)? != 1.0 {
                        return Err(statement(line, ObjProblem::RationalWeight));
                    }
                }
                n => return Err(statement(line, ObjProblem::VertexArity(n))),
            }
            if count == 6 {
                for value in &values[3..6] {
                    number(value, line, limits)?;
                }
                chunk.colours += 1;
            }
            let [x, y, z] = [0, 1, 2].map(|i| number(values[i], line, limits));
            if chunk.positions.len() >= limits.input.max_vertices {
                return Err(ModelImportError::LimitExceeded {
                    resource: ImportResource::Vertices,
                    limit: limits.input.max_vertices,
                });
            }
            chunk
                .positions
                .try_reserve(1)
                .map_err(|_| allocation("OBJ vertices"))?;
            chunk.positions.push(Point3::new(x?, y?, z?));
        }
        "f" => {
            let start = chunk.corners.len();
            let local =
                u32::try_from(chunk.positions.len()).map_err(|_| allocation("OBJ index"))?;
            for token in tokens {
                let index = token.split('/').next().unwrap_or("");
                let value: i64 = index
                    .parse()
                    .ok()
                    .filter(|&v| v != 0)
                    .ok_or_else(|| statement(line, ObjProblem::InvalidIndex))?;
                chunk
                    .corners
                    .try_reserve(1)
                    .map_err(|_| allocation("OBJ face corners"))?;
                chunk.corners.push((value, local));
            }
            let n = chunk.corners.len() - start;
            if n < 3 {
                return Err(statement(line, ObjProblem::FaceArity(n)));
            }
            if n > limits.max_polygon_vertices {
                return Err(statement(
                    line,
                    ObjProblem::PolygonTooLarge {
                        limit: limits.max_polygon_vertices,
                    },
                ));
            }
            chunk
                .faces
                .try_reserve(1)
                .map_err(|_| allocation("OBJ faces"))?;
            chunk.faces.push(Face {
                line,
                start,
                end: chunk.corners.len(),
            });
        }
        keyword if IGNORED.contains(&keyword) => {
            *chunk.ignored.entry(keyword.to_owned()).or_default() += 1;
        }
        keyword if FREE_FORM.contains(&keyword) => {
            return Err(statement(
                line,
                ObjProblem::Unsupported(format!("{keyword} (free-form geometry)")),
            ));
        }
        keyword => return Err(statement(line, ObjProblem::Unsupported(keyword.into()))),
    }
    Ok(())
}

/// Run `work(i)` for `0..count` on scoped threads, keeping results in order.
fn in_parallel<T: Send>(count: usize, work: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let work = &work;
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..count).map(|i| scope.spawn(move || work(i))).collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_else(|p| std::panic::resume_unwind(p)))
            .collect()
    })
}

/// Parse, resolve and triangulate on every available core (TD-058); results equal a sequential
/// pass, including which error is reported: the one on the earliest line.
#[allow(clippy::too_many_lines)] // Chunked parsing, merging, then chunked triangulation.
fn parse(
    bytes: &[u8],
    limits: &ModelLimits,
    cancellation: &mut Cancellation<'_>,
) -> Result<SourceModel, ModelImportError> {
    cancellation.check(Checkpoint::Detection)?;
    // Some exporters pad files with NUL bytes.
    let end = bytes.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
    let text = &bytes[..end];
    let threads = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(text.len() / (1 << 20))
        .max(1);
    let mut starts = chunk_starts(text, threads);
    starts.push(text.len());
    let ranges: Vec<(usize, usize)> = starts.windows(2).map(|w| (w[0], w[1])).collect();
    // Lines before each chunk.
    let newlines = in_parallel(ranges.len(), |k| {
        let (a, b) = ranges[k];
        // A plain linear count; one pass per chunk, in parallel.
        #[allow(clippy::naive_bytecount)]
        let count = text[a..b].iter().filter(|&&c| c == b'\n').count();
        count
    });
    let mut first_lines = Vec::with_capacity(ranges.len());
    let mut lines = 0;
    for count in &newlines {
        first_lines.push(lines);
        lines += count;
    }
    cancellation.check(Checkpoint::AsciiParsing)?;
    let chunks = in_parallel(ranges.len(), |k| {
        let (a, b) = ranges[k];
        parse_chunk(&text[a..b], first_lines[k], limits)
    });
    cancellation.check(Checkpoint::AsciiParsing)?;
    // Resolve negative indices against the vertices of earlier chunks, and report the error on
    // the earliest line: a chunk's own error, or a negative index out of range.
    let mut offset = 0i64;
    let mut first_error: Option<(usize, ModelImportError)> = None;
    let keep =
        |line: usize, error: ModelImportError, slot: &mut Option<(usize, ModelImportError)>| {
            if slot.as_ref().is_none_or(|(l, _)| line < *l) {
                *slot = Some((line, error));
            }
        };
    let mut resolved: Vec<i64> = Vec::new();
    let total_corners: usize = chunks.iter().map(|c| c.corners.len()).sum();
    resolved
        .try_reserve_exact(total_corners)
        .map_err(|_| allocation("OBJ face corners"))?;
    let mut faces: Vec<Face> = Vec::new();
    let total_faces: usize = chunks.iter().map(|c| c.faces.len()).sum();
    faces
        .try_reserve_exact(total_faces)
        .map_err(|_| allocation("OBJ faces"))?;
    for chunk in &chunks {
        let base = resolved.len();
        for face in &chunk.faces {
            for &(value, local) in &chunk.corners[face.start..face.end] {
                let absolute = if value < 0 {
                    // Relative to the vertices read before this line, across all chunks.
                    let result = offset + i64::from(local) + value;
                    if result < 0 {
                        keep(
                            face.line,
                            statement(face.line, ObjProblem::IndexOutOfRange),
                            &mut first_error,
                        );
                    }
                    result + 1
                } else {
                    value
                };
                resolved.push(absolute);
            }
            faces.push(Face {
                line: face.line,
                start: base + face.start,
                end: base + face.end,
            });
        }
        offset += i64::try_from(chunk.positions.len()).unwrap_or(i64::MAX);
    }
    let mut ignored: BTreeMap<String, usize> = BTreeMap::new();
    let mut colours = 0;
    let mut positions: Vec<Point3> = Vec::new();
    let total_positions: usize = chunks.iter().map(|c| c.positions.len()).sum();
    positions
        .try_reserve_exact(total_positions)
        .map_err(|_| allocation("OBJ vertices"))?;
    for chunk in chunks {
        if let Some((line, error)) = chunk.error {
            keep(line, error, &mut first_error);
        }
        for (keyword, count) in chunk.ignored {
            *ignored.entry(keyword).or_default() += count;
        }
        colours += chunk.colours;
        positions.extend(chunk.positions);
    }
    if let Some((_, error)) = first_error {
        return Err(error);
    }
    if positions.len() > limits.input.max_vertices {
        return Err(ModelImportError::LimitExceeded {
            resource: ImportResource::Vertices,
            limit: limits.input.max_vertices,
        });
    }
    if faces.is_empty() {
        return Err(ModelImportError::Obj(ObjError::NoFaces));
    }
    // Triangulate groups of faces in parallel; the earliest failing face is reported.
    let groups = threads.min(faces.len());
    let per = faces.len().div_ceil(groups);
    let triangulated = in_parallel(groups, |g| {
        let mut corners: Vec<Point3> = Vec::new();
        let (mut polygons, mut split) = (0usize, 0usize);
        let mut polygon: Vec<Point3> = Vec::new();
        let mut triangles: Vec<[usize; 3]> = Vec::new();
        for face in &faces[g * per..((g + 1) * per).min(faces.len())] {
            polygon.clear();
            for &index in &resolved[face.start..face.end] {
                let Some(position) = usize::try_from(index - 1)
                    .ok()
                    .and_then(|i| positions.get(i))
                else {
                    return Err(statement(face.line, ObjProblem::IndexOutOfRange));
                };
                polygon.push(*position);
            }
            triangles.clear();
            triangulate(&polygon, &mut triangles)
                .map_err(|()| statement(face.line, ObjProblem::Untriangulable))?;
            if polygon.len() > 3 {
                polygons += 1;
                split += triangles.len();
            }
            corners
                .try_reserve(triangles.len() * 3)
                .map_err(|_| allocation("OBJ triangles"))?;
            corners.extend(triangles.iter().flat_map(|t| t.map(|i| polygon[i])));
        }
        Ok((corners, polygons, split))
    });
    drop(positions);
    drop(resolved);
    cancellation.check(Checkpoint::Deduplication)?;
    let mut builder = WeldingBuilder::new(limits.input);
    let (mut polygons, mut split) = (0usize, 0usize);
    for group in triangulated {
        let (corners, p, t) = group?;
        polygons += p;
        split += t;
        builder.extend(corners)?;
    }
    let (mesh, diagnostics) = builder.finish(cancellation)?;
    let mut notes = Vec::new();
    if polygons > 0 {
        notes.push(ImportNote::PolygonsTriangulated {
            polygons,
            triangles: split,
        });
    }
    if colours > 0 {
        notes.push(ImportNote::VertexColorsIgnored { count: colours });
    }
    for (keyword, count) in ignored {
        notes.push(ImportNote::StatementsIgnored { keyword, count });
    }
    Ok(SourceModel {
        mesh,
        declared_unit: None,
        objects: 1,
        notes,
        diagnostics,
    })
}

/// Split a polygon into triangles of corner positions, preserving its winding.
///
/// Ear clipping in the plane of the Newell normal, trying the ear at the second remaining corner
/// first: convex polygons give the fan `(0, i, i + 1)`. Polygons whose corners are all collinear
/// (or coincident) get the fan; their triangles are degenerate and reported by mesh diagnostics.
/// `Err` when no ear remains, or when the normal vanishes for a non-collinear outline: both mean
/// the polygon intersects itself in projection.
// Geometric notation (corners a, b, c; axes u, v); exact equality detects coincident corners.
#[allow(clippy::many_single_char_names, clippy::float_cmp)]
pub(crate) fn triangulate(polygon: &[Point3], out: &mut Vec<[usize; 3]>) -> Result<(), ()> {
    let n = polygon.len();
    if n == 3 {
        out.push([0, 1, 2]);
        return Ok(());
    }
    let mut normal = [0.0f64; 3];
    for i in 0..n {
        let (a, b) = (polygon[i], polygon[(i + 1) % n]);
        normal[0] += (a.y - b.y) * (a.z + b.z);
        normal[1] += (a.z - b.z) * (a.x + b.x);
        normal[2] += (a.x - b.x) * (a.y + b.y);
    }
    let axis = (0..3)
        .max_by(|&i, &j| normal[i].abs().total_cmp(&normal[j].abs()))
        .unwrap_or(2);
    if normal[axis] == 0.0 || !normal[axis].is_finite() {
        // Only corners on one line (or coincident) get the degenerate fan; a zero normal with
        // extent in two directions is a self-cancelling (self-intersecting) outline.
        let origin = polygon[0];
        let collinear = polygon.windows(2).all(|pair| {
            let (a, b) = (pair[0], pair[1]);
            let u = [a.x - origin.x, a.y - origin.y, a.z - origin.z];
            let w = [b.x - origin.x, b.y - origin.y, b.z - origin.z];
            u[1] * w[2] - u[2] * w[1] == 0.0
                && u[2] * w[0] - u[0] * w[2] == 0.0
                && u[0] * w[1] - u[1] * w[0] == 0.0
        });
        if !collinear {
            return Err(());
        }
        out.extend((1..n - 1).map(|i| [0, i, i + 1]));
        return Ok(());
    }
    // Drop the dominant axis; order the remaining two so the polygon runs counter-clockwise.
    let (u, v) = match axis {
        0 => (1, 2),
        1 => (2, 0),
        _ => (0, 1),
    };
    let flip = normal[axis] < 0.0;
    let flat: Vec<[f64; 2]> = polygon
        .iter()
        .map(|p| {
            let c = [p.x, p.y, p.z];
            if flip {
                [c[v], c[u]]
            } else {
                [c[u], c[v]]
            }
        })
        .collect();
    let cross = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    };
    let mut remaining: Vec<usize> = (0..n).collect();
    while remaining.len() > 3 {
        let m = remaining.len();
        let ear = (1..=m).find(|&k| {
            let (a, b, c) = (remaining[k - 1], remaining[k % m], remaining[(k + 1) % m]);
            let (pa, pb, pc) = (flat[a], flat[b], flat[c]);
            if cross(pa, pb, pc) <= 0.0 {
                return false;
            }
            // No other remaining corner inside or on the candidate triangle.
            remaining.iter().all(|&o| {
                let p = flat[o];
                o == a
                    || o == b
                    || o == c
                    || p == pa
                    || p == pb
                    || p == pc
                    || cross(pa, pb, p) < 0.0
                    || cross(pb, pc, p) < 0.0
                    || cross(pc, pa, p) < 0.0
            })
        });
        let Some(k) = ear else {
            return Err(());
        };
        out.push([remaining[k - 1], remaining[k % m], remaining[(k + 1) % m]]);
        remaining.remove(k % m);
    }
    out.push([remaining[0], remaining[1], remaining[2]]);
    Ok(())
}
