//! Live preparation previews (M6.4a, TD-082).
//!
//! When the plate has preparation (hollowing, drain holes or manual supports) and a printer and
//! resin, the plate is prepared in the background with [`karasu_project::pipeline::prepare`]
//! [`DEBOUNCE`] after its last edit, so a drag starts one preparation when it pauses, not one per
//! frame. An edit cancels a preparation in progress. Nothing is sliced or written.
//!
//! **Freshness:** a preparation is shown only while the plate still has the revision it was
//! prepared from; an older one is hidden rather than drawn where the objects no longer are.
//!
//! **Display:** per object, in world coordinates:
//! - supports, in the support colour, or the error colour when any finding is an error;
//! - the cavity of a hollowed object, seen when the section view cuts the model (M6.4b).
//!
//! Drain holes are drawn by the preparation panel, from each hole's verdict (TD-084).
//!
//! Per-support highlighting comes with the supports panel (M6.4c).
use crate::{
    appearance::{Appearance, Rgba},
    camera::{Mat4, Ray},
    gizmo::{ray_triangle, Affine},
    i18n::tr,
    messages::Status,
    theme,
    viewport::{Draw, Section, Viewport},
    workspace::{affine, map_hole, SurfaceHit, Workspace},
};
use eframe::egui_wgpu;
use karasu_core::mesh::TriangleMesh;
use karasu_core::{
    cancellation::Cancellation,
    preparation::{
        sampled::{HoleCuts, HoleFindings, HoleFrame, HoleProbe, NOMINAL_REFUSED_DEPTH_MM},
        HoleSpec,
    },
};
use karasu_project::{
    jobs::{JobError, PreparationEvent, PreparationJob},
    pipeline::{ObjectPreparation, Preparation},
    project::{Hollowing, PlateContent, PlateObject},
    request::{DrainHole, SliceRequest, Subject},
};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

/// What the probe says of a drain hole (TD-089), with its shape: even a hole that cannot be
/// cut is drawn where it would be.
#[derive(Debug)]
pub struct Verdict {
    /// Why the hole cannot be cut, in the core's words; `None` when it can.
    pub refusal: Option<String>,
    /// The cutting cylinder in world millimetres, when it could be built.
    pub cutter: Option<TriangleMesh>,
    /// The cut as resolved: entry, direction, length and diameter.
    pub spec: Option<HoleSpec>,
    /// Where a surface hole opens: its foot and the normal towards the free side.
    pub frame: Option<HoleFrame>,
    /// What the hole does, when it can be cut.
    pub findings: Option<HoleFindings>,
}

impl Verdict {
    /// Whether the hole can be cut.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.refusal.is_none()
    }

    /// A copy, its cylinder rebuilt from the same vertices and faces.
    fn copy(&self) -> Self {
        Self {
            refusal: self.refusal.clone(),
            cutter: self.cutter.as_ref().and_then(|mesh| {
                TriangleMesh::try_new(mesh.vertices().to_vec(), mesh.triangles().to_vec()).ok()
            }),
            spec: self.spec,
            frame: self.frame,
            findings: self.findings.clone(),
        }
    }
}

/// The cylinder of `spec` for display, within 1 µm.
fn cylinder(spec: HoleSpec) -> Option<TriangleMesh> {
    let cuts = HoleCuts::build(&[spec], 1e-3, 1, 4096, &mut Cancellation::never()).ok()?;
    let mesh = cuts.cutters();
    TriangleMesh::try_new(mesh.vertices().to_vec(), mesh.triangles().to_vec()).ok()
}

/// `hole`, given in its object's model frame, in world coordinates where `map` places the
/// object, its tilt carried by `probe`'s normal (TD-089).
fn world(probe: &HoleProbe, map: &Affine, hole: DrainHole) -> Result<DrainHole, String> {
    map_hole(hole, map)
        .with_world_tilt(map.linear, probe, &mut Cancellation::never())
        .map_err(|e| e.to_string())
}

/// Resolve `hole`, given in its object's model frame placed by `map`, with `probe` and report
/// what it does (TD-089). Nothing about a hole's path is refused; a hole whose automatic depth
/// finds no cavity is drawn at a nominal depth.
fn check(probe: &HoleProbe, map: &Affine, hole: &DrainHole) -> Verdict {
    let mut never = Cancellation::never();
    let hole = match world(probe, map, *hole) {
        Ok(hole) => hole,
        Err(refusal) => {
            return Verdict {
                refusal: Some(refusal),
                cutter: None,
                spec: None,
                frame: None,
                findings: None,
            }
        }
    };
    match hole.resolve(probe, &mut never) {
        Ok((spec, frame)) => Verdict {
            refusal: None,
            cutter: cylinder(spec),
            spec: Some(spec),
            frame,
            findings: probe.findings(&spec, &mut never).ok(),
        },
        Err(error) => {
            let nominal = match hole {
                DrainHole::Surface {
                    depth_mm: None,
                    surface_mm,
                    diameter_mm,
                    outside_mm,
                    inside_mm,
                    tilt,
                    surface,
                } => DrainHole::Surface {
                    surface_mm,
                    diameter_mm,
                    outside_mm,
                    inside_mm,
                    tilt,
                    depth_mm: Some(NOMINAL_REFUSED_DEPTH_MM),
                    surface,
                }
                .resolve(probe, &mut never)
                .ok(),
                _ => None,
            };
            Verdict {
                refusal: Some(error.to_string()),
                cutter: nominal.and_then(|(spec, _)| cylinder(spec)),
                spec: nominal.map(|(spec, _)| spec),
                frame: nominal.and_then(|(_, frame)| frame),
                findings: None,
            }
        }
    }
}

/// The unit normal of a triangle by its winding, `None` when it is degenerate.
fn triangle_normal([first, second, third]: [[f64; 3]; 3]) -> Option<[f64; 3]> {
    let (along, across): ([f64; 3], [f64; 3]) = (
        std::array::from_fn(|k| second[k] - first[k]),
        std::array::from_fn(|k| third[k] - first[k]),
    );
    let normal = [
        along[1] * across[2] - along[2] * across[1],
        along[2] * across[0] - along[0] * across[2],
        along[0] * across[1] - along[1] * across[0],
    ];
    let length = normal.iter().map(|c| c * c).sum::<f64>().sqrt();
    (length > 0.0 && length.is_finite()).then(|| normal.map(|c| c / length))
}

/// Quiet time after the last edit before the plate is prepared again.
pub const DEBOUNCE: Duration = Duration::from_millis(300);
/// Viewport mesh keys of previews, apart from the model keys (hashes of model identities).
const PREVIEW_KEYS: u64 = 0xF00D_0000_0000_0000;

/// What a preview mesh shows. Supports are not prepared here: the Supports tool builds them
/// against this preparation (TD-108).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Cavity,
}

impl Kind {
    /// The preview's colour; a cavity has its own when its object is `selected`.
    fn color(self, selected: bool, appearance: &Appearance) -> Rgba {
        match self {
            Self::Cavity if selected => appearance.cavity_selected,
            Self::Cavity => appearance.cavity,
        }
    }
}

/// A preview mesh in the viewport.
struct Uploaded {
    key: u64,
    origin: [f64; 3],
    kind: Kind,
    /// The plate object it belongs to.
    object: usize,
}

/// The plate's preparation: the job, the latest result and its viewport meshes.
#[derive(Default)]
pub struct PreparationUi {
    job: Option<PreparationJob>,
    phase: String,
    /// The plate revision last seen, and when it was first seen.
    seen: Option<(u64, Instant)>,
    /// The latest preparation, the revision it describes and the plate as it was then. Shared:
    /// the Supports tool builds supports against it off the interface thread (TD-108).
    result: Option<(u64, Arc<Preparation>, PlateContent)>,
    /// The plate the running job prepares.
    preparing: Option<PlateContent>,
    /// Objects drain holes are being placed on, which need a hole probe even when solid.
    probes: Vec<usize>,
    /// The objects the running job, then the latest result, keep a probe for.
    probed: (Vec<usize>, Vec<usize>),
    /// Each hole's verdict, per object, for the plate revision they were probed at.
    verdicts: (u64, HashMap<usize, Vec<Verdict>>),
    /// The latest verdicts probed per object, kept with the revision they were probed at, so
    /// a moved object's holes are still shown while it is prepared again.
    held: HashMap<usize, (u64, Vec<Verdict>)>,
    /// The inverse of each object's placement when the result was prepared.
    prepared_at: Vec<Option<Affine>>,
    /// Why the plate could not be prepared at all, and for which revision.
    refused: Option<(u64, String)>,
    /// Bumped with every result, so its meshes get new keys.
    generation: u64,
    /// Meshes uploaded for the result, and the generation they belong to.
    uploaded: (u64, Vec<Uploaded>),
}

/// Whether the plate has anything to prepare: any object, whose repairs the Repair section
/// reports (TD-092 r8), with its operations and hole `probe`s. A model already imported and
/// repaired is kept by the pipeline cache, so a solid object costs little after the first time.
fn wanted(workspace: &Workspace, probes: bool) -> bool {
    let plate = workspace.plate();
    plate.setting().is_some() && (probes || !plate.objects.is_empty())
}

/// The request without supports: the Supports tool builds them against the preparation
/// (TD-108), so changing them never prepares a model again.
fn without_supports(request: &mut SliceRequest) {
    if let Subject::Plate(plate) = &mut request.subject {
        for object in &mut plate.objects {
            object.supports = None;
            object.accept_support_collisions = false;
        }
    }
    request.supports = None;
    request.accept_support_collisions = false;
}

/// Whether the plates `a` and `b` differ at most in their objects' supports. Every field is
/// named, so a new one is compared or left out on purpose.
#[allow(clippy::float_cmp)] // The same edit compares exactly; any change prepares again.
fn same_but_supports(a: &PlateContent, b: &PlateContent) -> bool {
    let PlateContent {
        name,
        printer,
        resin,
        print_setting,
        settings,
        objects,
    } = a;
    let objects_alike = objects.len() == b.objects.len()
        && objects.iter().zip(&b.objects).all(|(x, y)| {
            let PlateObject {
                name,
                model,
                units,
                translate_mm,
                pose,
                operations,
                supports: _,
                accept_support_collisions: _,
                copies,
                arrange,
                // The Orient tool's points do not change what is prepared.
                protected: _,
            } = x;
            *name == y.name
                && *model == y.model
                && *units == y.units
                && *translate_mm == y.translate_mm
                && *pose == y.pose
                && *operations == y.operations
                && *copies == y.copies
                && *arrange == y.arrange
        });
    objects_alike
        && *name == b.name
        && *printer == b.printer
        && *resin == b.resin
        && *print_setting == b.print_setting
        && *settings == b.settings
}

/// The centre of a mesh's bounding box: preview vertices are uploaded relative to it.
pub(crate) fn centre(mesh: &TriangleMesh) -> [f64; 3] {
    let (mut min, mut max) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for p in mesh.vertices() {
        for (k, v) in [p.x, p.y, p.z].into_iter().enumerate() {
            min[k] = min[k].min(v);
            max[k] = max[k].max(v);
        }
    }
    std::array::from_fn(|k| {
        if min[k] <= max[k] {
            f64::midpoint(min[k], max[k])
        } else {
            0.0
        }
    })
}

/// The matrix of `forward` after `back`: what was placed by the inverse of `back` (an object's
/// old placement, inverted) carried to where `forward` places it now.
pub(crate) fn carried(back: &Affine, forward: &Affine) -> Mat4 {
    // forward(back(p)): the linear parts compose, and back's translation is carried over.
    let linear: [[f64; 3]; 3] = std::array::from_fn(|r| {
        std::array::from_fn(|c| {
            (0..3)
                .map(|k| forward.linear[r][k] * back.linear[k][c])
                .sum()
        })
    });
    let translation = forward.point(back.translation);
    Mat4(std::array::from_fn(|row| {
        std::array::from_fn(|column| match (row, column) {
            (3, 3) => 1.0,
            (3, _) => 0.0,
            (_, 3) => translation[row],
            _ => linear[row][column],
        })
    }))
}

/// The translation that places vertices stored relative to `origin`.
pub(crate) fn translation(origin: [f64; 3]) -> Mat4 {
    Mat4(std::array::from_fn(|row| {
        let mut r = [0.0; 4];
        r[row] = 1.0;
        if row < 3 {
            r[3] = origin[row];
        }
        r
    }))
}

impl PreparationUi {
    /// Follow the plate: take the job's events, cancel an outdated job, and start a new one once
    /// the plate has been quiet for [`DEBOUNCE`]. Returns whether a repaint is needed soon.
    pub fn poll(&mut self, workspace: &Workspace) -> bool {
        let revision = workspace.revision();
        if self.seen.is_none_or(|(seen, _)| seen != revision) {
            self.seen = Some((revision, Instant::now()));
        }
        let mut done = false;
        if let Some(job) = &mut self.job {
            for event in job.poll() {
                match event {
                    PreparationEvent::Phase(name) => self.phase = name,
                    PreparationEvent::Finished(prepared) => {
                        done = true;
                        match prepared.result {
                            Ok(preparation) => {
                                self.generation += 1;
                                let content = self
                                    .preparing
                                    .take()
                                    .unwrap_or_else(|| workspace.plate().clone());
                                self.prepared_at = (0..content.objects.len())
                                    .map(|i| {
                                        let placement = workspace.placement_in(&content, i)?;
                                        affine(&placement)?.inverse()
                                    })
                                    .collect();
                                // The new result's probe decides the verdicts from now on:
                                // taken again, they are also held again for the next move.
                                self.held.clear();
                                self.verdicts = (0, HashMap::new());
                                self.probed.1 = std::mem::take(&mut self.probed.0);
                                self.result =
                                    Some((prepared.revision, Arc::new(preparation), content));
                            }
                            Err(JobError::Failed(error)) => {
                                // A refusal answers the probes asked for too.
                                self.probed.1 = std::mem::take(&mut self.probed.0);
                                self.refused = Some((prepared.revision, error.reason().to_owned()));
                            }
                            Err(JobError::Cancelled | JobError::Lost) => {}
                        }
                    }
                }
            }
        }
        // A finished job is dropped, and so is an outdated one (dropping cancels it).
        if done
            || self
                .job
                .as_ref()
                .is_some_and(|job| job.revision() != revision)
        {
            self.job = None;
        }
        if self.job.is_some() {
            return true;
        }
        let probed = self.probes.iter().all(|i| self.probed.1.contains(i));
        if !wanted(workspace, !self.probes.is_empty()) || (self.has_result_for(workspace) && probed)
        {
            return false;
        }
        let quiet = self
            .seen
            .is_some_and(|(_, since)| since.elapsed() >= DEBOUNCE);
        if quiet {
            // The preparation writes nothing; the request still names an output.
            match workspace.request(std::env::temp_dir().join("karasu-preparation.unused")) {
                Ok((mut request, revision)) => {
                    without_supports(&mut request);
                    self.phase = tr!("preparation-starting");
                    self.preparing = Some(workspace.plate().clone());
                    self.probed.0.clone_from(&self.probes);
                    self.job = Some(PreparationJob::start_with(
                        workspace.plate_id(),
                        revision,
                        request,
                        Some(workspace.pipeline_cache()),
                        self.probes.clone(),
                    ));
                }
                Err(error) => {
                    self.probed.1.clone_from(&self.probes);
                    self.refused = Some((revision, error.to_string()));
                }
            }
        }
        true
    }

    /// Whether nothing is left to prepare for the plate as it is: no job runs, and a result or
    /// a refusal describes it, or it needs none. A project opened waits for it (TD-130).
    #[must_use]
    pub fn settled(&self, workspace: &Workspace) -> bool {
        self.job.is_none()
            && (!wanted(workspace, !self.probes.is_empty()) || self.has_result_for(workspace))
    }

    /// The phase of the preparation running, if one is.
    #[must_use]
    pub fn running_phase(&self) -> Option<&str> {
        self.job.as_ref().map(|_| self.phase.as_str())
    }

    /// Whether the latest result, or refusal, describes the plate as it is: the same
    /// revision, or a later one whose only changes are supports, which are not prepared here.
    fn has_result_for(&self, workspace: &Workspace) -> bool {
        let revision = workspace.revision();
        self.current(workspace).is_some()
            || self.refused.as_ref().is_some_and(|(r, _)| *r == revision)
    }

    /// The preparation of the plate as it is now, if there is one: changing only supports
    /// keeps it (TD-108).
    #[must_use]
    pub fn current(&self, workspace: &Workspace) -> Option<&Preparation> {
        let revision = workspace.revision();
        self.result
            .as_ref()
            .filter(|(r, _, then)| *r == revision || same_but_supports(then, workspace.plate()))
            .map(|(_, preparation, _)| &**preparation)
    }

    /// The hole probe of object `index`, from the latest preparation, while the object's
    /// hollow is unchanged since (see [`Self::latest`]). Editing only holes keeps the probe, so
    /// holes are checked at once.
    fn probe(&self, workspace: &Workspace, index: usize) -> Option<&HoleProbe> {
        self.latest(workspace, index)?.probe()
    }

    /// Why no drain hole can be cut into object `index` at all, from the latest preparation
    /// while the object's hollow is unchanged: known before any hole is placed.
    pub fn drainage_unavailable(&self, workspace: &Workspace, index: usize) -> Option<String> {
        self.latest(workspace, index)?
            .drainage_unavailable()
            .map(|e| e.reason().to_owned())
    }

    /// Object `index` in the latest preparation, while its hollow is unchanged since: its
    /// model, placement, hollowing settings and the plate's repairs.
    #[allow(clippy::float_cmp)] // Exactly the same placement: any change re-hollows.
    fn latest(&self, workspace: &Workspace, index: usize) -> Option<&ObjectPreparation> {
        let now = workspace.plate();
        let (_, preparation, then) = self.result.as_ref()?;
        let (before, after) = (then.objects.get(index)?, now.objects.get(index)?);
        let same = |a: Option<&Hollowing>, b: Option<&Hollowing>| match (a, b) {
            (Some(a), Some(b)) => {
                a.wall_mm.to_bits() == b.wall_mm.to_bits()
                    && a.cell_mm == b.cell_mm
                    && a.simplify == b.simplify
            }
            (None, None) => true,
            _ => false,
        };
        let unchanged = before.model == after.model
            && before.units == after.units
            && before.translate_mm == after.translate_mm
            && before.pose == after.pose
            && same(before.hollowing(), after.hollowing())
            && then.settings.repair == now.settings.repair
            && then.printer == now.printer;
        if !unchanged {
            return None;
        }
        preparation.objects.get(index)?.as_ref().ok()
    }

    /// Object `index`'s latest preparation, shared, while its surface is unchanged since (see
    /// [`Self::latest`]), with the generation of the result: the Supports tool builds the
    /// object's supports against it off the interface thread, and again for a new result.
    #[must_use]
    pub fn shared(&self, workspace: &Workspace, index: usize) -> Option<(Arc<Preparation>, u64)> {
        self.latest(workspace, index)?;
        let (_, preparation, _) = self.result.as_ref()?;
        Some((Arc::clone(preparation), self.generation))
    }

    /// Ask for a hole probe of the `objects` drain holes are being placed on, which have none
    /// without holes; the plate is prepared again when one is missing.
    pub fn set_probes(&mut self, objects: Vec<usize>) {
        self.probes = objects;
    }

    /// Each drain hole's verdict for object `index` as the plate is now. `None` until the
    /// object has a probe.
    pub fn verdicts(&mut self, workspace: &Workspace, index: usize) -> Option<&[Verdict]> {
        let revision = workspace.revision();
        if self.verdicts.0 != revision {
            self.verdicts = (revision, HashMap::new());
        }
        if !self.verdicts.1.contains_key(&index) {
            let holes = workspace.holes(index);
            let map = workspace.model_to_world(index)?;
            let probe = self.probe(workspace, index)?;
            let verdicts: Vec<Verdict> =
                holes.iter().map(|hole| check(probe, &map, hole)).collect();
            let kept = verdicts.iter().map(Verdict::copy).collect();
            self.held.insert(index, (revision, kept));
            self.verdicts.1.insert(index, verdicts);
        }
        self.verdicts.1.get(&index).map(Vec::as_slice)
    }

    /// The verdict on `hole`, in object `index`'s model frame; holes may overlap, so the
    /// others do not matter. `None` until the object has a probe.
    pub fn verdict(
        &self,
        workspace: &Workspace,
        index: usize,
        hole: &DrainHole,
    ) -> Option<Verdict> {
        let map = workspace.model_to_world(index)?;
        let probe = self.probe(workspace, index)?;
        Some(check(probe, &map, hole))
    }

    /// The world normal surface hole `hole` (in object `index`'s model frame) leans from, the
    /// mean over its radius at its foot; `None` until the object has a probe, or where the
    /// normal is undefined.
    #[must_use]
    pub fn hole_outward(
        &self,
        workspace: &Workspace,
        index: usize,
        hole: &DrainHole,
    ) -> Option<[f64; 3]> {
        let map = workspace.model_to_world(index)?;
        let probe = self.probe(workspace, index)?;
        map_hole(*hole, &map)
            .outward(probe, &mut Cancellation::never())
            .ok()?
    }

    /// Where `ray` first meets the visible cavity of object `index`, within the `section`'s
    /// heights: the cavity is seen from inside the void, so only faces turned away from the
    /// viewer count (its faces point towards the material). `None` until the object's cavity
    /// matches the plate (drain holes and supports are placed on it, TD-089 and TD-108).
    #[must_use]
    pub fn cavity_hit(
        &self,
        workspace: &Workspace,
        index: usize,
        ray: &Ray,
        section: Section,
    ) -> Option<SurfaceHit> {
        self.latest(workspace, index)?;
        let (_, preparation, _) = self.result.as_ref()?;
        let cavity = preparation
            .objects
            .get(index)?
            .as_ref()
            .ok()?
            .hollowed()?
            .cavity();
        let vertices = cavity.vertices();
        let corner = |i: u32| vertices.get(i as usize).map(|p| [p.x, p.y, p.z]);
        let kept =
            |z: f64| section.below.is_none_or(|b| z >= b) && section.above.is_none_or(|a| z <= a);
        cavity
            .triangles()
            .iter()
            .filter_map(|t| {
                let corners = [corner(t[0])?, corner(t[1])?, corner(t[2])?];
                let normal = triangle_normal(corners)?;
                if (0..3).map(|k| normal[k] * ray.direction[k]).sum::<f64>() <= 0.0 {
                    return None;
                }
                let t = ray_triangle(ray.origin, ray.direction, corners)?;
                let point: [f64; 3] = std::array::from_fn(|k| ray.origin[k] + ray.direction[k] * t);
                kept(point[2]).then_some((t, point, normal))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, point, normal)| SurfaceHit {
                object: index,
                point,
                // Towards the viewer, into the void.
                normal: normal.map(|c| -c),
            })
    }

    /// The map from where object `index` was when the latest result was prepared to where it is
    /// now, while that result still describes it apart from its placement (TD-089): the same
    /// model, hollowing, repairs and printer, and with `holes`, the same drain holes. A moved,
    /// turned or scaled object then shows its earlier cavity and holes at once, following it,
    /// while it is prepared again.
    #[allow(clippy::float_cmp)] // Exactly the same settings.
    #[must_use]
    pub fn follow(&self, workspace: &Workspace, index: usize, holes: bool) -> Option<Mat4> {
        let (_, _, then) = self.result.as_ref()?;
        let now = workspace.plate();
        let (before, after) = (then.objects.get(index)?, now.objects.get(index)?);
        let same = before.model == after.model
            && before.units == after.units
            && before.hollowing() == after.hollowing()
            && then.settings.repair == now.settings.repair
            && then.printer == now.printer
            && (!holes || before.drain_holes().eq(after.drain_holes()));
        if !same {
            return None;
        }
        let back = self.prepared_at.get(index)?.as_ref()?;
        let forward = affine(&workspace.placement_of(index)?)?;
        Some(carried(back, &forward))
    }

    /// The holes of object `index` as last probed, and the map that carries them to where the
    /// object is now ([`Self::follow`]), while the plate's current holes cannot be probed yet.
    #[must_use]
    pub fn held_verdicts(
        &self,
        workspace: &Workspace,
        index: usize,
    ) -> Option<(u64, &[Verdict], Mat4)> {
        let (revision, verdicts) = self.held.get(&index)?;
        let follow = self.follow(workspace, index, true)?;
        Some((*revision, verdicts.as_slice(), follow))
    }

    /// Upload the current result's meshes, freeing those of older results.
    pub fn sync(&mut self, viewport: &mut Viewport, state: &egui_wgpu::RenderState) {
        if self.uploaded.0 == self.generation {
            return;
        }
        for old in self.uploaded.1.drain(..) {
            viewport.remove(old.key);
        }
        self.uploaded.0 = self.generation;
        let Some((_, preparation, _)) = &self.result else {
            return;
        };
        let mut next = PREVIEW_KEYS | (self.generation & 0xFFFF_FFFF) << 16;
        for (index, object) in preparation
            .objects
            .iter()
            .enumerate()
            .filter_map(|(i, o)| Some((i, o.as_ref().ok()?)))
        {
            let mut meshes: Vec<(&TriangleMesh, Kind)> = Vec::new();
            if let Some(hollow) = object.hollowed() {
                meshes.push((hollow.cavity(), Kind::Cavity));
            }
            for (mesh, kind) in meshes {
                if mesh.triangles().is_empty() {
                    continue;
                }
                next += 1;
                let origin = centre(mesh);
                viewport.upload(&state.device, next, mesh, origin);
                self.uploaded.1.push(Uploaded {
                    key: next,
                    origin,
                    kind,
                    object: index,
                });
            }
        }
    }

    /// The preview meshes to draw, while the result describes the plate; a cavity is opened by
    /// the holes `holes` gives for its object. Object `inside`'s cavity is seen from inside (the
    /// cavity view), so it is not capped.
    #[must_use]
    pub fn draws(
        &self,
        workspace: &Workspace,
        appearance: &Appearance,
        selected: Option<usize>,
        inside: Option<usize>,
        holes: impl Fn(usize) -> [u32; 2],
    ) -> Vec<Draw> {
        if self.uploaded.0 != self.generation {
            return Vec::new();
        }
        let current = self.current(workspace).is_some();
        self.uploaded
            .1
            .iter()
            // A hidden object's previews hide with it.
            .filter(|u| !workspace.is_hidden(u.object))
            .filter_map(|u| {
                // An outdated result still shows a cavity whose object only moved, carried to
                // where the object is now; supports wait for the new result.
                let model = if current {
                    translation(u.origin)
                } else if u.kind == Kind::Cavity {
                    self.follow(workspace, u.object, false)?
                        .mul(&translation(u.origin))
                } else {
                    return None;
                };
                Some((u, model))
            })
            .map(|(u, model)| Draw {
                mesh: u.key,
                model,
                color: u
                    .kind
                    .color(Some(u.object) == selected, appearance)
                    .linear(),
                overlay: false,
                // A cavity counts in the section's parity: its cap is the wall around it.
                cap: if u.kind == Kind::Cavity && Some(u.object) != inside {
                    crate::viewport::Cap::Model
                } else {
                    crate::viewport::Cap::Off
                },
                // The object's holes open its cavity too.
                holes: if u.kind == Kind::Cavity {
                    holes(u.object)
                } else {
                    [0, 0]
                },
                bore: None,
                outside: None,
                triangles: None,
                look: if Some(u.object) == inside {
                    crate::viewport::Look::Inside
                } else {
                    crate::viewport::Look::Solid
                },
            })
            .collect()
    }

    /// The preparation's state for the status bar: its progress, or what it found wrong.
    #[must_use]
    pub fn statuses(&self, workspace: &Workspace) -> Vec<Status> {
        if self.job.is_some() {
            let text = tr!("preparation-running", phase = &self.phase);
            return vec![Status {
                busy: true,
                ..Status::new(text, theme::TEXT_PRIMARY.color())
            }];
        }
        let revision = workspace.revision();
        if let Some((_, reason)) = self.refused.as_ref().filter(|(r, _)| *r == revision) {
            return vec![Status {
                detail: Some(reason.clone()),
                ..Status::new(tr!("preparation-refused"), theme::ERROR.color())
            }];
        }
        let Some(preparation) = self.current(workspace) else {
            return Vec::new();
        };
        let failed: Vec<String> = preparation
            .objects
            .iter()
            .zip(&workspace.plate().objects)
            .filter_map(|(result, object)| {
                result
                    .as_ref()
                    .err()
                    .map(|e| format!("{}: {}", object.name, e.reason()))
            })
            .collect();
        let mut statuses = Vec::new();
        if !failed.is_empty() {
            let text = tr!("preparation-objects-failed", count = failed.len());
            statuses.push(Status {
                detail: Some(failed.join("\n")),
                ..Status::new(text, theme::ERROR.color())
            });
        }
        statuses
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use karasu_project::project::Hollowing;

    #[test]
    fn the_selected_models_cavity_has_its_own_colour() {
        let appearance = Appearance::default();
        assert_ne!(appearance.cavity, appearance.cavity_selected);
        assert_eq!(Kind::Cavity.color(false, &appearance), appearance.cavity);
        assert_eq!(
            Kind::Cavity.color(true, &appearance),
            appearance.cavity_selected
        );
    }

    #[test]
    fn a_hollowed_plate_is_prepared_after_a_quiet_pause_and_outdated_by_edits() {
        let directory = std::env::temp_dir().join(format!("karasu-prep-ui-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("block.stl");
        let stl = crate::workspace::tests::box_stl([0.0; 3], [12.0, 12.0, 8.0]);
        std::fs::write(&path, stl).unwrap();
        let mut workspace = Workspace::new();
        workspace.open(path);
        crate::workspace::tests::wait(&mut workspace);
        let (printer, resin, setting) =
            crate::workspace::tests::catalog_profiles("anycubic-photon-mono-x");
        let mut ui = PreparationUi::default();
        // Nothing to prepare yet.
        assert!(!ui.poll(&workspace));
        workspace.edit(|plate| {
            plate.printer = Some(printer);
            plate.resin = Some(resin);
            plate.print_setting = Some(setting);
            plate.objects[0].set_hollowing(Some(Hollowing {
                wall_mm: 2.0,
                cell_mm: Some(0.5),
                simplify: None,
            }));
        });
        // Prepared once the plate has been quiet for the debounce time.
        let started = Instant::now();
        while ui.current(&workspace).is_none() {
            assert!(
                started.elapsed() < Duration::from_secs(60),
                "no preparation"
            );
            ui.poll(&workspace);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(started.elapsed() >= DEBOUNCE);
        let object = ui.current(&workspace).unwrap().objects[0].as_ref().unwrap();
        assert!(object.hollowed().is_some());
        // An edit outdates it. A move keeps its cavity shown, carried by the move (TD-089),
        // until the plate is prepared again.
        workspace.edit(|plate| plate.objects[0].translate_mm[0] += 1.0);
        assert!(ui.current(&workspace).is_none());
        let follow = ui.follow(&workspace, 0, false).unwrap();
        let moved = follow.project([3.0, 4.0, 5.0]).unwrap();
        for (k, expected) in [4.0, 4.0, 5.0].into_iter().enumerate() {
            assert!((moved[k] - expected).abs() < 1e-9, "{moved:?}");
        }
        // Another wall is another cavity: nothing is carried.
        workspace.edit(|plate| {
            plate.objects[0].set_hollowing(Some(Hollowing {
                wall_mm: 2.5,
                cell_mm: Some(0.5),
                simplify: None,
            }));
        });
        assert!(ui.follow(&workspace, 0, false).is_none());
        assert!(ui.poll(&workspace));
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn preview_meshes_are_placed_from_their_centre() {
        let mesh = TriangleMesh::try_new(
            vec![
                karasu_core::geometry::Point3::new(0.0, 0.0, 0.0),
                karasu_core::geometry::Point3::new(10.0, 0.0, 0.0),
                karasu_core::geometry::Point3::new(0.0, 20.0, 4.0),
            ],
            vec![[0, 1, 2]],
        )
        .unwrap();
        let origin = centre(&mesh);
        assert_eq!(origin, [5.0, 10.0, 2.0]);
        // A vertex stored relative to the centre returns to its world position.
        assert_eq!(
            translation(origin).project([-5.0, 10.0, 2.0]),
            Some([0.0, 20.0, 4.0])
        );
    }
}
