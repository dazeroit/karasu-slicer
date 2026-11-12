//! Model files: the models of a plate, with their supports and raft, as binary STL, 3MF or OBJ
//! (TD-139).
//!
//! **Content.** A file holds bodies: closed outward triangle meshes in world millimetres, each
//! moved by its instance's offset, each named and of a kind (a model, its supports, its raft).
//! STL and OBJ hold one solid, the bodies concatenated (overlapping closed shells, united by
//! positive winding as slicers read them). 3MF holds each body as an object of its own, of type
//! `model` so a slicer reading it prints it, and a build item for each, unit millimetre.
//!
//! **Precision.** Every format stores each coordinate as the nearest `f32` ([`stored`]): binary
//! STL can hold nothing else, and slicers read 3MF and OBJ coordinates as `f32`. A drilled model
//! lies on the Boolean kernel's grid, whose points within ±512 mm are `f32` values
//! ([`crate::preparation::sampled::DrilledModel`], TD-140 phase 5): every format stores it
//! exactly, as long as its instance's offset keeps it on the grid. In 3MF and OBJ a coordinate is
//! written as the shortest decimal of that `f32` taken as an `f64`: a reader parsing it as `f32`
//! and one parsing it as `f64` both get exactly the stored value (the shortest decimal of the
//! `f32` itself would read as another `f64`). Negative zero is written as zero.
//!
//! **Determinism.** The same bodies give the same bytes: no time stamp, a fixed STL header, a
//! fixed member order and fixed DOS times in the 3MF package (its ZIP writer, deflated, TD-133).
//!
//! **Publication.** A file is written to a hidden staging file beside the destination and
//! published whole, never replacing an existing file, exactly as print files are: a failure or a
//! cancellation leaves the destination as it was and removes the staging file.
use super::{io_error, publish_file, stage_path, ExportError, Stage};
use crate::{
    cancellation::{Cancellation, Cancelled, Checkpoint},
    geometry::Point3,
    mesh::TriangleMesh,
    print_job::{
        machine::{CodecError, EncoderLimits},
        zip::{ClassicZip, Member},
    },
};
use std::{
    error::Error,
    fmt::{self, Write as _},
    fs::{self, OpenOptions},
    io::{self, BufWriter, Write},
    path::Path,
};

/// The file formats a model is exported as.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ModelFileFormat {
    /// Binary STL: one solid, every body's triangles.
    Stl,
    /// 3MF: each body an object and a build item.
    ThreeMf,
    /// Wavefront OBJ: one solid, each body an `o` group.
    Obj,
}

impl ModelFileFormat {
    /// Every format, in the order the interface offers them.
    pub const ALL: [Self; 3] = [Self::Stl, Self::ThreeMf, Self::Obj];

    /// The file extension, without the dot.
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Stl => "stl",
            Self::ThreeMf => "3mf",
            Self::Obj => "obj",
        }
    }

    /// The format whose extension `extension` is (any letter case).
    #[must_use]
    pub fn from_extension(extension: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|f| f.extension().eq_ignore_ascii_case(extension))
    }
}

/// What a body is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BodyKind {
    /// A model as it stands on the plate (hollowed and drilled when it is).
    Model,
    /// A model's supports, braces included.
    Supports,
    /// A model's raft.
    Raft,
}

/// One body of a model file: a closed outward mesh in world millimetres, moved by `offset`.
#[derive(Clone, Copy, Debug)]
pub struct ModelBody<'a> {
    /// The name 3MF and OBJ give the body.
    pub name: &'a str,
    /// What the body is.
    pub kind: BodyKind,
    /// Its triangles, facing out.
    pub mesh: &'a TriangleMesh,
    /// The move from the mesh's coordinates to where the body is written, in mm (an instance's
    /// place on the plate).
    pub offset: [f64; 3],
}

/// What a model file holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelFileSummary {
    /// Bodies written.
    pub bodies: usize,
    /// Triangles written.
    pub triangles: usize,
    /// Vertices written (in STL, three per triangle).
    pub vertices: usize,
    /// The file's size in bytes.
    pub bytes: u64,
    /// 3MF only: edges still used by more than two triangles, by vertex index, where sheets
    /// meet that neither way of pairing their faces separates (Core §4.1 requires none).
    pub shared_edges: usize,
}

/// Why a model file was not written. The destination is then as it was.
#[derive(Debug)]
pub enum ModelFileError {
    /// The destination exists: model files never replace one.
    Exists,
    /// The destination's extension is not the format's.
    Extension(ModelFileFormat),
    /// Nothing to write.
    Empty,
    /// A coordinate the format cannot hold: not finite, or beyond `f32`.
    Coordinate {
        /// The body's name.
        body: String,
        /// The vertex's index in the body's mesh.
        vertex: usize,
    },
    /// More triangles or vertices than the format can count, or a 3MF package beyond 4 GiB.
    TooLarge(&'static str),
    /// A file system failure, with the operation it stopped.
    Io {
        /// What was being done.
        operation: &'static str,
        /// The failure.
        source: io::Error,
    },
    /// Cancelled; nothing was published.
    Cancelled(Cancelled),
}

impl fmt::Display for ModelFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exists => f.write_str("the file exists; a model export never replaces a file"),
            Self::Extension(format) => write!(
                f,
                "the file name must end in .{} for this format",
                format.extension()
            ),
            Self::Empty => f.write_str("there is nothing to write"),
            Self::Coordinate { body, vertex } => write!(
                f,
                "{body}: vertex {vertex} has a coordinate a model file cannot hold"
            ),
            Self::TooLarge(what) => write!(f, "the file would be too large: {what}"),
            Self::Io { operation, source } => write!(f, "{operation}: {source}"),
            Self::Cancelled(cancelled) => cancelled.fmt(f),
        }
    }
}

impl Error for ModelFileError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Cancelled(cancelled) => Some(cancelled),
            _ => None,
        }
    }
}

impl From<Cancelled> for ModelFileError {
    fn from(cancelled: Cancelled) -> Self {
        Self::Cancelled(cancelled)
    }
}

impl From<ExportError> for ModelFileError {
    fn from(error: ExportError) -> Self {
        match error {
            ExportError::Io { operation, source } => Self::Io { operation, source },
            ExportError::Cancelled(cancelled) => Self::Cancelled(cancelled),
            other => Self::Io {
                operation: "publish the model file",
                source: io::Error::other(other.to_string()),
            },
        }
    }
}

impl From<CodecError> for ModelFileError {
    fn from(error: CodecError) -> Self {
        match error {
            CodecError::Io(source) => Self::Io {
                operation: "write the 3MF package",
                source,
            },
            CodecError::Cancelled(cancelled) => Self::Cancelled(cancelled),
            CodecError::Limit(what) | CodecError::Invalid(what) | CodecError::Unsupported(what) => {
                Self::TooLarge(what)
            }
        }
    }
}

/// A body of overlapping closed shells made fit for a file ([`tidy_shells`]), and what was
/// taken out.
#[derive(Debug)]
pub struct Tidied {
    /// The shells, each once, their vertices shared by position.
    pub mesh: TriangleMesh,
    /// Shells left out because an identical one (the same triangles at the same points) was
    /// kept.
    pub repeated: usize,
    /// Pairs of faces left out because they coincide with opposite facings (two shells' caps
    /// back to back).
    pub cancelled: usize,
}

/// `mesh`, a body of closed shells (each its own vertices, such as a model's supports, TD-062),
/// made fit for a file: a shell identical to one before it is written once, the vertices are
/// shared by position, and two faces at the same three points facing opposite ways are both left
/// out. Points are compared as the file stores them, as `f32`; points equal there become the
/// first of them. Nothing the shells enclose changes: a repeated shell adds nothing to their union, and
/// two opposite faces at one place are a wall of no thickness between two shells (consecutive
/// shaft segments' caps), whose union is the same without it. The result stays closed: a face
/// pair left out shares its edges with each other only. Deterministic: the first of identical
/// shells is kept, in triangle order.
///
/// Slicing reads supports by positive winding and needs none of this; a file read by another
/// program does: repeated shells and caps back to back read as duplicate faces and as edges of
/// four faces once the program welds the vertices.
///
/// # Panics
/// Never: the result is a subset of `mesh`'s triangles on some of its points.
#[must_use]
pub fn tidy_shells(mesh: &TriangleMesh) -> Tidied {
    use std::collections::{HashMap, HashSet};
    let triangles = mesh.triangles();
    // Points compared as a file stores them (`f32`): a joint reached by two routes lands a few
    // units in the last place of `f64` apart, and a reader sees one point.
    let bits = |v: u32| {
        let p = mesh.vertices()[v as usize];
        #[allow(clippy::cast_possible_truncation)] // The file's precision.
        [p.x, p.y, p.z].map(|c| u64::from(((c as f32) + 0.0).to_bits()))
    };
    let shells = shells_of(mesh);
    // A shell's points, each face turned to start at its least corner, in order: equal for
    // identical shells.
    let key = |faces: &[usize]| {
        let mut out: Vec<[[u64; 3]; 3]> = faces
            .iter()
            .map(|&i| {
                let c = triangles[i].map(bits);
                let start = (0..3).min_by_key(|&k| c[k]).unwrap_or(0);
                [c[start], c[(start + 1) % 3], c[(start + 2) % 3]]
            })
            .collect();
        out.sort_unstable();
        out
    };
    let mut seen: HashSet<Vec<[[u64; 3]; 3]>> = HashSet::new();
    let mut repeated = 0;
    let mut kept: Vec<usize> = Vec::with_capacity(triangles.len());
    for faces in &shells {
        if seen.insert(key(faces)) {
            kept.extend(faces);
        } else {
            repeated += 1;
        }
    }
    kept.sort_unstable();
    // Vertices shared by position.
    let mut index: HashMap<[u64; 3], u32> = HashMap::new();
    let mut vertices: Vec<Point3> = Vec::new();
    let faces: Vec<[u32; 3]> = kept
        .iter()
        .map(|&i| {
            triangles[i].map(|v| {
                *index.entry(bits(v)).or_insert_with(|| {
                    vertices.push(mesh.vertices()[v as usize]);
                    u32::try_from(vertices.len() - 1).unwrap_or(u32::MAX)
                })
            })
        })
        .collect();
    // Faces at the same three points facing opposite ways, paired off in order.
    let turned = |t: [u32; 3]| {
        let start = (0..3).min_by_key(|&k| t[k]).unwrap_or(0);
        [t[start], t[(start + 1) % 3], t[(start + 2) % 3]]
    };
    let mut waiting: HashMap<[u32; 3], Vec<usize>> = HashMap::new();
    let mut gone = vec![false; faces.len()];
    let mut cancelled = 0;
    for (i, &t) in faces.iter().enumerate() {
        let reverse = turned([t[0], t[2], t[1]]);
        if let Some(j) = waiting.get_mut(&reverse).and_then(Vec::pop) {
            gone[i] = true;
            gone[j] = true;
            cancelled += 1;
        } else {
            waiting.entry(turned(t)).or_default().push(i);
        }
    }
    let faces: Vec<[u32; 3]> = faces
        .into_iter()
        .zip(&gone)
        .filter(|(_, &g)| !g)
        .map(|(t, _)| t)
        .collect();
    let mesh = TriangleMesh::try_new(vertices, faces)
        .expect("a subset of a valid mesh's triangles on its own points is valid");
    Tidied {
        mesh,
        repeated,
        cancelled,
    }
}

/// `mesh`'s shells: its triangles connected through shared vertex indices, each in triangle
/// order, the shells in the order of their first triangles.
fn shells_of(mesh: &TriangleMesh) -> Vec<Vec<usize>> {
    let triangles = mesh.triangles();
    let mut parent: Vec<u32> =
        (0..u32::try_from(mesh.vertices().len()).unwrap_or(u32::MAX)).collect();
    for t in triangles {
        let a = root(&mut parent, t[0]);
        for &v in &t[1..] {
            let b = root(&mut parent, v);
            if a != b {
                parent[b as usize] = a;
            }
        }
    }
    let mut shells: Vec<Vec<usize>> = Vec::new();
    let mut shell_of: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    for (i, t) in triangles.iter().enumerate() {
        let r = root(&mut parent, t[0]);
        let k = *shell_of.entry(r).or_insert_with(|| {
            shells.push(Vec::new());
            shells.len() - 1
        });
        shells[k].push(i);
    }
    shells
}

/// Where `p`, moved by `offset`, is written: the nearest `f32` of each coordinate, negative zero
/// as zero; `None` when it is not finite or beyond `f32`.
#[must_use]
pub fn stored(p: Point3, offset: [f64; 3]) -> Option<[f32; 3]> {
    let c = [p.x + offset[0], p.y + offset[1], p.z + offset[2]];
    let mut out = [0.0_f32; 3];
    for (value, &coordinate) in out.iter_mut().zip(&c) {
        #[allow(clippy::cast_possible_truncation)] // Rounding to f32 is the format's precision.
        let rounded = coordinate as f32;
        if !rounded.is_finite() {
            return None;
        }
        // Adding zero turns negative zero into zero.
        *value = rounded + 0.0;
    }
    Some(out)
}

/// Write `bodies` as a new file `destination` in `format`, published whole.
///
/// # Errors
/// [`ModelFileError`]: an existing destination, a wrong extension, nothing to write, a coordinate
/// or a count the format cannot hold, a file system failure, or cancellation. The destination is
/// then as it was, and the staging file removed.
pub fn write_model_file(
    destination: &Path,
    format: ModelFileFormat,
    bodies: &[ModelBody<'_>],
    cancel: &mut Cancellation<'_>,
) -> Result<ModelFileSummary, ModelFileError> {
    write(destination, format, bodies, false, cancel)
}

/// [`write_model_file`], replacing `destination` if it exists: what the user confirmed. The
/// complete staging file is renamed over it, so a failure or a cancellation still leaves the
/// existing file as it was.
///
/// # Errors
/// As [`write_model_file`], but for an existing destination.
pub fn replace_model_file(
    destination: &Path,
    format: ModelFileFormat,
    bodies: &[ModelBody<'_>],
    cancel: &mut Cancellation<'_>,
) -> Result<ModelFileSummary, ModelFileError> {
    write(destination, format, bodies, true, cancel)
}

/// `offset` to the nearest point of the Boolean kernel's grid (2⁻¹⁵ mm, TD-140): a drilled model,
/// on the grid, moved by it stays on the grid, so a file still stores it exactly. It differs
/// from `offset` by at most 2⁻¹⁶ mm on each axis.
#[must_use]
pub fn grid_offset(offset: [f64; 3]) -> [f64; 3] {
    let steps = crate::boolean::grid::STEPS_PER_MM;
    offset.map(|c| (c * steps).round_ties_even() / steps)
}

fn write(
    destination: &Path,
    format: ModelFileFormat,
    bodies: &[ModelBody<'_>],
    replace: bool,
    cancel: &mut Cancellation<'_>,
) -> Result<ModelFileSummary, ModelFileError> {
    cancel.check(Checkpoint::ExportStart)?;
    if destination
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        != Some(format.extension().to_owned())
    {
        return Err(ModelFileError::Extension(format));
    }
    if bodies.iter().all(|b| b.mesh.triangles().is_empty()) {
        return Err(ModelFileError::Empty);
    }
    if !replace
        && destination
            .try_exists()
            .map_err(|source| ModelFileError::Io {
                operation: "check the destination",
                source,
            })?
    {
        return Err(ModelFileError::Exists);
    }
    let path = stage_path(destination)?;
    let file = OpenOptions::new()
        .write(true)
        .read(true)
        .create_new(true)
        .open(&path)
        .map_err(|source| ModelFileError::Io {
            operation: "create the staging file",
            source,
        })?;
    let mut stage = Stage {
        path,
        directory: false,
        published: false,
    };
    let mut writer = BufWriter::with_capacity(1 << 20, file);
    let mut summary = match format {
        ModelFileFormat::Stl => write_stl(&mut writer, bodies, cancel)?,
        ModelFileFormat::Obj => write_obj(&mut writer, bodies, cancel)?,
        ModelFileFormat::ThreeMf => write_3mf(&mut writer, bodies, cancel)?,
    };
    cancel.check(Checkpoint::ExportFinish)?;
    let file = writer
        .into_inner()
        .map_err(|e| io_error("flush the staging file", e.into_error()))?;
    file.sync_all()
        .map_err(|e| io_error("sync the staging file", e))?;
    summary.bytes = file
        .metadata()
        .map_err(|e| io_error("measure the staging file", e))?
        .len();
    drop(file);
    cancel.check(Checkpoint::ExportPublish)?;
    if replace {
        fs::rename(&stage.path, destination).map_err(|e| io_error("replace the model file", e))?;
    } else {
        publish_file(&stage.path, destination)?;
        // Published: failing to remove the staging link cannot undo the complete destination.
        let _ = fs::remove_file(&stage.path);
    }
    stage.published = true;
    Ok(summary)
}

/// Every vertex of `body` where it is written.
fn stored_vertices(body: &ModelBody<'_>) -> Result<Vec<[f32; 3]>, ModelFileError> {
    body.mesh
        .vertices()
        .iter()
        .enumerate()
        .map(|(vertex, &p)| {
            stored(p, body.offset).ok_or_else(|| ModelFileError::Coordinate {
                body: body.name.to_owned(),
                vertex,
            })
        })
        .collect()
}

/// Triangles between cancellation checks.
const CHUNK: usize = 1 << 16;

/// Binary STL's 80-byte header: never starting with `solid`, which would read as ASCII.
const STL_HEADER: &[u8; 80] =
    b"karasu-slicer model export, binary STL, millimetres                             ";

fn write_stl(
    out: &mut impl Write,
    bodies: &[ModelBody<'_>],
    cancel: &mut Cancellation<'_>,
) -> Result<ModelFileSummary, ModelFileError> {
    let total: usize = bodies.iter().map(|b| b.mesh.triangles().len()).sum();
    let count = u32::try_from(total).map_err(|_| ModelFileError::TooLarge("STL triangle count"))?;
    let io = |source| ModelFileError::Io {
        operation: "write the STL file",
        source,
    };
    out.write_all(STL_HEADER).map_err(io)?;
    out.write_all(&count.to_le_bytes()).map_err(io)?;
    let mut record = [0_u8; 50];
    for body in bodies {
        let vertices = stored_vertices(body)?;
        for (n, triangle) in body.mesh.triangles().iter().enumerate() {
            if n % CHUNK == 0 {
                cancel.check(Checkpoint::ExportLayer)?;
            }
            let corners = triangle.map(|v| vertices[v as usize]);
            let values = std::iter::once(normal(corners)).chain(corners);
            // Twelve little-endian f32 (the normal, then the corners), then the attribute.
            for (slot, c) in record
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(values.flatten())
            {
                *slot = c.to_le_bytes();
            }
            // The attribute bytes stay zero.
            out.write_all(&record).map_err(io)?;
        }
    }
    Ok(ModelFileSummary {
        bodies: bodies.len(),
        triangles: total,
        vertices: 3 * total,
        bytes: 0,
        shared_edges: 0,
    })
}

/// The unit normal of a triangle of stored corners, by its winding; zero when it has no area.
/// Readers take the winding, not the normal: it is written for those that show it.
#[allow(clippy::many_single_char_names)] // Points and vectors of the cross product.
fn normal([a, b, c]: [[f32; 3]; 3]) -> [f32; 3] {
    let d = |p: [f32; 3], q: [f32; 3]| [0, 1, 2].map(|k| f64::from(q[k]) - f64::from(p[k]));
    let (u, v) = (d(a, b), d(a, c));
    let n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if length > 0.0 && length.is_finite() {
        #[allow(clippy::cast_possible_truncation)] // A unit vector's components.
        n.map(|c| (c / length) as f32)
    } else {
        [0.0; 3]
    }
}

fn write_obj(
    out: &mut impl Write,
    bodies: &[ModelBody<'_>],
    cancel: &mut Cancellation<'_>,
) -> Result<ModelFileSummary, ModelFileError> {
    let io = |source| ModelFileError::Io {
        operation: "write the OBJ file",
        source,
    };
    out.write_all(b"# karasu-slicer model export, millimetres\n")
        .map_err(io)?;
    let (mut base, mut triangles) = (1_u64, 0);
    let mut line = String::with_capacity(96);
    for body in bodies {
        let vertices = stored_vertices(body)?;
        // Object names are one token on the line: whitespace becomes underscores.
        let name: String = body
            .name
            .chars()
            .map(|c| if c.is_whitespace() { '_' } else { c })
            .collect();
        writeln!(out, "o {name}").map_err(io)?;
        for (n, [x, y, z]) in vertices.iter().enumerate() {
            if n % CHUNK == 0 {
                cancel.check(Checkpoint::ExportLayer)?;
            }
            line.clear();
            let [x, y, z] = [x, y, z].map(|&c| f64::from(c));
            let _ = writeln!(line, "v {x} {y} {z}");
            out.write_all(line.as_bytes()).map_err(io)?;
        }
        for (n, t) in body.mesh.triangles().iter().enumerate() {
            if n % CHUNK == 0 {
                cancel.check(Checkpoint::ExportLayer)?;
            }
            let [a, b, c] = t.map(|v| u64::from(v) + base);
            line.clear();
            let _ = writeln!(line, "f {a} {b} {c}");
            out.write_all(line.as_bytes()).map_err(io)?;
        }
        base += vertices.len() as u64;
        triangles += body.mesh.triangles().len();
    }
    Ok(ModelFileSummary {
        bodies: bodies.len(),
        triangles,
        vertices: usize::try_from(base - 1).unwrap_or(usize::MAX),
        bytes: 0,
        shared_edges: 0,
    })
}

const CONTENT_TYPES: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8"?>"#,
    "\n",
    r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
    r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
    r#"<Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/>"#,
    "</Types>\n"
);

const RELATIONSHIPS: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8"?>"#,
    "\n",
    r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    r#"<Relationship Target="/3D/3dmodel.model" Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/>"#,
    "</Relationships>\n"
);

/// The 3MF package's limits: the classic ZIP writer's 4 GiB.
const PACKAGE_LIMITS: EncoderLimits = EncoderLimits {
    max_output_bytes: u32::MAX as u64,
    max_metadata_bytes: 1 << 20,
    max_layer_bytes: u32::MAX as usize,
};

fn write_3mf(
    out: &mut (impl Write + io::Seek),
    bodies: &[ModelBody<'_>],
    cancel: &mut Cancellation<'_>,
) -> Result<ModelFileSummary, ModelFileError> {
    let mut model = String::new();
    model.push_str(concat!(
        r#"<?xml version="1.0" encoding="UTF-8"?>"#,
        "\n",
        r#"<model unit="millimeter" xml:lang="en-US" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">"#,
        "\n",
        r#"<metadata name="Application">karasu-slicer</metadata>"#,
        "\n<resources>\n"
    ));
    let (mut triangles, mut vertex_count, mut shared_edges) = (0, 0, 0);
    for (id, body) in (1..).zip(bodies) {
        let stored = stored_vertices(body)?;
        cancel.check(Checkpoint::ExportLayer)?;
        let (vertices, faces, left) = manifold_edges(&stored, body.mesh.triangles());
        shared_edges += left;
        let _ = writeln!(
            model,
            r#"<object id="{id}" type="model" name="{}"><mesh><vertices>"#,
            escape(body.name)
        );
        for (n, [x, y, z]) in vertices.iter().enumerate() {
            if n % CHUNK == 0 {
                cancel.check(Checkpoint::ExportLayer)?;
            }
            let [x, y, z] = [x, y, z].map(|&c| f64::from(c));
            let _ = writeln!(model, r#"<vertex x="{x}" y="{y}" z="{z}"/>"#);
        }
        model.push_str("</vertices><triangles>\n");
        for (n, [a, b, c]) in faces.iter().enumerate() {
            if n % CHUNK == 0 {
                cancel.check(Checkpoint::ExportLayer)?;
            }
            let _ = writeln!(model, r#"<triangle v1="{a}" v2="{b}" v3="{c}"/>"#);
        }
        model.push_str("</triangles></mesh></object>\n");
        triangles += body.mesh.triangles().len();
        vertex_count += vertices.len();
    }
    model.push_str("</resources>\n<build>\n");
    for (id, _) in (1..).zip(bodies) {
        let _ = writeln!(model, r#"<item objectid="{id}"/>"#);
    }
    model.push_str("</build>\n</model>\n");
    let mut zip = ClassicZip::new(PACKAGE_LIMITS);
    for (name, data) in [
        ("[Content_Types].xml", CONTENT_TYPES.as_bytes().to_vec()),
        ("_rels/.rels", RELATIONSHIPS.as_bytes().to_vec()),
        ("3D/3dmodel.model", model.into_bytes()),
    ] {
        let member = Member::deflated(data, cancel)?;
        zip.add_member(out, name, &member, cancel)?;
    }
    zip.finish(out, cancel)?;
    Ok(ModelFileSummary {
        bodies: bodies.len(),
        triangles,
        vertices: vertex_count,
        bytes: 0,
        shared_edges,
    })
}

/// `vertices` and `triangles` with every edge used by exactly two triangles, by vertex index, as
/// 3MF requires of a `model` object (Core §4.1), and the number of edges left used by more.
///
/// A closed oriented mesh can use one edge in more than two triangles: where two bodies of a
/// union touch along a line, four faces meet there. Around such an edge, ordered by their exact
/// angle about it, the faces alternate in direction. They are paired so that each pair bounds
/// one wedge of solid: a face using the edge backwards, with the next one using it forwards.
/// The vertices are then copied so that each sheet has its own: corners joined through paired
/// edges share a vertex, the others do not. Where the sheets still meet around both ends of such
/// an edge, its faces are paired the other way instead, each pair bounding a wedge of empty
/// space, which separates them there; a few passes settle it. Nothing moves: a copy is a new
/// index at the same point. A mesh without such edges comes back unchanged.
pub(crate) fn manifold_edges(
    vertices: &[[f32; 3]],
    triangles: &[[u32; 3]],
) -> (Vec<[f32; 3]>, Vec<[u32; 3]>, usize) {
    // Each edge's uses: (triangle, corner of its start), keyed by its ends in increasing order.
    let mut uses: std::collections::HashMap<(u32, u32), Vec<(u32, u8)>> =
        std::collections::HashMap::new();
    for (t, triangle) in (0_u32..).zip(triangles) {
        for k in 0..3_u8 {
            let (a, b) = (triangle[k as usize], triangle[(k as usize + 1) % 3]);
            uses.entry((a.min(b), a.max(b))).or_default().push((t, k));
        }
    }
    if uses.values().all(|u| u.len() <= 2) {
        return (vertices.to_vec(), triangles.to_vec(), 0);
    }
    let mut keys: Vec<(u32, u32)> = uses.keys().copied().collect();
    keys.sort_unstable();
    // The plain edges' pairs, and each shared edge's faces counterclockwise about u → v:
    // (forwards, triangle).
    let mut plain = Vec::with_capacity(keys.len());
    let mut shared: Vec<Shared> = Vec::new();
    for (u, v) in keys {
        let faces = &uses[&(u, v)];
        if faces.len() <= 2 {
            if let [a, b] = faces[..] {
                plain.push(((u, v), (a.0, b.0)));
            }
            continue;
        }
        shared.push(((u, v), around_edge(vertices, triangles, (u, v), faces)));
    }
    let corner_of = |t: u32, v: u32| {
        let k = triangles[t as usize]
            .iter()
            .position(|&w| w == v)
            .expect("an edge's end is a corner of its triangle");
        3 * t + u32::try_from(k).unwrap_or(0)
    };
    // Corners joined through paired edges: union-find over triangle * 3 + corner.
    let count = u32::try_from(3 * triangles.len()).unwrap_or(u32::MAX);
    let mut flipped = vec![false; shared.len()];
    let mut parent: Vec<u32> = Vec::new();
    let mut left = 0;
    for _ in 0..4 {
        parent = (0..count).collect();
        let join = |parent: &mut Vec<u32>, (u, v): (u32, u32), (a, b): (u32, u32)| {
            for w in [u, v] {
                let (x, y) = (root(parent, corner_of(a, w)), root(parent, corner_of(b, w)));
                if x != y {
                    parent[x.max(y) as usize] = x.min(y);
                }
            }
        };
        for &(edge, pair) in &plain {
            join(&mut parent, edge, pair);
        }
        let pairings: Vec<Vec<(u32, u32)>> = shared
            .iter()
            .zip(&flipped)
            .map(|((_, around), &flip)| pair_faces(around, flip))
            .collect();
        for ((edge, _), pairs) in shared.iter().zip(&pairings) {
            for &pair in pairs {
                join(&mut parent, *edge, pair);
            }
        }
        // An edge is still shared where two of its pairs end at the same two vertices.
        left = 0;
        let mut changed = false;
        for (k, ((edge, _), pairs)) in shared.iter().zip(&pairings).enumerate() {
            let mut ends: Vec<(u32, u32)> = pairs
                .iter()
                .map(|&(a, _)| {
                    (
                        root(&mut parent, corner_of(a, edge.0)),
                        root(&mut parent, corner_of(a, edge.1)),
                    )
                })
                .collect();
            ends.sort_unstable();
            ends.dedup();
            if ends.len() < pairs.len() {
                left += 1;
                flipped[k] = !flipped[k];
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let (out_vertices, out) = renumber(vertices, triangles, &mut parent);
    (out_vertices, out, left)
}

/// A vertex for each group of joined corners (`parent`'s roots over triangle * 3 + corner), at
/// its vertex's point, numbered in order of first use, and the triangles on them.
fn renumber(
    vertices: &[[f32; 3]],
    triangles: &[[u32; 3]],
    parent: &mut [u32],
) -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
    let mut index: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    let mut out_vertices = Vec::with_capacity(vertices.len());
    let mut out = Vec::with_capacity(triangles.len());
    for (t, triangle) in (0_u32..).zip(triangles) {
        let mut corners = [0; 3];
        for (k, &v) in triangle.iter().enumerate() {
            let r = root(parent, 3 * t + u32::try_from(k).unwrap_or(0));
            let next = u32::try_from(out_vertices.len()).unwrap_or(u32::MAX);
            corners[k] = *index.entry(r).or_insert_with(|| {
                out_vertices.push(vertices[v as usize]);
                next
            });
        }
        out.push(corners);
    }
    (out_vertices, out)
}

/// A shared edge, by its ends in increasing order, and its faces counterclockwise about it:
/// (uses it forwards, triangle).
type Shared = ((u32, u32), Vec<(bool, u32)>);

/// The union-find root of `x`, halving the path.
fn root(parent: &mut [u32], mut x: u32) -> u32 {
    while parent[x as usize] != x {
        parent[x as usize] = parent[parent[x as usize] as usize];
        x = parent[x as usize];
    }
    x
}

/// The faces using edge `(u, v)` (each as its triangle and the corner its use starts at),
/// ordered counterclockwise about `u → v` by their exact angle from the first one's, ties by
/// triangle: each as (uses it forwards, triangle).
pub(crate) fn around_edge(
    vertices: &[[f32; 3]],
    triangles: &[[u32; 3]],
    (u, v): (u32, u32),
    faces: &[(u32, u8)],
) -> Vec<(bool, u32)> {
    use crate::preparation::hollow::numeric::orient3d;
    use std::cmp::Ordering;
    let point = |vertex: u32| {
        let c = vertices[vertex as usize].map(f64::from);
        Point3::new(c[0], c[1], c[2])
    };
    let mut around: Vec<(bool, u32, u32)> = faces
        .iter()
        .map(|&(t, k)| {
            let tri = triangles[t as usize];
            (tri[k as usize] == u, t, tri[(k as usize + 2) % 3])
        })
        .collect();
    let (pu, pv) = (point(u), point(v));
    let reference = point(around[0].2);
    let axis = [pv.x - pu.x, pv.y - pu.y, pv.z - pu.z];
    let normal_to = |w: Point3| {
        let r = [w.x - pu.x, w.y - pu.y, w.z - pu.z];
        [
            axis[1] * r[2] - axis[2] * r[1],
            axis[2] * r[0] - axis[0] * r[2],
            axis[0] * r[1] - axis[1] * r[0],
        ]
    };
    let half = |w: Point3| match orient3d(pu, pv, reference, w) {
        Ordering::Greater => 0,
        Ordering::Less => 1,
        // In the reference's plane: on its ray, or the opposite one.
        Ordering::Equal => {
            let (a, b) = (normal_to(reference), normal_to(w));
            u8::from(a[0] * b[0] + a[1] * b[1] + a[2] * b[2] < 0.0)
        }
    };
    // Counterclockwise about u → v from the reference; ties by triangle.
    around.sort_by(|a, b| {
        let (wa, wb) = (point(a.2), point(b.2));
        half(wa)
            .cmp(&half(wb))
            .then_with(|| orient3d(pu, pv, wb, wa))
            .then(a.1.cmp(&b.1))
    });
    around.iter().map(|&(f, t, _)| (f, t)).collect()
}

/// The faces about an edge (in counterclockwise order, each forwards or not) paired: each face
/// using the edge backwards with the next one using it forwards, cyclically (the pair bounds a
/// wedge of solid), or with `flip` the previous one (a wedge of empty space).
pub(crate) fn pair_faces(around: &[(bool, u32)], flip: bool) -> Vec<(u32, u32)> {
    let n = around.len();
    let mut taken = vec![false; n];
    let mut pairs = Vec::with_capacity(n / 2);
    for i in 0..n {
        if around[i].0 || taken[i] {
            continue;
        }
        let step = |s: usize| if flip { (i + n - s) % n } else { (i + s) % n };
        if let Some(j) = (1..n).map(step).find(|&j| around[j].0 && !taken[j]) {
            taken[i] = true;
            taken[j] = true;
            pairs.push((around[i].1, around[j].1));
        }
    }
    pairs
}

/// `text` as XML attribute content.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Control characters are not allowed in XML 1.0 text.
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{stored, write_model_file, BodyKind, ModelBody, ModelFileError, ModelFileFormat};
    use crate::{
        cancellation::{Cancellation, Checkpoint},
        geometry::Point3,
        mesh::{formats::FormatRegistry, formats::LengthUnit, TriangleMesh},
    };
    use std::path::{Path, PathBuf};

    /// A closed box from `lo` to `hi`, facing out.
    fn cube(lo: [f64; 3], hi: [f64; 3]) -> TriangleMesh {
        let p = |i: usize| {
            Point3::new(
                if i & 1 == 0 { lo[0] } else { hi[0] },
                if i & 2 == 0 { lo[1] } else { hi[1] },
                if i & 4 == 0 { lo[2] } else { hi[2] },
            )
        };
        let quads = [
            [0, 2, 3, 1],
            [4, 5, 7, 6],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [0, 4, 6, 2],
            [1, 3, 7, 5],
        ];
        let triangles = quads
            .iter()
            .flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]])
            .collect();
        TriangleMesh::try_new((0..8).map(p).collect(), triangles).unwrap()
    }

    fn folder(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("karasu-model-files-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The file read back by karasu's importer, its vertices sorted.
    fn read(path: &Path) -> (Vec<[u64; 3]>, usize) {
        let imported = FormatRegistry::builtin()
            .import_path(
                path,
                &crate::mesh::formats::ModelImportOptions::new(Some(LengthUnit::Millimeter)),
                &mut Cancellation::never(),
            )
            .unwrap();
        let mesh = &imported.mesh;
        let mut vertices: Vec<[u64; 3]> = mesh
            .vertices()
            .iter()
            .map(|p| [p.x, p.y, p.z].map(f64::to_bits))
            .collect();
        vertices.sort_unstable();
        (vertices, mesh.triangles().len())
    }

    #[test]
    fn every_format_reads_back_as_the_bodies_written() {
        let dir = folder("roundtrip");
        let model = cube([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let supports = cube([2.0, 2.0, -5.0], [3.0, 3.0, 0.5]);
        let bodies = [
            ModelBody {
                name: "Venom & <friends>",
                kind: BodyKind::Model,
                mesh: &model,
                offset: [1.0 / 3.0, 20.0, 0.0],
            },
            ModelBody {
                name: "Venom supports",
                kind: BodyKind::Supports,
                mesh: &supports,
                offset: [1.0 / 3.0, 20.0, 0.0],
            },
        ];
        // What every format must hold: the corners where they are stored, as f32.
        let mut expected: Vec<[u64; 3]> = bodies
            .iter()
            .flat_map(|b| {
                b.mesh
                    .vertices()
                    .iter()
                    .map(|&p| stored(p, b.offset).unwrap().map(|c| f64::from(c).to_bits()))
            })
            .collect();
        expected.sort_unstable();
        for format in ModelFileFormat::ALL {
            let path = dir.join(format!("plate.{}", format.extension()));
            let summary =
                write_model_file(&path, format, &bodies, &mut Cancellation::never()).unwrap();
            assert_eq!((summary.bodies, summary.triangles), (2, 24), "{format:?}");
            assert_eq!(
                summary.bytes,
                std::fs::metadata(&path).unwrap().len(),
                "{format:?}"
            );
            assert_eq!(read(&path), (expected.clone(), 24), "{format:?}");
            // The same bodies, the same bytes.
            let again = dir.join(format!("again.{}", format.extension()));
            write_model_file(&again, format, &bodies, &mut Cancellation::never()).unwrap();
            assert_eq!(
                std::fs::read(&path).unwrap(),
                std::fs::read(&again).unwrap()
            );
        }
        // No staging file is left behind.
        let left: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .filter(|n| n.to_string_lossy().starts_with('.'))
            .collect();
        assert!(left.is_empty(), "{left:?}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_3mf_names_each_body_and_builds_every_one() {
        let dir = folder("names");
        let model = cube([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
        let bodies = [
            ModelBody {
                name: "A \"quoted\" & <tagged> name",
                kind: BodyKind::Model,
                mesh: &model,
                offset: [0.0; 3],
            },
            ModelBody {
                name: "raft",
                kind: BodyKind::Raft,
                mesh: &model,
                offset: [5.0, 0.0, 0.0],
            },
        ];
        let path = dir.join("named.3mf");
        write_model_file(
            &path,
            ModelFileFormat::ThreeMf,
            &bodies,
            &mut Cancellation::never(),
        )
        .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let archive = crate::mesh::formats::zip::Archive::parse(
            &bytes,
            crate::mesh::formats::zip::ArchiveLimits {
                max_entries: 16,
                max_entry_bytes: 1 << 20,
            },
        )
        .unwrap();
        let entry = archive.find("3D/3dmodel.model").unwrap();
        let text = String::from_utf8(archive.read(entry).unwrap()).unwrap();
        assert!(
            text.contains(r#"name="A &quot;quoted&quot; &amp; &lt;tagged&gt; name""#),
            "{text}"
        );
        assert_eq!(text.matches("<item objectid=").count(), 2);
        assert_eq!(text.matches(r#"type="model""#).count(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_3mf_gives_each_sheet_its_own_vertices_where_four_faces_share_an_edge() {
        // Two cubes touching along the edge x = y = 1, welded into one closed mesh: that edge
        // and its two ends are shared by both bodies.
        let (a, b) = (
            cube([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]),
            cube([1.0, 1.0, 0.0], [2.0, 2.0, 1.0]),
        );
        let mut vertices: Vec<Point3> = Vec::new();
        let mut triangles = Vec::new();
        for mesh in [&a, &b] {
            for t in mesh.triangles() {
                triangles.push(t.map(|v| {
                    let p = mesh.vertices()[v as usize];
                    let i = vertices.iter().position(|q| *q == p).unwrap_or_else(|| {
                        vertices.push(p);
                        vertices.len() - 1
                    });
                    u32::try_from(i).unwrap()
                }));
            }
        }
        assert_eq!(vertices.len(), 14);
        let welded = TriangleMesh::try_new(vertices, triangles).unwrap();
        let dir = folder("sheets");
        let path = dir.join("touching.3mf");
        let body = [ModelBody {
            name: "touching",
            kind: BodyKind::Model,
            mesh: &welded,
            offset: [0.0; 3],
        }];
        let summary = write_model_file(
            &path,
            ModelFileFormat::ThreeMf,
            &body,
            &mut Cancellation::never(),
        )
        .unwrap();
        // The two shared vertices get a copy each.
        assert_eq!(summary.vertices, 16);
        let bytes = std::fs::read(&path).unwrap();
        let archive = crate::mesh::formats::zip::Archive::parse(
            &bytes,
            crate::mesh::formats::zip::ArchiveLimits {
                max_entries: 16,
                max_entry_bytes: 1 << 20,
            },
        )
        .unwrap();
        let text = String::from_utf8(
            archive
                .read(archive.find("3D/3dmodel.model").unwrap())
                .unwrap(),
        )
        .unwrap();
        let mut edges = std::collections::HashMap::new();
        let faces: Vec<Vec<u32>> = text
            .lines()
            .filter(|l| l.starts_with("<triangle "))
            .map(|line| {
                line.split('"')
                    .skip(1)
                    .step_by(2)
                    .map(|n| n.parse().unwrap())
                    .collect()
            })
            .collect();
        for v in &faces {
            for k in 0..3 {
                let (p, q) = (v[k], v[(k + 1) % 3]);
                *edges.entry((p.min(q), p.max(q))).or_insert(0) += 1;
            }
        }
        assert_eq!(edges.len(), 36);
        assert!(edges.values().all(|&n| n == 2), "{edges:?}");
        // Each cube is a sheet of its own: the faces connected through shared vertices are the
        // twelve of one cube, never some of each.
        let mut group: Vec<usize> = (0..faces.len()).collect();
        loop {
            let mut changed = false;
            for i in 0..faces.len() {
                for j in 0..faces.len() {
                    if group[j] < group[i] && faces[i].iter().any(|v| faces[j].contains(v)) {
                        group[i] = group[j];
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        assert!(group[..12].iter().all(|&g| g == 0), "{group:?}");
        assert!(group[12..].iter().all(|&g| g == 12), "{group:?}");
        // Read back, the cubes are what they were.
        assert_eq!(read(&path).1, 24);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn nothing_is_replaced_or_left_behind_on_refusal() {
        let dir = folder("refusals");
        let model = cube([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
        let body = [ModelBody {
            name: "m",
            kind: BodyKind::Model,
            mesh: &model,
            offset: [0.0; 3],
        }];
        let never = || Cancellation::never();
        let existing = dir.join("existing.stl");
        std::fs::write(&existing, b"keep").unwrap();
        let result = write_model_file(&existing, ModelFileFormat::Stl, &body, &mut never());
        assert!(matches!(result, Err(ModelFileError::Exists)), "{result:?}");
        assert_eq!(std::fs::read(&existing).unwrap(), b"keep");
        let result = write_model_file(
            &dir.join("m.obj"),
            ModelFileFormat::Stl,
            &body,
            &mut never(),
        );
        assert!(
            matches!(result, Err(ModelFileError::Extension(ModelFileFormat::Stl))),
            "{result:?}"
        );
        let far = TriangleMesh::try_new(
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1e300, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
            ],
            vec![[0, 1, 2]],
        )
        .unwrap();
        let far_body = [ModelBody {
            name: "far",
            kind: BodyKind::Model,
            mesh: &far,
            offset: [0.0; 3],
        }];
        let result = write_model_file(
            &dir.join("far.stl"),
            ModelFileFormat::Stl,
            &far_body,
            &mut never(),
        );
        assert!(
            matches!(&result, Err(ModelFileError::Coordinate { vertex: 1, .. })),
            "{result:?}"
        );
        for format in ModelFileFormat::ALL {
            let mut predicate = |c| c == Checkpoint::ExportLayer;
            let path = dir.join(format!("cancelled.{}", format.extension()));
            let result =
                write_model_file(&path, format, &body, &mut Cancellation::new(&mut predicate));
            assert!(
                matches!(result, Err(ModelFileError::Cancelled(_))),
                "{result:?}"
            );
            assert!(!path.exists());
        }
        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["existing.stl".to_owned()]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn repeated_shells_and_caps_back_to_back_are_tidied_away() {
        // Two boxes stacked face to face (their shared face back to back), and the upper box
        // again: one is repeated, and the faces between them cancel.
        let (lower, upper) = (
            cube([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]),
            cube([0.0, 0.0, 1.0], [1.0, 1.0, 2.0]),
        );
        let mut vertices = Vec::new();
        let mut triangles = Vec::new();
        for part in [&lower, &upper, &upper] {
            let base = u32::try_from(vertices.len()).unwrap();
            vertices.extend_from_slice(part.vertices());
            triangles.extend(part.triangles().iter().map(|t| t.map(|v| v + base)));
        }
        let body = TriangleMesh::try_new(vertices, triangles).unwrap();
        let tidied = super::tidy_shells(&body);
        assert_eq!((tidied.repeated, tidied.cancelled), (1, 2));
        // The two boxes' union: 20 faces on 12 points, closed, each edge used twice.
        assert_eq!(tidied.mesh.triangles().len(), 20);
        assert_eq!(tidied.mesh.vertices().len(), 12);
        let mut edges = std::collections::HashMap::new();
        for t in tidied.mesh.triangles() {
            for k in 0..3 {
                *edges.entry((t[k], t[(k + 1) % 3])).or_insert(0) += 1;
            }
        }
        for (&(a, b), &n) in &edges {
            assert_eq!(edges.get(&(b, a)), Some(&n), "{a} {b}");
        }
    }

    #[test]
    fn a_confirmed_replacement_takes_the_place_of_the_existing_file() {
        let dir = folder("replace");
        let model = cube([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
        let body = [ModelBody {
            name: "m",
            kind: BodyKind::Model,
            mesh: &model,
            offset: [0.0; 3],
        }];
        let path = dir.join("m.stl");
        std::fs::write(&path, b"old").unwrap();
        // Cancelled, the old file stays.
        let mut predicate = |c| c == Checkpoint::ExportPublish;
        let result = super::replace_model_file(
            &path,
            ModelFileFormat::Stl,
            &body,
            &mut Cancellation::new(&mut predicate),
        );
        assert!(
            matches!(result, Err(ModelFileError::Cancelled(_))),
            "{result:?}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"old");
        let summary = super::replace_model_file(
            &path,
            ModelFileFormat::Stl,
            &body,
            &mut Cancellation::never(),
        )
        .unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), summary.bytes);
        assert_eq!(read(&path).1, 12);
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
        assert_eq!(names.len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_offset_on_the_grid_keeps_a_grid_point_on_it() {
        let offset = super::grid_offset([12.345_678_9, -0.1, 0.0]);
        let step = 1.0 / 32_768.0;
        for (o, c) in offset.iter().zip([12.345_678_9, -0.1, 0.0]) {
            assert!((o - c).abs() <= step / 2.0);
            assert_eq!((o / step).fract(), 0.0);
        }
        // A grid point moved by it is a grid point, and an f32 value.
        let p = 100.0 + 3.0 * step + offset[0];
        assert_eq!((p / step).fract(), 0.0);
        #[allow(clippy::cast_possible_truncation)]
        let stored = f64::from(p as f32);
        assert_eq!(stored, p);
    }

    #[test]
    fn coordinates_are_stored_as_the_nearest_f32_and_zero_has_no_sign() {
        let p = Point3::new(-0.0, 0.1, 1.0 / 3.0);
        let [x, y, z] = stored(p, [0.0; 3]).unwrap();
        assert_eq!(x.to_bits(), 0);
        assert_eq!(y, 0.1_f32);
        #[allow(clippy::cast_possible_truncation)] // The rounding under test.
        let third = (1.0_f64 / 3.0) as f32;
        assert_eq!(z, third);
        assert!(stored(Point3::new(f64::MAX, 0.0, 0.0), [0.0; 3]).is_none());
    }
}
