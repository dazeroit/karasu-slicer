//! Drain holes cut into a model's mesh (TD-139, TD-140 phase 4): the solid the printer makes,
//! as one closed mesh, for export.
//!
//! Slicing never needs this: it removes the holes' exact sections from every layer
//! ([`HoleCuts`] as a [`crate::raster::LayerCut`]). A model written for another program has no
//! layers, so the holes must be in its mesh: the model's solid minus the union of the holes'
//! polygonal cylinders, made by the exact Boolean kernel ([`crate::boolean`]).
//!
//! **What the result is:** the points where the model's winding number is positive and no
//! cylinder's is, as a closed mesh facing out. Overlapping bodies of the model come out as their
//! union (wherever they are, not only near the holes), overlapping holes remove their union, and
//! a hole leaves the model's surface where it does not reach it.
//!
//! **On the grid** (TD-140 phase 5): the model's and the cylinders' vertices are snapped to the
//! kernel's grid of 2⁻¹⁵ mm (at most 2⁻¹⁶ mm on each axis), and every vertex of the result is a
//! grid point, exact in `f64` and, within ±512 mm, in `f32`: a model file of any format stores
//! it exactly. Rounding the points the cut makes to the grid can fold slivers; the kernel
//! resolves them in rounds, moving the vertices near them to the centres of 15.6 µm cells (at
//! most 7.8 µm on each axis); a separate speck it leaves, no larger than such a cell, is left
//! out. [`DrilledModel`] counts all of it, and what crosses or is flat
//! when the rounds stop; an export writes the mesh and reports those (TD-139). Nothing here
//! hides them.
//!
//! **Refusals, never repairs:** a model that is not closed has no inside, and is refused with
//! the number of its unmatched edges; so is a vertex beyond ±2,000 mm.
use super::HoleCuts;
use crate::{
    boolean::{self, BooleanError},
    cancellation::{Cancellation, Cancelled},
    mesh::{MeshError, TriangleMesh},
};
use std::fmt;

/// A model with its drain holes cut ([`HoleCuts::drill`]), and what the grid did to it.
#[derive(Debug)]
pub struct DrilledModel {
    /// The model's solid without the holes: closed and facing out, every vertex a grid point.
    pub mesh: TriangleMesh,
    /// Model and cylinder vertices moved by snapping to the grid (at most 2⁻¹⁶ mm on each axis).
    pub snapped: usize,
    /// Points made by the cut rounded to the grid (at most 2⁻¹⁶ mm on each axis).
    pub rounded: usize,
    /// Rounds of resolving what rounding made cross or flat (0: nothing).
    pub rounds: u32,
    /// Vertices moved to the centre of their 15.6 µm cell by those rounds.
    pub moved: usize,
    /// The farthest such a move on one axis, in millimetres (at most 7.8 µm).
    pub farthest_mm: f64,
    /// Separate bodies no larger than 15.6 µm on any axis that rounding left, left out.
    pub specks: usize,
    /// Vertex copies moved about 0.06 µm into their own bodies where bodies of the model touched
    /// along an edge or at a point, so no reader welds them into an edge of four faces.
    pub separated: usize,
    /// Triangles without area left when the rounds stopped.
    pub flat: usize,
    /// Pairs of triangles that meet other than along shared vertices and edges, left when the
    /// rounds stopped.
    pub crossings: usize,
}

/// Which mesh of a cut a refusal concerns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrillInput {
    /// The model being drilled.
    Model,
    /// The holes' cylinders.
    Holes,
}

impl fmt::Display for DrillInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Model => "the model",
            Self::Holes => "the drain holes",
        })
    }
}

/// Why drain holes could not be cut into a model's mesh.
#[derive(Debug)]
pub enum DrillError {
    /// A vertex is not finite, or lies beyond the ±2,000 mm the cut admits.
    Coordinate {
        /// The mesh it belongs to.
        input: DrillInput,
        /// Its index in that mesh.
        vertex: usize,
        /// The coordinate and why it is refused.
        detail: String,
    },
    /// A mesh is not closed, so it has no inside: this many edges are not used as often in
    /// each direction (on its vertices snapped to the grid).
    Open {
        /// The mesh concerned.
        input: DrillInput,
        /// Unmatched edges.
        edges: usize,
    },
    /// The caller cancelled.
    Cancelled(Cancelled),
    /// Memory for the cut could not be reserved.
    Resource,
    /// The result could not be made a mesh.
    Mesh(MeshError),
    /// The result came out not closed: a defect of the cut, never written.
    NotClosed {
        /// Edges, by position, not used as often in each direction.
        edges: usize,
    },
}

impl fmt::Display for DrillError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Coordinate {
                input,
                vertex,
                detail,
            } => write!(f, "{input}, vertex {vertex}: {detail}"),
            Self::Open { input, edges } => write!(
                f,
                "{input} is not closed ({edges} edges are not used as often in each direction), \
                 so the holes cannot be cut into it"
            ),
            Self::Cancelled(c) => c.fmt(f),
            Self::Resource => f.write_str("memory for cutting the drain holes ran out"),
            Self::Mesh(e) => write!(f, "the drilled model is not a valid mesh: {e}"),
            Self::NotClosed { edges } => write!(
                f,
                "the drilled model came out not closed ({edges} edges); this is a defect of \
                 karasu's Boolean kernel, please report it"
            ),
        }
    }
}

impl std::error::Error for DrillError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Cancelled(c) => Some(c),
            Self::Mesh(e) => Some(e),
            _ => None,
        }
    }
}

impl From<BooleanError> for DrillError {
    fn from(error: BooleanError) -> Self {
        let input = |index: usize| {
            if index == 0 {
                DrillInput::Model
            } else {
                DrillInput::Holes
            }
        };
        match error {
            BooleanError::Coordinate {
                input: i,
                vertex,
                error,
            } => Self::Coordinate {
                input: input(i),
                vertex,
                detail: error.to_string(),
            },
            BooleanError::Open { input: i, edges } => Self::Open {
                input: input(i),
                edges,
            },
            BooleanError::Arrange(boolean::arrange::ArrangeError::Cancelled(c)) => {
                Self::Cancelled(c)
            }
            BooleanError::Arrange(boolean::arrange::ArrangeError::Resource) => Self::Resource,
            BooleanError::Mesh(e) => Self::Mesh(e),
            BooleanError::NotClosed { edges } => Self::NotClosed { edges },
        }
    }
}

/// In the model (its first membership) and in no hole.
fn drilled(inside: &[bool]) -> bool {
    inside[0] && !inside[1]
}

impl HoleCuts {
    /// `model` (a closed mesh in world millimetres, where its winding number is positive) with
    /// these holes cut: the solid the layers print, as one closed mesh (see the module
    /// documentation). Deterministic: the same mesh for any number of threads.
    ///
    /// # Errors
    /// [`DrillError`]: a vertex beyond the admitted range, a mesh that is not closed,
    /// cancellation, exhausted memory, a result that cannot be a mesh, or one that comes out not
    /// closed.
    pub fn drill(
        &self,
        model: &TriangleMesh,
        cancel: &mut Cancellation<'_>,
    ) -> Result<DrilledModel, DrillError> {
        let solved = boolean::solve(&[model, self.cutters()], &drilled, cancel)?;
        Ok(DrilledModel {
            mesh: solved.mesh,
            snapped: solved.snapped,
            rounded: solved.rounded,
            rounds: solved.rounds,
            moved: solved.moved,
            farthest_mm: solved.farthest_mm,
            specks: solved.specks,
            separated: solved.separated,
            flat: solved.flat,
            crossings: solved.crossings,
        })
    }
}
