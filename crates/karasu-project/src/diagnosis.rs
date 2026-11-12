//! What is wrong with a model as imported, and whether a plate's settings deal with it (M6.3).
//!
//! An interface diagnoses a model when it is opened, so the user learns about holes or inside-out
//! bodies before slicing rather than from a refused job. The diagnosis only reads the mesh: it
//! reports what the core's defect analysis finds and what its repairs *would* change, and never
//! changes the model. Repairs happen only when the plate's settings ask for them (TD-054).
//!
//! Nested reversed bodies are not findings: a closed body inside another with reversed faces is
//! how a model describes an internal cavity.
//!
//! The printability state (M6.4d r1, TD-092) sums a diagnosis up for the mesh as it will be
//! sliced: diagnose the repaired mesh to judge the model after its repairs.
use karasu_core::{
    cancellation::Cancellation,
    mesh::{
        repair::{RepairPolicy, RepairReport},
        topology::{DefectReport, TopologyError, TopologyLimits},
        TriangleMesh,
    },
};

/// The defects of a model and what the repairs would change.
#[derive(Clone, Debug, PartialEq)]
pub struct Diagnosis {
    /// The defects found in the model as imported.
    pub defects: DefectReport,
    /// What duplicate collapse, consistent orientation and outward orientation would change.
    pub repairs: RepairReport,
    /// Unbalanced non-manifold edges left after those repairs: fins, which make layers fail
    /// until the non-manifold repair removes them (TD-092, R5).
    pub fins: usize,
    /// Facets the import left out for a non-finite coordinate (TD-092 r9). The mesh does not
    /// have them, so [`diagnose`] leaves this at zero; the caller that imported the model sets it.
    pub dropped_facets: usize,
}

/// One problem a diagnosis found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Finding {
    /// Closed bodies enclose negative volume: their faces point inward.
    InsideOut {
        /// Bodies turned outward by the outward repair.
        bodies: usize,
    },
    /// Some faces point the other way from their neighbours.
    InconsistentOrientation {
        /// Bodies whose orientation the repair makes consistent.
        bodies: usize,
    },
    /// Faces listed more than once.
    DuplicateFaces {
        /// Faces the repair removes.
        faces: usize,
    },
    /// The surface is open: boundary edges form holes or open chains.
    Holes {
        /// Holes whose rim closes into a loop.
        holes: usize,
        /// Boundary edges in all.
        edges: usize,
    },
    /// Holes of zero area whose rim lies on a line (cracks), as exporters leave at
    /// T-junctions; they are among the holes too.
    Cracks {
        /// Such holes.
        cracks: usize,
    },
    /// Edges shared by more than two faces, as where bodies touch.
    NonManifold {
        /// Such edges.
        edges: usize,
    },
    /// Edges with an extra open face, as a flap or a sheet hanging from the surface: layers
    /// crossing them do not close.
    Fins {
        /// Such edges, left after duplicate collapse.
        edges: usize,
    },
    /// Bodies whose orientation cannot be made consistent (for example a Möbius strip).
    NonOrientable {
        /// Such bodies.
        bodies: usize,
    },
    /// Bodies whose volume is too close to zero to tell inside from outside.
    AmbiguousVolume {
        /// Such bodies.
        bodies: usize,
    },
    /// No usable triangle remains after degenerate ones are excluded.
    Empty,
    /// Faces of zero area: needles (two corners at one position) and caps (three corners on a
    /// line). Slicing ignores them; hollowing refuses them.
    DegenerateFaces {
        /// Needles.
        needles: usize,
        /// Caps.
        caps: usize,
    },
    /// Vertices where bodies or parts of one body touch at a single point.
    PinchedVertices {
        /// Such vertices.
        vertices: usize,
    },
    /// Bodies with less surface than [`karasu_core::mesh::topology::SMALL_COMPONENT_MM2`]: specks that print as nothing
    /// useful.
    SmallBodies {
        /// Such bodies.
        bodies: usize,
    },
    /// Facets with a NaN or infinite coordinate, left out at import: they have no position, so
    /// what they belonged to is lost; jobs refuse the model unless the repairs accept it.
    DroppedFacets {
        /// Such facets.
        facets: usize,
    },
}

/// Whether a model can be sliced and prepared as it is, ordered from best to worst.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Printability {
    /// No defect, or only a cavity's reversed shell.
    Ready,
    /// Defects that slicing handles, such as touching bodies or duplicate faces: named in the
    /// findings, nothing to do.
    ReadyWithNotes,
    /// Slicing works (holes closed in each layer up to the plate's bound), but hollowing refuses
    /// the model until it is repaired: holes other than zero-area cracks.
    NeedsRepairForHollowing,
    /// Slicing would fail or could be wrong where the findings say.
    NeedsRepair,
}

/// Whether a plate's settings deal with a finding when it is sliced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handling {
    /// A repair or layer closure enabled on the plate deals with it. Layer closure admits holes
    /// and closes each layer's gaps up to its bound; a layer with a wider gap still fails.
    Handled,
    /// Slicing fills layers correctly anyway (for example touching bodies under union filling).
    Harmless,
    /// Slicing will be refused, or the result may be wrong, unless a setting changes.
    Unhandled,
}

/// The plate settings a diagnosis is judged against.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Treatment {
    /// The source repairs enabled.
    pub repair: karasu_core::preparation::repair::RepairSpec,
    /// Whether holes are closed in each layer.
    pub close_holes: bool,
}

/// Diagnose `mesh` without changing it.
///
/// # Errors
/// Cancellation, or a mesh beyond the topology limits.
pub fn diagnose(
    mesh: &TriangleMesh,
    cancel: &mut Cancellation<'_>,
) -> Result<Diagnosis, TopologyError> {
    let limits = TopologyLimits::default();
    let defects = mesh.defect_report_with_cancellation(limits, cancel)?;
    let policy = RepairPolicy {
        duplicates: true,
        orientation: true,
        outward: true,
    };
    let (repaired, repairs) = mesh.repair_with_cancellation(policy, limits, cancel)?;
    let fins = match repaired {
        Some(repaired) if defects.unbalanced_non_manifold_edges > 0 => {
            repaired
                .defect_report_with_cancellation(limits, cancel)?
                .unbalanced_non_manifold_edges
        }
        _ => defects.unbalanced_non_manifold_edges,
    };
    Ok(Diagnosis {
        defects,
        repairs,
        fins,
        dropped_facets: 0,
    })
}

impl Diagnosis {
    /// The problems found, most serious first.
    #[must_use]
    pub fn findings(&self) -> Vec<Finding> {
        let (d, r) = (&self.defects, &self.repairs);
        let mut findings = Vec::new();
        if d.usable_triangles == 0 {
            findings.push(Finding::Empty);
        }
        if self.dropped_facets > 0 {
            findings.push(Finding::DroppedFacets {
                facets: self.dropped_facets,
            });
        }
        if r.outward_flipped_components > 0 {
            findings.push(Finding::InsideOut {
                bodies: r.outward_flipped_components,
            });
        }
        if d.boundary_edges > 0 {
            findings.push(Finding::Holes {
                holes: d.rim_loops,
                edges: d.boundary_edges,
            });
        }
        if r.non_orientable_components > 0 {
            findings.push(Finding::NonOrientable {
                bodies: r.non_orientable_components,
            });
        }
        if r.ambiguous_volume_components > 0 {
            findings.push(Finding::AmbiguousVolume {
                bodies: r.ambiguous_volume_components,
            });
        }
        if r.reoriented_components > 0 {
            findings.push(Finding::InconsistentOrientation {
                bodies: r.reoriented_components,
            });
        }
        if r.dropped_duplicate_faces > 0 {
            findings.push(Finding::DuplicateFaces {
                faces: r.dropped_duplicate_faces,
            });
        }
        if d.cracks > 0 {
            findings.push(Finding::Cracks { cracks: d.cracks });
        }
        if self.fins > 0 {
            findings.push(Finding::Fins { edges: self.fins });
        }
        if d.non_manifold_edges > 0 {
            findings.push(Finding::NonManifold {
                edges: d.non_manifold_edges,
            });
        }
        if d.needles + d.caps > 0 {
            findings.push(Finding::DegenerateFaces {
                needles: d.needles,
                caps: d.caps,
            });
        }
        if d.non_manifold_vertices > 0 {
            findings.push(Finding::PinchedVertices {
                vertices: d.non_manifold_vertices,
            });
        }
        if d.small_components > 0 {
            findings.push(Finding::SmallBodies {
                bodies: d.small_components,
            });
        }
        findings
    }

    /// The state of the mesh as diagnosed, sliced by union without further repair, with holes
    /// closed in each layer when `close_holes` is set. It follows the checks slicing and
    /// hollowing apply: union slicing refuses winding conflicts, open edges without layer
    /// closure, and models with no usable face; a layer crossing an unbalanced non-manifold edge
    /// (a fin) does not close; rasterizing refuses inside-out bodies; hollowing
    /// refuses holes other than zero-area cracks and unbalanced edges (it leaves zero-area faces
    /// and cracks out itself, TD-092 H1, and hollows overlapping bodies as their union, H2).
    ///
    /// Self-intersections are not part of it: a fold that crosses itself can leave reversed
    /// slivers that rasterizing refuses in a layer, as `Base_01.stl` has (0.0017 mm² in five
    /// layers), which only slicing finds; a reversed tolerance leaves small ones empty.
    ///
    /// Facets dropped at import make it at best [`Printability::ReadyWithNotes`]: the mesh is
    /// whole without them, but part of the file is lost.
    #[must_use]
    pub fn printability(&self, close_holes: bool) -> Printability {
        let (d, r) = (&self.defects, &self.repairs);
        if d.usable_triangles == 0
            || d.winding_conflicts > 0
            || d.unbalanced_non_manifold_edges > 0
            || (d.boundary_edges > 0 && !close_holes)
            || r.outward_flipped_components > 0
            || r.ambiguous_volume_components > 0
            || d.non_orientable_components > 0
        {
            Printability::NeedsRepair
        } else if d.rim_chains > 0 || d.rim_loops > d.cracks {
            Printability::NeedsRepairForHollowing
        } else if d.boundary_edges > 0
            || self.dropped_facets > 0
            || d.excluded_triangles > 0
            || d.non_manifold_edges > 0
            || d.non_manifold_vertices > 0
            || d.same_orientation_duplicates > 0
            || d.opposite_duplicate_pairs > 0
            || d.small_components > 0
        {
            Printability::ReadyWithNotes
        } else {
            Printability::Ready
        }
    }
}

impl Finding {
    /// Whether `treatment` deals with this finding under union filling, the interface's fill.
    #[must_use]
    pub fn handling(self, treatment: Treatment) -> Handling {
        let handled = |yes: bool| {
            if yes {
                Handling::Handled
            } else {
                Handling::Unhandled
            }
        };
        match self {
            Self::InsideOut { .. } => handled(treatment.repair.outward),
            Self::InconsistentOrientation { .. } => handled(treatment.repair.orientation),
            Self::DuplicateFaces { .. } => handled(treatment.repair.duplicates),
            // Hole filling closes cracks by splitting the faces along them; layer closure closes
            // their zero-width gaps.
            Self::Holes { .. } | Self::Cracks { .. } => {
                handled(treatment.close_holes || treatment.repair.fill_holes.is_some())
            }
            Self::Fins { .. } => handled(treatment.repair.non_manifold.is_some()),
            Self::DroppedFacets { .. } => handled(treatment.repair.non_finite),
            Self::NonManifold { .. }
            | Self::DegenerateFaces { .. }
            | Self::PinchedVertices { .. }
            | Self::SmallBodies { .. } => Handling::Harmless,
            Self::NonOrientable { .. } | Self::AmbiguousVolume { .. } | Self::Empty => {
                Handling::Unhandled
            }
        }
    }
}
