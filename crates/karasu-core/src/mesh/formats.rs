//! Extensible model import (TD-057): one interface over STL, OBJ, 3MF and future formats.
//!
//! - **Formats:** a [`ModelFormat`] parses a file's bytes into a mesh in the file's own length
//!   units, with the unit it declares (if any), notes about what it ignored or converted, and the
//!   mesh diagnostics.
//! - **Registry:** [`FormatRegistry`] owns the formats, selects one by file extension (never by
//!   guessing from content), and refuses duplicate identifiers or extensions.
//! - **Units:** the registry resolves units the same way for every format. A unit declared by the
//!   file is used, and an assumed unit that contradicts it is an error. A file that declares no
//!   unit (STL, OBJ) needs an explicitly assumed one. Coordinates are then scaled to millimetres.
//!   Millimetre files are not rescaled, so their coordinates stay bit-exact.
//! - **Shared construction:** formats build meshes through the shared welding builder, which
//!   gives equal positions one vertex by exact coordinate bits (signed zero normalized), on all
//!   cores and exactly as STL import does (TD-058). The same geometry therefore gives the same mesh from any format.
//!
//! A new format implements [`ModelFormat`] and is added with [`FormatRegistry::register`].

pub(crate) mod obj;
pub(crate) mod threemf;
pub(crate) mod zip;

pub use obj::{ObjError, ObjFormat};
pub use threemf::{ThreeMfError, ThreeMfFormat};
pub use zip::ArchiveError;

use super::weld::{weld, WeldError};
use super::{
    stl::{
        import_stl_bytes_with_cancellation, DroppedFacets, ImportLimits, NonFinitePolicy,
        StlOptions, StlUnitPolicy,
    },
    ImportResource, MeshDiagnostic, MeshError, StlError, Triangle, TriangleMesh,
};
use crate::cancellation::{Cancellation, Cancelled, Checkpoint};
use crate::geometry::Point3;
use std::{error::Error, fmt, fs::File, io, io::Read, path::Path};

/// A length unit declared by a file or assumed by the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LengthUnit {
    /// 0.001 mm.
    Micrometer,
    /// 1 mm.
    Millimeter,
    /// 10 mm.
    Centimeter,
    /// 25.4 mm.
    Inch,
    /// 304.8 mm.
    Foot,
    /// 1000 mm.
    Meter,
}

impl LengthUnit {
    /// Every unit, smallest first.
    pub const ALL: [Self; 6] = [
        Self::Micrometer,
        Self::Millimeter,
        Self::Centimeter,
        Self::Inch,
        Self::Foot,
        Self::Meter,
    ];

    /// Millimetres per unit (exact SI and international-inch definitions).
    #[must_use]
    pub const fn millimeters(self) -> f64 {
        match self {
            Self::Micrometer => 0.001,
            Self::Millimeter => 1.0,
            Self::Centimeter => 10.0,
            Self::Inch => 25.4,
            Self::Foot => 304.8,
            Self::Meter => 1000.0,
        }
    }

    /// The 3MF spelling: `micron`, `millimeter`, `centimeter`, `inch`, `foot` or `meter`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Micrometer => "micron",
            Self::Millimeter => "millimeter",
            Self::Centimeter => "centimeter",
            Self::Inch => "inch",
            Self::Foot => "foot",
            Self::Meter => "meter",
        }
    }

    /// Short symbol: `um`, `mm`, `cm`, `in`, `ft` or `m`.
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Micrometer => "um",
            Self::Millimeter => "mm",
            Self::Centimeter => "cm",
            Self::Inch => "in",
            Self::Foot => "ft",
            Self::Meter => "m",
        }
    }

    /// Parse a symbol or a 3MF name, exactly as written by [`Self::symbol`] or [`Self::name`].
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|unit| unit.symbol() == text || unit.name() == text)
    }
}

impl fmt::Display for LengthUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Where the resolved unit came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitOrigin {
    /// The file declares it.
    Declared,
    /// The caller assumed it for a file that declares none.
    Assumed,
}

/// Resource caps of model import.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelLimits {
    /// Input size, triangle, vertex and text caps shared with STL import.
    pub input: ImportLimits,
    /// Maximum entries in a package (3MF ZIP).
    pub max_package_entries: usize,
    /// Maximum declared uncompressed bytes of one package part.
    pub max_part_bytes: u64,
    /// Maximum nesting of 3MF components.
    pub max_component_depth: usize,
    /// Maximum vertices of one polygon (OBJ face).
    pub max_polygon_vertices: usize,
}

impl Default for ModelLimits {
    fn default() -> Self {
        Self {
            input: ImportLimits::default(),
            max_package_entries: 65_536,
            max_part_bytes: 8 << 30,
            max_component_depth: 32,
            max_polygon_vertices: 65_536,
        }
    }
}

/// Model import choices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelImportOptions {
    /// The unit assumed for files that declare none; `None` refuses them. When a file declares a
    /// unit, an assumed unit must equal it.
    pub unit: Option<LengthUnit>,
    /// Resource caps.
    pub limits: ModelLimits,
    /// Facets with a non-finite corner coordinate: refused, or dropped and reported in
    /// [`ImportNote::NonFiniteFacetsDropped`]. STL applies it; OBJ and 3MF refuse such
    /// coordinates whatever it is.
    pub non_finite: NonFinitePolicy,
}

impl ModelImportOptions {
    /// Default limits, the given assumed unit, and refusal of non-finite coordinates.
    #[must_use]
    pub fn new(unit: Option<LengthUnit>) -> Self {
        Self {
            unit,
            limits: ModelLimits::default(),
            non_finite: NonFinitePolicy::Refuse,
        }
    }
}

/// Something an importer ignored, skipped or converted, reported instead of done silently.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImportNote {
    /// Polygons with more than three vertices were split into triangles.
    PolygonsTriangulated {
        /// Polygons split.
        polygons: usize,
        /// Triangles they produced.
        triangles: usize,
    },
    /// Statements without solid geometry were ignored (texture coordinates, normals, groups,
    /// materials, lines, points).
    StatementsIgnored {
        /// The statement keyword.
        keyword: String,
        /// How many.
        count: usize,
    },
    /// Per-vertex colours were ignored.
    VertexColorsIgnored {
        /// Vertices that carried a colour.
        count: usize,
    },
    /// Objects that are not solids were left out (3MF `support` and `surface` objects).
    ObjectsSkipped {
        /// The object type.
        kind: String,
        /// Build-item instances skipped.
        count: usize,
    },
    /// Colour and material properties were ignored; geometry does not depend on them.
    PropertiesIgnored,
    /// Objects came from other model parts of the package (3MF Production extension).
    ExternalParts {
        /// Distinct model parts read besides the root.
        count: usize,
    },
    /// A mirroring transform reversed the winding of an instance to keep it outward.
    MirroredInstances {
        /// Instances mirrored.
        count: usize,
    },
    /// Facets with a non-finite corner coordinate were left out
    /// ([`NonFinitePolicy::Drop`]); their positions are unknown.
    NonFiniteFacetsDropped(DroppedFacets),
}

impl fmt::Display for ImportNote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PolygonsTriangulated {
                polygons,
                triangles,
            } => write!(f, "{polygons} polygons split into {triangles} triangles"),
            Self::StatementsIgnored { keyword, count } => {
                write!(f, "{count} '{keyword}' statements ignored")
            }
            Self::VertexColorsIgnored { count } => write!(f, "{count} vertex colours ignored"),
            Self::ObjectsSkipped { kind, count } => {
                write!(f, "{count} {kind} objects skipped (not solid)")
            }
            Self::PropertiesIgnored => f.write_str("colours and materials ignored"),
            Self::ExternalParts { count } => write!(f, "objects read from {count} external parts"),
            Self::MirroredInstances { count } => {
                write!(f, "{count} mirrored instances re-wound outward")
            }
            Self::NonFiniteFacetsDropped(dropped) => {
                write!(
                    f,
                    "{} facet(s) with non-finite coordinates dropped, in {} run(s) (",
                    dropped.facets, dropped.runs
                )?;
                for (i, (first, count)) in dropped.ranges.iter().enumerate() {
                    let separator = if i == 0 { "" } else { ", " };
                    if *count == 1 {
                        write!(f, "{separator}facet {first}")?;
                    } else {
                        write!(f, "{separator}facets {first}-{}", first + count - 1)?;
                    }
                }
                if dropped.runs > dropped.ranges.len() {
                    write!(f, ", ...")?;
                }
                write!(f, "); their positions are unknown")
            }
        }
    }
}

/// What a format parsed, in the file's own units.
#[derive(Debug)]
pub struct SourceModel {
    /// The geometry, in file units.
    pub mesh: TriangleMesh,
    /// The unit the file declares, or `None` when the format has no unit declaration.
    pub declared_unit: Option<LengthUnit>,
    /// Instances or objects placed (1 for single-mesh formats).
    pub objects: usize,
    /// Reported conversions and omissions.
    pub notes: Vec<ImportNote>,
    /// Mesh diagnostics (at least [`TriangleMesh::diagnostics`]) in triangle order.
    pub diagnostics: Vec<MeshDiagnostic>,
}

/// One importable file format.
pub trait ModelFormat: Send + Sync {
    /// Stable lowercase identifier, e.g. `stl`, `obj`, `3mf`.
    fn id(&self) -> &'static str;
    /// Human-readable name.
    fn name(&self) -> &'static str;
    /// Lowercase file extensions without the dot.
    fn extensions(&self) -> &'static [&'static str];
    /// Whether files of this format declare their length unit.
    fn declares_units(&self) -> bool;
    /// Parse `bytes` into a mesh in file units. Implementations must be deterministic, bounded by
    /// `options.limits`, follow `options.non_finite` when they can, and return no partial model on
    /// failure. The unit is resolved by the registry, not here.
    ///
    /// # Errors
    /// The format's own structured failure, cancellation or a resource limit.
    fn parse(
        &self,
        bytes: &[u8],
        options: &ModelImportOptions,
        cancellation: &mut Cancellation<'_>,
    ) -> Result<SourceModel, ModelImportError>;
}

/// An imported model in millimetres.
#[derive(Debug)]
pub struct ImportedModel {
    /// Geometry in millimetres.
    pub mesh: TriangleMesh,
    /// Identifier of the format that read it.
    pub format: &'static str,
    /// The unit the file's coordinates were in.
    pub unit: LengthUnit,
    /// Whether the unit was declared or assumed.
    pub unit_origin: UnitOrigin,
    /// Instances or objects placed.
    pub objects: usize,
    /// Reported conversions and omissions.
    pub notes: Vec<ImportNote>,
    /// Mesh diagnostics in triangle order.
    pub diagnostics: Vec<MeshDiagnostic>,
}

/// Why a model could not be imported.
#[derive(Debug)]
#[non_exhaustive]
pub enum ModelImportError {
    /// Cooperative cancellation.
    Cancelled(Cancelled),
    /// Reading the file failed.
    Io(io::Error),
    /// No registered format has this file extension.
    UnknownFormat {
        /// The extension, lowercased, if the path has one.
        extension: Option<String>,
    },
    /// The format declares no unit and none was assumed.
    UnitsRequired {
        /// The format's name.
        format: &'static str,
    },
    /// The assumed unit contradicts the unit the file declares.
    UnitConflict {
        /// What the file declares.
        declared: LengthUnit,
        /// What the caller assumed.
        assumed: LengthUnit,
    },
    /// A resource cap was exceeded.
    LimitExceeded {
        /// The resource.
        resource: ImportResource,
        /// The cap.
        limit: usize,
    },
    /// STL parsing failed.
    Stl(StlError),
    /// OBJ parsing failed.
    Obj(ObjError),
    /// 3MF parsing failed.
    ThreeMf(ThreeMfError),
    /// A format registered outside this crate failed.
    Format {
        /// The format's identifier.
        format: &'static str,
        /// Its error.
        source: Box<dyn Error + Send + Sync>,
    },
    /// The mesh could not be built.
    Mesh(MeshError),
}

impl fmt::Display for ModelImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled(e) => e.fmt(f),
            Self::Io(e) => write!(f, "cannot read model: {e}"),
            Self::UnknownFormat {
                extension: Some(extension),
            } => write!(f, "unsupported model file extension '.{extension}'"),
            Self::UnknownFormat { extension: None } => {
                f.write_str("model file has no extension to select its format")
            }
            Self::UnitsRequired { format } => write!(
                f,
                "{format} files declare no length unit; the unit must be given explicitly"
            ),
            Self::UnitConflict { declared, assumed } => write!(
                f,
                "the file declares {declared} units, but {assumed} was assumed"
            ),
            Self::LimitExceeded { resource, limit } => {
                write!(f, "model exceeds the {resource:?} limit of {limit}")
            }
            Self::Stl(e) => e.fmt(f),
            Self::Obj(e) => e.fmt(f),
            Self::ThreeMf(e) => e.fmt(f),
            Self::Format { format, source } => write!(f, "{format}: {source}"),
            Self::Mesh(e) => e.fmt(f),
        }
    }
}

impl Error for ModelImportError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cancelled(e) => Some(e),
            Self::Io(e) => Some(e),
            Self::Stl(e) => Some(e),
            Self::Obj(e) => Some(e),
            Self::ThreeMf(e) => Some(e),
            Self::Format { source, .. } => Some(source.as_ref()),
            Self::Mesh(e) => Some(e),
            Self::UnknownFormat { .. }
            | Self::UnitsRequired { .. }
            | Self::UnitConflict { .. }
            | Self::LimitExceeded { .. } => None,
        }
    }
}

impl From<Cancelled> for ModelImportError {
    fn from(error: Cancelled) -> Self {
        Self::Cancelled(error)
    }
}

impl From<MeshError> for ModelImportError {
    fn from(error: MeshError) -> Self {
        match error {
            MeshError::Cancelled(c) => Self::Cancelled(c),
            e => Self::Mesh(e),
        }
    }
}

/// Why a format could not be registered.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RegistryError {
    /// Another format has this identifier.
    DuplicateId(String),
    /// Another format claims this extension.
    DuplicateExtension(String),
    /// Identifiers and extensions must be nonempty, lowercase ASCII alphanumerics.
    InvalidName(String),
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateId(id) => {
                write!(f, "a model format with id '{id}' is already registered")
            }
            Self::DuplicateExtension(e) => {
                write!(
                    f,
                    "the extension '.{e}' already belongs to a registered format"
                )
            }
            Self::InvalidName(name) => write!(f, "invalid model format id or extension '{name}'"),
        }
    }
}

impl Error for RegistryError {}

/// The registered model formats.
pub struct FormatRegistry {
    formats: Vec<Box<dyn ModelFormat>>,
}

impl fmt::Debug for FormatRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.formats.iter().map(|format| format.id()))
            .finish()
    }
}

impl Default for FormatRegistry {
    fn default() -> Self {
        Self::builtin()
    }
}

impl FormatRegistry {
    /// A registry without formats.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            formats: Vec::new(),
        }
    }

    /// STL, OBJ and 3MF.
    ///
    /// # Panics
    /// Never: the built-in formats have distinct valid identifiers and extensions.
    #[must_use]
    pub fn builtin() -> Self {
        let mut registry = Self::empty();
        for format in [
            Box::new(StlFormat) as Box<dyn ModelFormat>,
            Box::new(ObjFormat),
            Box::new(ThreeMfFormat),
        ] {
            registry
                .register(format)
                .expect("built-in formats have distinct valid names");
        }
        registry
    }

    /// Add a format.
    ///
    /// # Errors
    /// An invalid, or already registered, identifier or extension.
    pub fn register(&mut self, format: Box<dyn ModelFormat>) -> Result<(), RegistryError> {
        let valid = |name: &str| {
            !name.is_empty()
                && name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        };
        for name in std::iter::once(format.id()).chain(format.extensions().iter().copied()) {
            if !valid(name) {
                return Err(RegistryError::InvalidName(name.into()));
            }
        }
        if self.by_id(format.id()).is_some() {
            return Err(RegistryError::DuplicateId(format.id().into()));
        }
        for extension in format.extensions() {
            if self.by_extension(extension).is_some() {
                return Err(RegistryError::DuplicateExtension((*extension).into()));
            }
        }
        self.formats.push(format);
        Ok(())
    }

    /// Registered formats, in registration order.
    pub fn formats(&self) -> impl Iterator<Item = &dyn ModelFormat> {
        self.formats.iter().map(AsRef::as_ref)
    }

    /// The format with this identifier.
    #[must_use]
    pub fn by_id(&self, id: &str) -> Option<&dyn ModelFormat> {
        self.formats().find(|format| format.id() == id)
    }

    /// The format claiming `extension` (without the dot), ignoring ASCII case.
    #[must_use]
    pub fn by_extension(&self, extension: &str) -> Option<&dyn ModelFormat> {
        let extension = extension.to_ascii_lowercase();
        self.formats()
            .find(|format| format.extensions().contains(&extension.as_str()))
    }

    /// The format selected by `path`'s extension.
    ///
    /// # Errors
    /// [`ModelImportError::UnknownFormat`] when no format claims it.
    pub fn for_path(&self, path: &Path) -> Result<&dyn ModelFormat, ModelImportError> {
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        extension
            .as_deref()
            .and_then(|e| self.by_extension(e))
            .ok_or(ModelImportError::UnknownFormat { extension })
    }

    /// Read `path` (bounded by the input limit) and import it with the format its extension
    /// selects.
    ///
    /// # Errors
    /// The failures of [`read_bounded`] and [`Self::import_bytes`].
    pub fn import_path(
        &self,
        path: &Path,
        options: &ModelImportOptions,
        cancellation: &mut Cancellation<'_>,
    ) -> Result<ImportedModel, ModelImportError> {
        let format = self.for_path(path)?;
        let bytes = read_bounded(path, options.limits.input.max_input_bytes, cancellation)?;
        import_bytes(format, &bytes, options, cancellation)
    }

    /// Import `bytes` with `format` (see [`import_bytes`]).
    ///
    /// # Errors
    /// See [`import_bytes`].
    pub fn import_bytes(
        &self,
        format: &dyn ModelFormat,
        bytes: &[u8],
        options: &ModelImportOptions,
        cancellation: &mut Cancellation<'_>,
    ) -> Result<ImportedModel, ModelImportError> {
        import_bytes(format, bytes, options, cancellation)
    }
}

/// Read a whole file, refusing files larger than `max_bytes`.
///
/// # Errors
/// I/O failures, cancellation, and [`ModelImportError::LimitExceeded`] for oversized files.
pub fn read_bounded(
    path: &Path,
    max_bytes: usize,
    cancellation: &mut Cancellation<'_>,
) -> Result<Vec<u8>, ModelImportError> {
    cancellation.check(Checkpoint::ImportStart)?;
    let file = File::open(path).map_err(ModelImportError::Io)?;
    let mut bytes = Vec::new();
    file.take(max_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(ModelImportError::Io)?;
    cancellation.check(Checkpoint::Read)?;
    if bytes.len() > max_bytes {
        return Err(ModelImportError::LimitExceeded {
            resource: ImportResource::InputBytes,
            limit: max_bytes,
        });
    }
    Ok(bytes)
}

/// Parse `bytes` with `format`, resolve the unit and scale the mesh to millimetres.
///
/// # Errors
/// The format's failure, [`ModelImportError::UnitsRequired`] or
/// [`ModelImportError::UnitConflict`], cancellation, and mesh construction failures.
pub fn import_bytes(
    format: &dyn ModelFormat,
    bytes: &[u8],
    options: &ModelImportOptions,
    cancellation: &mut Cancellation<'_>,
) -> Result<ImportedModel, ModelImportError> {
    cancellation.check(Checkpoint::ImportStart)?;
    if bytes.len() > options.limits.input.max_input_bytes {
        return Err(ModelImportError::LimitExceeded {
            resource: ImportResource::InputBytes,
            limit: options.limits.input.max_input_bytes,
        });
    }
    // Refuse before parsing when no unit can be resolved.
    if !format.declares_units() && options.unit.is_none() {
        return Err(ModelImportError::UnitsRequired {
            format: format.name(),
        });
    }
    let source = format.parse(bytes, options, cancellation)?;
    let (unit, unit_origin) = match (source.declared_unit, options.unit) {
        (Some(declared), Some(assumed)) if declared != assumed => {
            return Err(ModelImportError::UnitConflict { declared, assumed });
        }
        (Some(declared), _) => (declared, UnitOrigin::Declared),
        (None, Some(assumed)) => (assumed, UnitOrigin::Assumed),
        (None, None) => {
            return Err(ModelImportError::UnitsRequired {
                format: format.name(),
            })
        }
    };
    let mesh = if unit == LengthUnit::Millimeter {
        source.mesh
    } else {
        scaled(&source.mesh, unit.millimeters(), cancellation)?
    };
    cancellation.check(Checkpoint::ImportComplete)?;
    Ok(ImportedModel {
        mesh,
        format: format.id(),
        unit,
        unit_origin,
        objects: source.objects,
        notes: source.notes,
        diagnostics: source.diagnostics,
    })
}

/// `mesh` with every coordinate multiplied by `factor`; indices are unchanged.
fn scaled(
    mesh: &TriangleMesh,
    factor: f64,
    cancellation: &mut Cancellation<'_>,
) -> Result<TriangleMesh, ModelImportError> {
    let mut vertices = Vec::new();
    vertices
        .try_reserve_exact(mesh.vertices().len())
        .map_err(|_| MeshError::Allocation {
            context: "scaled vertices",
        })?;
    vertices.extend(
        mesh.vertices()
            .iter()
            .map(|p| Point3::new(p.x * factor, p.y * factor, p.z * factor)),
    );
    let mut triangles = Vec::new();
    triangles
        .try_reserve_exact(mesh.triangles().len())
        .map_err(|_| MeshError::Allocation {
            context: "scaled triangles",
        })?;
    triangles.extend_from_slice(mesh.triangles());
    Ok(TriangleMesh::try_new_with_cancellation(
        vertices,
        triangles,
        cancellation,
    )?)
}

/// STL through the existing importer (binary or ASCII, detected as before).
#[derive(Clone, Copy, Debug, Default)]
pub struct StlFormat;

impl ModelFormat for StlFormat {
    fn id(&self) -> &'static str {
        "stl"
    }
    fn name(&self) -> &'static str {
        "STL"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["stl"]
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
        // File coordinates are taken as they are; the registry applies the resolved unit.
        let mut stl = StlOptions::new(StlUnitPolicy::AssumeMillimeters);
        stl.limits = options.limits.input;
        stl.non_finite = options.non_finite;
        let imported =
            import_stl_bytes_with_cancellation(bytes, stl, cancellation).map_err(|e| match e {
                StlError::Cancelled(c) => ModelImportError::Cancelled(c),
                e => ModelImportError::Stl(e),
            })?;
        let mut notes = Vec::new();
        if imported.dropped.facets > 0 {
            notes.push(ImportNote::NonFiniteFacetsDropped(imported.dropped));
        }
        Ok(SourceModel {
            mesh: imported.mesh,
            declared_unit: None,
            objects: 1,
            notes,
            diagnostics: imported.diagnostics,
        })
    }
}

/// Builds a mesh from triangles given as positions, giving equal positions one vertex by exact
/// coordinate bits (signed zero normalized), in first-occurrence order, as STL import does.
pub(crate) struct WeldingBuilder {
    corners: Vec<Point3>,
    limits: ImportLimits,
}

impl WeldingBuilder {
    pub(crate) fn new(limits: ImportLimits) -> Self {
        Self {
            corners: Vec::new(),
            limits,
        }
    }

    /// Triangles added so far.
    pub(crate) fn triangle_count(&self) -> usize {
        self.corners.len() / 3
    }

    /// Add one triangle with corners in order. Coordinates must be finite.
    pub(crate) fn triangle(&mut self, corners: [Point3; 3]) -> Result<(), ModelImportError> {
        if self.triangle_count() >= self.limits.max_triangles {
            return Err(ModelImportError::LimitExceeded {
                resource: ImportResource::Triangles,
                limit: self.limits.max_triangles,
            });
        }
        self.corners
            .try_reserve(3)
            .map_err(|_| MeshError::Allocation {
                context: "triangle corners",
            })?;
        self.corners.extend(corners);
        Ok(())
    }

    /// Add triangles given as consecutive corner triples.
    pub(crate) fn extend(&mut self, corners: Vec<Point3>) -> Result<(), ModelImportError> {
        if self.triangle_count() + corners.len() / 3 > self.limits.max_triangles {
            return Err(ModelImportError::LimitExceeded {
                resource: ImportResource::Triangles,
                limit: self.limits.max_triangles,
            });
        }
        if self.corners.is_empty() {
            self.corners = corners;
        } else {
            self.corners
                .try_reserve(corners.len())
                .map_err(|_| MeshError::Allocation {
                    context: "triangle corners",
                })?;
            self.corners.extend(corners);
        }
        Ok(())
    }

    /// Weld the corners on all cores (TD-058) and return the mesh and its geometry diagnostics.
    pub(crate) fn finish(
        self,
        cancellation: &mut Cancellation<'_>,
    ) -> Result<(TriangleMesh, Vec<MeshDiagnostic>), ModelImportError> {
        cancellation.check(Checkpoint::Deduplication)?;
        let corners = self.corners;
        let welded = weld(corners.len(), &|c| corners[c], self.limits.max_vertices).map_err(
            |e| match e {
                WeldError::TooManyVertices => ModelImportError::LimitExceeded {
                    resource: ImportResource::Vertices,
                    limit: self.limits.max_vertices,
                },
                WeldError::Allocation => ModelImportError::Mesh(MeshError::Allocation {
                    context: "vertex welding",
                }),
            },
        )?;
        drop(corners);
        let mut triangles: Vec<Triangle> = Vec::new();
        triangles
            .try_reserve_exact(welded.corner_vertex.len() / 3)
            .map_err(|_| MeshError::Allocation {
                context: "mesh triangles",
            })?;
        triangles.extend(welded.corner_vertex.as_chunks::<3>().0.iter().copied());
        drop(welded.corner_vertex);
        let mesh =
            TriangleMesh::try_new_with_cancellation(welded.vertices, triangles, cancellation)?;
        let diagnostics = mesh.diagnostics_with_cancellation(cancellation)?;
        Ok((mesh, diagnostics))
    }
}
