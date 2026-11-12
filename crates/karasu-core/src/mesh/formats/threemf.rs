//! 3MF packages (3MF Core Specification 1.4 with the Production extension; TD-057).
//!
//! **Package:**
//! - The ZIP package is read with the bounded in-memory reader.
//! - The model part is found through the OPC start-part relationship in `_rels/.rels`, never by
//!   a fixed name.
//! - Parts are parsed as streaming XML: UTF-8 only, document type declarations refused, and
//!   only the predefined entities and character references expanded.
//!
//! **Model:**
//! - **Units:** the `unit` attribute (micron, millimeter, centimeter, inch, foot, meter;
//!   millimeter by default) is declared to the caller.
//! - **Required extensions:** resolved through their namespace prefixes. Production and Materials
//!   are supported; colour and material properties never change geometry and are ignored. Any
//!   other required extension refuses the file, as the specification demands.
//! - **Objects:** mesh objects and component objects (with transforms) form the resources.
//!   Objects must be defined before use, and ids must be unique within a part.
//! - **Build:** only build items are output. Each instance is transformed by its component
//!   transforms, then its item transform, all in the specification's row-vector convention
//!   (`x' = m00·x + m10·y + m20·z + m30`). A mirroring transform (negative determinant) reverses
//!   the winding, so the instance stays outward.
//! - **Production extension:** a `p:path` on build items and on root-model components refers to
//!   objects in another model part of the package. That part's build is ignored, and it must use
//!   the root's unit and no further paths.
//!
//! **Object types:**
//! - `model` and `solidsupport` objects are sliced.
//! - `support` and `surface` objects are not solids; they are skipped and reported.
//! - An `other` object referenced for output is an error.
//!
//! **Validation:** triangle indices are range-checked, and coordinates and transforms must be
//! finite. Counts, nesting depth and expanded triangles are capped before the mesh is built. All
//! instances form one mesh, welded like every format.

use super::zip::{archive_error, Archive, ArchiveError, ArchiveLimits};
use super::{
    ImportNote, LengthUnit, ModelFormat, ModelImportError, ModelImportOptions, ModelLimits,
    SourceModel, WeldingBuilder,
};
use crate::cancellation::{Cancellation, Checkpoint};
use crate::geometry::Point3;
use crate::mesh::ImportResource;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::{QName, ResolveResult};
use quick_xml::reader::NsReader;
use quick_xml::XmlVersion;
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::io::{self, BufRead, BufReader, Read};
use std::sync::mpsc::{sync_channel, Receiver};
use std::thread;
use std::{error::Error, fmt};

const CORE: &str = "http://schemas.microsoft.com/3dmanufacturing/core/2015/02";
const PRODUCTION: &str = "http://schemas.microsoft.com/3dmanufacturing/production/2015/06";
const MATERIAL: &str = "http://schemas.microsoft.com/3dmanufacturing/material/2015/02";
const RELATIONSHIPS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const START_PART: &str = "http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel";

/// What is wrong inside a model part.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ThreeMfProblem {
    /// A required attribute is missing.
    MissingAttribute(&'static str),
    /// An attribute value is malformed or not finite.
    InvalidValue(&'static str),
    /// A core element appears where it is not allowed.
    UnexpectedElement(String),
    /// Two objects have the same id.
    DuplicateObject(u32),
    /// A reference to an object not defined before it.
    UnknownObject(u32),
    /// A triangle index outside its object's vertices.
    IndexOutOfRange {
        /// The object.
        object: u32,
    },
    /// An object of type `other` is referenced for output.
    OtherObjectInBuild(u32),
    /// Components nest deeper than the limit.
    TooDeep {
        /// The limit.
        limit: usize,
    },
    /// A referenced part declares a different unit than the root model.
    UnitMismatch,
    /// A `p:path` inside a non-root part (the Production extension allows one level).
    NestedPath,
    /// The root model has no build items.
    EmptyBuild,
    /// An object holds more than one mesh, or both a mesh and components.
    MixedObject(u32),
}

/// Why a 3MF package cannot be read.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ThreeMfError {
    /// The ZIP container is invalid.
    Archive(ArchiveError),
    /// A part is missing: the relationships, the start part, or a `p:path` target.
    MissingPart(String),
    /// The package declares no (or several) 3D model start parts.
    StartPart,
    /// A part is not well-formed XML.
    Xml {
        /// The part.
        part: String,
        /// The parser's message.
        message: String,
    },
    /// A part has a document type declaration, which 3MF prohibits.
    DocumentType(String),
    /// A part declares an encoding other than UTF-8.
    Encoding(String),
    /// The model `unit` is not one of the specified values.
    UnknownUnit(String),
    /// A required extension this reader does not implement.
    UnsupportedExtension(String),
    /// Invalid model content.
    Model {
        /// The part.
        part: String,
        /// What is wrong.
        problem: ThreeMfProblem,
    },
}

impl fmt::Display for ThreeMfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Archive(e) => write!(f, "3MF package: {e}"),
            Self::MissingPart(part) => write!(f, "3MF package has no part '{part}'"),
            Self::StartPart => {
                f.write_str("3MF package must declare exactly one 3D model start part")
            }
            Self::Xml { part, message } => {
                write!(f, "3MF part '{part}' is not valid XML: {message}")
            }
            Self::DocumentType(part) => {
                write!(
                    f,
                    "3MF part '{part}' has a document type declaration (prohibited)"
                )
            }
            Self::Encoding(part) => write!(f, "3MF part '{part}' is not UTF-8"),
            Self::UnknownUnit(unit) => write!(f, "3MF model has unknown unit '{unit}'"),
            Self::UnsupportedExtension(ns) => {
                write!(f, "3MF model requires an unsupported extension: {ns}")
            }
            Self::Model { part, problem } => {
                write!(f, "3MF part '{part}': ")?;
                match problem {
                    ThreeMfProblem::MissingAttribute(a) => write!(f, "missing attribute '{a}'"),
                    ThreeMfProblem::InvalidValue(a) => write!(f, "invalid value of '{a}'"),
                    ThreeMfProblem::UnexpectedElement(e) => write!(f, "unexpected element '{e}'"),
                    ThreeMfProblem::DuplicateObject(id) => write!(f, "duplicate object id {id}"),
                    ThreeMfProblem::UnknownObject(id) => {
                        write!(f, "reference to object {id}, not defined before it")
                    }
                    ThreeMfProblem::IndexOutOfRange { object } => {
                        write!(f, "object {object} has a triangle index out of range")
                    }
                    ThreeMfProblem::OtherObjectInBuild(id) => {
                        write!(f, "object {id} of type 'other' is referenced for output")
                    }
                    ThreeMfProblem::TooDeep { limit } => {
                        write!(f, "components nest deeper than {limit} levels")
                    }
                    ThreeMfProblem::UnitMismatch => {
                        f.write_str("the part's unit differs from the root model's")
                    }
                    ThreeMfProblem::NestedPath => {
                        f.write_str("a non-root part refers to another part (p:path)")
                    }
                    ThreeMfProblem::EmptyBuild => f.write_str("the model has no build items"),
                    ThreeMfProblem::MixedObject(id) => write!(
                        f,
                        "object {id} holds more than one mesh, or both a mesh and components"
                    ),
                }
            }
        }
    }
}

impl Error for ThreeMfError {}

impl From<ThreeMfError> for ModelImportError {
    fn from(error: ThreeMfError) -> Self {
        Self::ThreeMf(error)
    }
}

/// The 3MF format.
#[derive(Clone, Copy, Debug, Default)]
pub struct ThreeMfFormat;

impl ModelFormat for ThreeMfFormat {
    fn id(&self) -> &'static str {
        "3mf"
    }
    fn name(&self) -> &'static str {
        "3MF"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["3mf"]
    }
    fn declares_units(&self) -> bool {
        true
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

/// A 3MF transform in the specification's row-vector form: `rows[0..3]` is the linear part and
/// `rows[3]` the translation; a point `p` maps to `p·L + t`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Transform {
    rows: [[f64; 3]; 4],
}

impl Transform {
    const IDENTITY: Self = Self {
        rows: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [0., 0., 0.]],
    };

    fn apply(&self, p: [f64; 3]) -> [f64; 3] {
        let r = &self.rows;
        [0, 1, 2].map(|j| p[0] * r[0][j] + p[1] * r[1][j] + p[2] * r[2][j] + r[3][j])
    }

    /// `self` first, then `then`.
    fn then(&self, then: &Self) -> Self {
        let mut rows = [[0.; 3]; 4];
        for (i, row) in rows.iter_mut().enumerate().take(3) {
            for (j, value) in row.iter_mut().enumerate() {
                *value = (0..3).map(|k| self.rows[i][k] * then.rows[k][j]).sum();
            }
        }
        rows[3] = then.apply(self.rows[3]);
        Self { rows }
    }

    fn determinant(&self) -> f64 {
        let m = &self.rows;
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Model,
    SolidSupport,
    Support,
    Surface,
    Other,
}

struct Reference {
    object: u32,
    transform: Transform,
    /// Normalized target part, for `p:path`.
    path: Option<String>,
}

enum Content {
    Mesh {
        vertices: Vec<[f64; 3]>,
        triangles: Vec<[u32; 3]>,
    },
    Components(Vec<Reference>),
}

struct Object {
    kind: Kind,
    content: Content,
}

/// One parsed model part.
struct Part {
    unit: LengthUnit,
    objects: HashMap<u32, Object>,
    build: Vec<Reference>,
    properties: bool,
}

/// Normalize an OPC part name: absolute, with a leading slash.
fn absolute(target: &str, base: &str) -> String {
    if target.starts_with('/') {
        target.to_owned()
    } else {
        let directory = base.rsplit_once('/').map_or("", |(d, _)| d);
        format!("{directory}/{target}")
    }
}

fn xml_error(part: &str, error: &quick_xml::Error) -> ModelImportError {
    if let quick_xml::Error::Io(io) = error {
        return ThreeMfError::Archive(archive_error(io)).into();
    }
    ThreeMfError::Xml {
        part: part.to_owned(),
        message: error.to_string(),
    }
    .into()
}

/// A streaming reader of one part.
fn reader<'a>(
    archive: &Archive<'a>,
    part: &str,
) -> Result<NsReader<BufReader<super::zip::EntryReader<'a>>>, ModelImportError> {
    let entry = archive
        .find(part)
        .ok_or_else(|| ThreeMfError::MissingPart(part.to_owned()))?;
    let stream = archive.open(entry).map_err(ThreeMfError::Archive)?;
    let mut reader = NsReader::from_reader(BufReader::with_capacity(64 * 1024, stream));
    reader.config_mut().trim_text(true);
    Ok(reader)
}

/// Common event handling: declarations must be UTF-8 and document types are refused.
fn check_prolog(event: &Event<'_>, part: &str) -> Result<(), ModelImportError> {
    match event {
        Event::DocType(_) => Err(ThreeMfError::DocumentType(part.to_owned()).into()),
        Event::Decl(declaration) => match declaration.encoding() {
            Some(Ok(encoding)) if !encoding.eq_ignore_ascii_case("utf-8") => {
                Err(ThreeMfError::Encoding(part.to_owned()).into())
            }
            Some(Err(_)) => Err(ThreeMfError::Encoding(part.to_owned()).into()),
            _ => Ok(()),
        },
        _ => Ok(()),
    }
}

/// The start part named by `_rels/.rels`.
fn start_part(archive: &Archive<'_>) -> Result<String, ModelImportError> {
    const RELS: &str = "_rels/.rels";
    let mut reader = reader(archive, RELS)?;
    let mut buffer = Vec::new();
    let mut targets = Vec::new();
    loop {
        buffer.clear();
        let (ns, event) = reader
            .read_resolved_event_into(&mut buffer)
            .map_err(|e| xml_error(RELS, &e))?;
        check_prolog(&event, RELS)?;
        match event {
            Event::Start(e) | Event::Empty(e)
                if matches!(ns, ResolveResult::Bound(n) if n.as_ref() == RELATIONSHIPS)
                    && e.local_name().as_ref() == "Relationship" =>
            {
                let ty = attribute(&e, "Type", RELS)?;
                if ty.as_deref() == Some(START_PART) {
                    let target =
                        attribute(&e, "Target", RELS)?.ok_or_else(|| ThreeMfError::Model {
                            part: RELS.into(),
                            problem: ThreeMfProblem::MissingAttribute("Target"),
                        })?;
                    targets.push(absolute(&target, "/"));
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    match <[String; 1]>::try_from(targets) {
        Ok([target]) => Ok(target),
        Err(_) => Err(ThreeMfError::StartPart.into()),
    }
}

/// The values of the unprefixed attributes `names`, in one pass; borrowed unless escaped.
fn attribute_set<'e, const N: usize>(
    element: &'e BytesStart<'_>,
    names: [&str; N],
    part: &str,
) -> Result<[Option<Cow<'e, str>>; N], ModelImportError> {
    let mut values = [const { None }; N];
    for attribute in element.attributes() {
        let attribute = attribute.map_err(|e| ThreeMfError::Xml {
            part: part.to_owned(),
            message: e.to_string(),
        })?;
        if let Some(slot) = names.iter().position(|&n| attribute.key.as_ref() == n) {
            // Without references or whitespace to normalize, the raw value is the value.
            let plain = !attribute
                .value
                .bytes()
                .any(|b| matches!(b, b'&' | b'\t' | b'\n' | b'\r'));
            values[slot] = Some(if plain {
                attribute.value
            } else {
                attribute
                    .normalized_value(XmlVersion::Implicit1_0)
                    .map_err(|e| ThreeMfError::Xml {
                        part: part.to_owned(),
                        message: e.to_string(),
                    })?
            });
        }
    }
    Ok(values)
}

/// The value of the unprefixed attribute `name`, unescaped.
fn attribute(
    element: &BytesStart<'_>,
    name: &str,
    part: &str,
) -> Result<Option<String>, ModelImportError> {
    for attribute in element.attributes() {
        let attribute = attribute.map_err(|e| ThreeMfError::Xml {
            part: part.to_owned(),
            message: e.to_string(),
        })?;
        if attribute.key.as_ref() == name {
            let value = attribute
                .normalized_value(XmlVersion::Implicit1_0)
                .map_err(|e| ThreeMfError::Xml {
                    part: part.to_owned(),
                    message: e.to_string(),
                })?;
            return Ok(Some(value.into_owned()));
        }
    }
    Ok(None)
}

fn invalid(part: &str, problem: ThreeMfProblem) -> ModelImportError {
    ThreeMfError::Model {
        part: part.to_owned(),
        problem,
    }
    .into()
}

/// A finite decimal number (no `inf`/`nan` words).
fn number(text: &str, name: &'static str, part: &str) -> Result<f64, ModelImportError> {
    let valid = !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.' | b'e' | b'E'));
    text.parse::<f64>()
        .ok()
        .filter(|v| valid && v.is_finite())
        .ok_or_else(|| invalid(part, ThreeMfProblem::InvalidValue(name)))
}

fn required(
    element: &BytesStart<'_>,
    name: &'static str,
    part: &str,
) -> Result<String, ModelImportError> {
    attribute(element, name, part)?
        .ok_or_else(|| invalid(part, ThreeMfProblem::MissingAttribute(name)))
}

fn index(
    element: &BytesStart<'_>,
    name: &'static str,
    part: &str,
) -> Result<u32, ModelImportError> {
    required(element, name, part)?
        .trim()
        .parse()
        .map_err(|_| invalid(part, ThreeMfProblem::InvalidValue(name)))
}

fn transform(element: &BytesStart<'_>, part: &str) -> Result<Transform, ModelImportError> {
    let Some(text) = attribute(element, "transform", part)? else {
        return Ok(Transform::IDENTITY);
    };
    let values: Vec<f64> = text
        .split_ascii_whitespace()
        .map(|t| number(t, "transform", part))
        .collect::<Result<_, _>>()?;
    let values: [f64; 12] = values
        .try_into()
        .map_err(|_| invalid(part, ThreeMfProblem::InvalidValue("transform")))?;
    let mut rows = [[0.; 3]; 4];
    for (i, row) in rows.iter_mut().enumerate() {
        row.copy_from_slice(&values[3 * i..3 * i + 3]);
    }
    Ok(Transform { rows })
}

/// The production `p:path` of a build item or component, normalized; any other attribute in the
/// production namespace (such as `p:UUID`) is ignored.
fn production_path(
    element: &BytesStart<'_>,
    reader: &NsReader<impl std::io::BufRead>,
    part: &str,
) -> Result<Option<String>, ModelImportError> {
    for attribute in element.attributes() {
        let attribute = attribute.map_err(|e| ThreeMfError::Xml {
            part: part.to_owned(),
            message: e.to_string(),
        })?;
        let (ns, local) = reader.resolver().resolve_attribute(attribute.key);
        if matches!(ns, ResolveResult::Bound(n) if n.as_ref() == PRODUCTION)
            && local.as_ref() == "path"
        {
            let value = attribute
                .normalized_value(XmlVersion::Implicit1_0)
                .map_err(|e| ThreeMfError::Xml {
                    part: part.to_owned(),
                    message: e.to_string(),
                })?;
            return Ok(Some(absolute(&value, part)));
        }
    }
    Ok(None)
}

/// Where the model parser is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Document,
    Model,
    Resources,
    Object,
    Mesh,
    Vertices,
    Triangles,
    Components,
    Build,
}

/// Bytes per inflated chunk handed from the inflating thread to the parser.
const CHUNK: usize = 1 << 20;

/// A reader of chunks inflated on another thread.
struct ChannelReader {
    chunks: Receiver<Result<Vec<u8>, ArchiveError>>,
    current: Vec<u8>,
    position: usize,
}

impl Read for ChannelReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        while self.position == self.current.len() {
            match self.chunks.recv() {
                Ok(Ok(chunk)) => (self.current, self.position) = (chunk, 0),
                Ok(Err(error)) => return Err(io::Error::new(io::ErrorKind::InvalidData, error)),
                // The inflating thread finished after a complete, verified entry.
                Err(_) => return Ok(0),
            }
        }
        let count = out.len().min(self.current.len() - self.position);
        out[..count].copy_from_slice(&self.current[self.position..self.position + count]);
        self.position += count;
        Ok(count)
    }
}

/// Parse a model part while a second thread inflates it (TD-058): inflation and XML parsing
/// overlap, with at most a few chunks in flight.
fn parse_part(
    archive: &Archive<'_>,
    part: &str,
    root: bool,
    limits: &ModelLimits,
    cancellation: &mut Cancellation<'_>,
) -> Result<Part, ModelImportError> {
    let entry = archive
        .find(part)
        .ok_or_else(|| ThreeMfError::MissingPart(part.to_owned()))?;
    let mut stream = archive.open(entry).map_err(ThreeMfError::Archive)?;
    let (sender, chunks) = sync_channel::<Result<Vec<u8>, ArchiveError>>(4);
    thread::scope(|scope| {
        scope.spawn(move || loop {
            let mut chunk = Vec::new();
            if chunk.try_reserve_exact(CHUNK).is_err() {
                let _ = sender.send(Err(ArchiveError::Malformed("inflate buffer allocation")));
                return;
            }
            chunk.resize(CHUNK, 0);
            let mut filled = 0;
            while filled < CHUNK {
                match stream.read(&mut chunk[filled..]) {
                    Ok(0) => break,
                    Ok(n) => filled += n,
                    Err(e) => {
                        let _ = sender.send(Err(archive_error(&e)));
                        return;
                    }
                }
            }
            chunk.truncate(filled);
            // An empty chunk means the entry ended (size and CRC verified); a closed channel
            // means the parser stopped early.
            if filled == 0 || sender.send(Ok(chunk)).is_err() {
                return;
            }
        });
        let source = ChannelReader {
            chunks,
            current: Vec::new(),
            position: 0,
        };
        let mut reader = NsReader::from_reader(BufReader::with_capacity(64 * 1024, source));
        reader.config_mut().trim_text(true);
        // Dropping the reader closes the channel, which stops the inflating thread.
        parse_stream(&mut reader, part, root, limits, cancellation)
    })
}

#[allow(clippy::too_many_lines)] // One streaming state machine over the model schema.
fn parse_stream(
    reader: &mut NsReader<impl BufRead>,
    part: &str,
    root: bool,
    limits: &ModelLimits,
    cancellation: &mut Cancellation<'_>,
) -> Result<Part, ModelImportError> {
    let mut buffer = Vec::new();
    let mut state = State::Document;
    let mut model = Part {
        unit: LengthUnit::Millimeter,
        objects: HashMap::new(),
        build: Vec::new(),
        properties: false,
    };
    let mut current: Option<(u32, Object)> = None;
    // Whether the current object already has its mesh or components element.
    let mut content_seen = false;
    let mut triangles_seen = 0usize;
    let mut events = 0usize;
    let mut root_seen = false;
    loop {
        buffer.clear();
        events += 1;
        cancellation.poll(Checkpoint::AsciiParsing, events)?;
        let (ns, event) = reader
            .read_resolved_event_into(&mut buffer)
            .map_err(|e| xml_error(part, &e))?;
        check_prolog(&event, part)?;
        let core = matches!(ns, ResolveResult::Bound(n) if n.as_ref() == CORE);
        let (element, empty) = match event {
            Event::Start(e) => (e, false),
            Event::Empty(e) => (e, true),
            Event::End(_) => {
                if core {
                    state = match state {
                        State::Model | State::Document => State::Document,
                        State::Resources | State::Build => State::Model,
                        State::Object => {
                            if let Some((id, object)) = current.take() {
                                model.objects.insert(id, object);
                            }
                            State::Resources
                        }
                        State::Mesh | State::Components => State::Object,
                        State::Vertices | State::Triangles => State::Mesh,
                    };
                }
                continue;
            }
            Event::Eof => {
                if !root_seen || state != State::Document {
                    return Err(ThreeMfError::Xml {
                        part: part.to_owned(),
                        message: "document ends before the model element is closed".into(),
                    }
                    .into());
                }
                break;
            }
            _ => continue,
        };
        if !core {
            // Elements of other namespaces (materials, vendor data) carry no core geometry.
            let material = matches!(ns, ResolveResult::Bound(n) if n.as_ref() == MATERIAL);
            model.properties |= material;
            if !empty {
                let end = element.to_end().into_owned();
                reader
                    .read_to_end_into(end.name(), &mut Vec::new())
                    .map_err(|e| xml_error(part, &e))?;
            }
            continue;
        }
        let name = element.local_name();
        let name = name.as_ref();
        let unexpected = || invalid(part, ThreeMfProblem::UnexpectedElement(name.to_owned()));
        match (state, name) {
            (State::Document, "model") if !root_seen => {
                root_seen = true;
                if let Some(unit) = attribute(&element, "unit", part)? {
                    model.unit = LengthUnit::ALL
                        .into_iter()
                        .find(|u| u.name() == unit)
                        .ok_or(ThreeMfError::UnknownUnit(unit))?;
                }
                if let Some(required) = attribute(&element, "requiredextensions", part)? {
                    for prefix in required.split_ascii_whitespace() {
                        let qualified = format!("{prefix}:x");
                        let (ns, _) = reader.resolver().resolve_attribute(QName(&qualified));
                        let namespace = match ns {
                            ResolveResult::Bound(n) => n.as_ref().to_owned(),
                            _ => prefix.to_owned(),
                        };
                        if namespace != PRODUCTION && namespace != MATERIAL {
                            return Err(ThreeMfError::UnsupportedExtension(namespace).into());
                        }
                    }
                }
                if !empty {
                    state = State::Model;
                }
            }
            (State::Model, "resources") if !empty => state = State::Resources,
            (State::Model, "build") if !empty => state = State::Build,
            (_, "metadata" | "metadatagroup") => {
                if !empty {
                    let end = element.to_end().into_owned();
                    reader
                        .read_to_end_into(end.name(), &mut Vec::new())
                        .map_err(|e| xml_error(part, &e))?;
                }
            }
            (State::Resources, "basematerials") => {
                model.properties = true;
                if !empty {
                    let end = element.to_end().into_owned();
                    reader
                        .read_to_end_into(end.name(), &mut Vec::new())
                        .map_err(|e| xml_error(part, &e))?;
                }
            }
            (State::Resources, "object") => {
                let id = index(&element, "id", part)?;
                if model.objects.contains_key(&id) {
                    return Err(invalid(part, ThreeMfProblem::DuplicateObject(id)));
                }
                let kind = match attribute(&element, "type", part)?.as_deref() {
                    None | Some("model") => Kind::Model,
                    Some("solidsupport") => Kind::SolidSupport,
                    Some("support") => Kind::Support,
                    Some("surface") => Kind::Surface,
                    Some("other") => Kind::Other,
                    Some(_) => return Err(invalid(part, ThreeMfProblem::InvalidValue("type"))),
                };
                model.properties |= attribute(&element, "pid", part)?.is_some();
                let object = Object {
                    kind,
                    content: Content::Components(Vec::new()),
                };
                if empty {
                    model.objects.insert(id, object);
                } else {
                    current = Some((id, object));
                    content_seen = false;
                    state = State::Object;
                }
            }
            (State::Object, "mesh") => {
                if std::mem::replace(&mut content_seen, true) {
                    let id = current.as_ref().map_or(0, |(id, _)| *id);
                    return Err(invalid(part, ThreeMfProblem::MixedObject(id)));
                }
                if let Some((_, object)) = &mut current {
                    object.content = Content::Mesh {
                        vertices: Vec::new(),
                        triangles: Vec::new(),
                    };
                }
                if !empty {
                    state = State::Mesh;
                }
            }
            (State::Object, "components") => {
                if std::mem::replace(&mut content_seen, true) {
                    let id = current.as_ref().map_or(0, |(id, _)| *id);
                    return Err(invalid(part, ThreeMfProblem::MixedObject(id)));
                }
                if !empty {
                    state = State::Components;
                }
            }
            (State::Mesh, "vertices") if !empty => state = State::Vertices,
            (State::Mesh, "triangles") if !empty => state = State::Triangles,
            // Empty containers hold nothing.
            (State::Model, "resources" | "build") | (State::Mesh, "vertices" | "triangles") => {}
            (State::Vertices, "vertex") => {
                let values = attribute_set(&element, ["x", "y", "z"], part)?;
                let mut point = [0.0; 3];
                for ((value, axis), slot) in values.iter().zip(["x", "y", "z"]).zip(&mut point) {
                    let value = value
                        .as_deref()
                        .ok_or_else(|| invalid(part, ThreeMfProblem::MissingAttribute(axis)))?;
                    *slot = number(value.trim(), axis, part)?;
                }
                if let Some((
                    _,
                    Object {
                        content: Content::Mesh { vertices, .. },
                        ..
                    },
                )) = &mut current
                {
                    if vertices.len() >= limits.input.max_vertices {
                        return Err(ModelImportError::LimitExceeded {
                            resource: ImportResource::Vertices,
                            limit: limits.input.max_vertices,
                        });
                    }
                    vertices.try_reserve(1).map_err(|_| allocation())?;
                    vertices.push(point);
                }
            }
            (State::Triangles, "triangle") => {
                let values = attribute_set(&element, ["v1", "v2", "v3", "pid", "p1"], part)?;
                let mut triangle = [0u32; 3];
                for ((value, name), slot) in
                    values.iter().zip(["v1", "v2", "v3"]).zip(&mut triangle)
                {
                    let value = value
                        .as_deref()
                        .ok_or_else(|| invalid(part, ThreeMfProblem::MissingAttribute(name)))?;
                    *slot = value
                        .trim()
                        .parse()
                        .map_err(|_| invalid(part, ThreeMfProblem::InvalidValue(name)))?;
                }
                model.properties |= values[3].is_some() || values[4].is_some();
                triangles_seen += 1;
                if triangles_seen > limits.input.max_triangles {
                    return Err(ModelImportError::LimitExceeded {
                        resource: ImportResource::Triangles,
                        limit: limits.input.max_triangles,
                    });
                }
                if let Some((
                    _,
                    Object {
                        content: Content::Mesh { triangles, .. },
                        ..
                    },
                )) = &mut current
                {
                    triangles.try_reserve(1).map_err(|_| allocation())?;
                    triangles.push(triangle);
                }
            }
            (State::Components, "component") => {
                let reference = Reference {
                    object: index(&element, "objectid", part)?,
                    transform: transform(&element, part)?,
                    path: production_path(&element, reader, part)?,
                };
                if reference.path.is_some() && !root {
                    return Err(invalid(part, ThreeMfProblem::NestedPath));
                }
                if reference.path.is_none() && !model.objects.contains_key(&reference.object) {
                    return Err(invalid(
                        part,
                        ThreeMfProblem::UnknownObject(reference.object),
                    ));
                }
                if let Some((
                    _,
                    Object {
                        content: Content::Components(components),
                        ..
                    },
                )) = &mut current
                {
                    components.try_reserve(1).map_err(|_| allocation())?;
                    components.push(reference);
                }
            }
            (State::Build, "item") => {
                let reference = Reference {
                    object: index(&element, "objectid", part)?,
                    transform: transform(&element, part)?,
                    path: production_path(&element, reader, part)?,
                };
                if root
                    && reference.path.is_none()
                    && !model.objects.contains_key(&reference.object)
                {
                    return Err(invalid(
                        part,
                        ThreeMfProblem::UnknownObject(reference.object),
                    ));
                }
                model.build.try_reserve(1).map_err(|_| allocation())?;
                model.build.push(reference);
            }
            _ => return Err(unexpected()),
        }
    }
    // Validate triangle indices once per object.
    for (&id, object) in &model.objects {
        if let Content::Mesh {
            vertices,
            triangles,
        } = &object.content
        {
            if triangles
                .iter()
                .flatten()
                .any(|&v| v as usize >= vertices.len())
            {
                return Err(invalid(
                    part,
                    ThreeMfProblem::IndexOutOfRange { object: id },
                ));
            }
        }
    }
    Ok(model)
}

fn allocation() -> ModelImportError {
    ModelImportError::Mesh(crate::mesh::MeshError::Allocation {
        context: "3MF model",
    })
}

/// Parsed parts, the root first, keyed by normalized lowercase name.
struct Package {
    root: String,
    parts: BTreeMap<String, Part>,
}

impl Package {
    fn part(&self, name: &str) -> &Part {
        &self.parts[&name.to_ascii_lowercase()]
    }
}

/// Counters gathered while placing instances.
#[derive(Default)]
struct Placement {
    instances: usize,
    skipped: BTreeMap<&'static str, usize>,
    mirrored: usize,
}

/// Triangles one instance of `object` in `part` expands to (saturating), checking types, depth
/// and references.
fn expanded(
    package: &Package,
    part: &str,
    object: u32,
    depth: usize,
    limits: &ModelLimits,
) -> Result<usize, ModelImportError> {
    if depth > limits.max_component_depth {
        return Err(invalid(
            part,
            ThreeMfProblem::TooDeep {
                limit: limits.max_component_depth,
            },
        ));
    }
    let entry = package
        .part(part)
        .objects
        .get(&object)
        .ok_or_else(|| invalid(part, ThreeMfProblem::UnknownObject(object)))?;
    match (&entry.content, entry.kind) {
        (_, Kind::Other) => Err(invalid(part, ThreeMfProblem::OtherObjectInBuild(object))),
        (_, Kind::Support | Kind::Surface) => Ok(0),
        (Content::Mesh { triangles, .. }, _) => Ok(triangles.len()),
        (Content::Components(components), _) => {
            let mut total = 0usize;
            for c in components {
                let target = c.path.as_deref().unwrap_or(part);
                total =
                    total.saturating_add(expanded(package, target, c.object, depth + 1, limits)?);
            }
            Ok(total)
        }
    }
}

/// Emit one instance of `object` in `part` under `transform`.
fn place(
    package: &Package,
    part: &str,
    object: u32,
    transform: &Transform,
    builder: &mut WeldingBuilder,
    placement: &mut Placement,
    cancellation: &mut Cancellation<'_>,
) -> Result<(), ModelImportError> {
    let entry = &package.part(part).objects[&object];
    match (&entry.content, entry.kind) {
        (_, Kind::Support) => *placement.skipped.entry("support").or_default() += 1,
        (_, Kind::Surface) => *placement.skipped.entry("surface").or_default() += 1,
        (
            Content::Mesh {
                vertices,
                triangles,
            },
            _,
        ) => {
            placement.instances += 1;
            let mirrored = transform.determinant() < 0.0;
            placement.mirrored += usize::from(mirrored);
            for &[a, b, c] in triangles {
                cancellation.poll(Checkpoint::Deduplication, builder.triangle_count())?;
                let corner = |v: u32| {
                    let [x, y, z] = transform.apply(vertices[v as usize]);
                    Point3::new(x, y, z)
                };
                let corners = [corner(a), corner(b), corner(c)];
                if corners
                    .iter()
                    .any(|p| !(p.x.is_finite() && p.y.is_finite() && p.z.is_finite()))
                {
                    return Err(invalid(part, ThreeMfProblem::InvalidValue("transform")));
                }
                builder.triangle(if mirrored {
                    [corners[0], corners[2], corners[1]]
                } else {
                    corners
                })?;
            }
        }
        (Content::Components(components), _) => {
            for c in components {
                let target = c.path.as_deref().unwrap_or(part);
                place(
                    package,
                    target,
                    c.object,
                    &c.transform.then(transform),
                    builder,
                    placement,
                    cancellation,
                )?;
            }
        }
    }
    Ok(())
}

fn parse(
    bytes: &[u8],
    limits: &ModelLimits,
    cancellation: &mut Cancellation<'_>,
) -> Result<SourceModel, ModelImportError> {
    cancellation.check(Checkpoint::Detection)?;
    let archive = Archive::parse(
        bytes,
        ArchiveLimits {
            max_entries: limits.max_package_entries,
            max_entry_bytes: limits.max_part_bytes,
        },
    )
    .map_err(ThreeMfError::Archive)?;
    let root = start_part(&archive)?;
    let root_part = parse_part(&archive, &root, true, limits, cancellation)?;
    if root_part.build.is_empty() {
        return Err(invalid(&root, ThreeMfProblem::EmptyBuild));
    }
    // Parts referenced through p:path, each parsed once.
    let mut targets: Vec<String> = root_part
        .build
        .iter()
        .map(|r| r.path.clone())
        .chain(root_part.objects.values().flat_map(|o| match &o.content {
            Content::Components(c) => c.iter().map(|r| r.path.clone()).collect(),
            Content::Mesh { .. } => Vec::new(),
        }))
        .flatten()
        .collect();
    targets.sort_unstable();
    targets.dedup();
    let unit = root_part.unit;
    let mut properties = root_part.properties;
    let mut parts = BTreeMap::new();
    parts.insert(root.to_ascii_lowercase(), root_part);
    for target in &targets {
        let key = target.to_ascii_lowercase();
        if parts.contains_key(&key) {
            continue;
        }
        let part = parse_part(&archive, target, false, limits, cancellation)?;
        if part.unit != unit {
            return Err(invalid(target, ThreeMfProblem::UnitMismatch));
        }
        properties |= part.properties;
        parts.insert(key, part);
    }
    let external = parts.len() - 1;
    let package = Package {
        root: root.clone(),
        parts,
    };
    // Check every reference and the expanded size before building anything.
    let mut total = 0usize;
    for item in &package.part(&package.root).build {
        let target = item.path.as_deref().unwrap_or(&package.root);
        total = total.saturating_add(expanded(&package, target, item.object, 0, limits)?);
    }
    if total > limits.input.max_triangles {
        return Err(ModelImportError::LimitExceeded {
            resource: ImportResource::Triangles,
            limit: limits.input.max_triangles,
        });
    }
    let mut builder = WeldingBuilder::new(limits.input);
    let mut placement = Placement::default();
    for item in &package.part(&package.root).build {
        let target = item.path.as_deref().unwrap_or(&package.root);
        place(
            &package,
            target,
            item.object,
            &item.transform,
            &mut builder,
            &mut placement,
            cancellation,
        )?;
    }
    drop(package);
    let (mesh, diagnostics) = builder.finish(cancellation)?;
    let mut notes = Vec::new();
    for (kind, count) in placement.skipped {
        notes.push(ImportNote::ObjectsSkipped {
            kind: kind.to_owned(),
            count,
        });
    }
    if placement.mirrored > 0 {
        notes.push(ImportNote::MirroredInstances {
            count: placement.mirrored,
        });
    }
    if properties {
        notes.push(ImportNote::PropertiesIgnored);
    }
    if external > 0 {
        notes.push(ImportNote::ExternalParts { count: external });
    }
    Ok(SourceModel {
        mesh,
        declared_unit: Some(unit),
        objects: placement.instances,
        notes,
        diagnostics,
    })
}
