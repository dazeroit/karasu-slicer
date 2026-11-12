//! The forms the profile dialogs share: a printer's and a resin's facts, the panel orientation
//! shown as an "F", the formats chosen by readable name, and a catalog printer's facts (TD-106).
use crate::{
    i18n::tr,
    theme,
    widgets::{fact_row, text_field, Unit, UnitField},
};
use eframe::egui;
use karasu_core::profiles::{
    swatch_rgb, PrinterLimits, PrinterProfile, ResinKind, ResinMaterial, LIBRARY_SCHEMA_VERSION,
};
use karasu_project::{
    library::Library,
    request::{format_name, format_names, FormatName},
};

/// The width of a text field or menu in a form.
pub(super) const WIDE_FIELD: f32 = 220.0;
/// The width of a printer form's two columns.
const FORM_COLUMN: f32 = 380.0;
/// An orientation tile's width: two side by side fill a form column.
const TILE_WIDTH: f32 = 180.0;

/// A labelled field in a form's grid: the label, then the field, then the row's end.
pub(super) fn form_row<R: Field>(
    ui: &mut egui::Ui,
    label: impl Into<egui::WidgetText>,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let label = label.into();
    let text = label.text().to_owned();
    ui.add(egui::Label::new(label).wrap());
    let inner = add(ui);
    // The field is named by the row's label for screen readers (TD-142).
    crate::widgets::name(inner.response(), &text);
    ui.end_row();
    inner
}

/// What a form row's field hands back: the response of the control the row's label names.
pub(super) trait Field {
    fn response(&self) -> &egui::Response;
}

impl Field for egui::Response {
    fn response(&self) -> &egui::Response {
        self
    }
}

impl<T> Field for egui::InnerResponse<T> {
    fn response(&self) -> &egui::Response {
        &self.response
    }
}

/// A form's grid: labels in the first column, fields in the second.
pub(super) fn form_grid<R>(ui: &mut egui::Ui, id: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([12.0, 6.0])
        .min_col_width(110.0)
        .show(ui, add)
        .inner
}

/// A format as the interface shows it: its name and extension.
pub(super) fn format_label(format: &FormatName) -> String {
    format!("{} (.{})", format.name, format.extension)
}

/// A format identifier's readable name with its family, or the identifier when it has none.
pub(super) fn format_text(id: &str) -> String {
    format_name(id).map_or_else(
        || id.to_owned(),
        |f| format!("{} · {}", f.family, format_label(&f)),
    )
}

/// How the panel shows the image, as the user chooses it (TD-104): a half turn is both mirrors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Orientation {
    AsPreview,
    MirrorXy,
    MirrorX,
    MirrorY,
}

impl Orientation {
    const ALL: [Self; 4] = [
        Self::AsPreview,
        Self::MirrorXy,
        Self::MirrorX,
        Self::MirrorY,
    ];

    const fn of(printer: &PrinterProfile) -> Self {
        match (printer.mirror_x, printer.mirror_y) {
            (false, false) => Self::AsPreview,
            (true, true) => Self::MirrorXy,
            (true, false) => Self::MirrorX,
            (false, true) => Self::MirrorY,
        }
    }

    const fn mirrors(self) -> (bool, bool) {
        match self {
            Self::AsPreview => (false, false),
            Self::MirrorXy => (true, true),
            Self::MirrorX => (true, false),
            Self::MirrorY => (false, true),
        }
    }

    fn label(self) -> String {
        match self {
            Self::AsPreview => tr!("orientation-as-preview"),
            Self::MirrorXy => tr!("orientation-mirror-xy"),
            Self::MirrorX => tr!("orientation-mirror-x"),
            Self::MirrorY => tr!("orientation-mirror-y"),
        }
    }
}

/// An "F" as the panel shows it with `orientation`, in `rect`.
fn paint_f(
    painter: &egui::Painter,
    rect: egui::Rect,
    orientation: Orientation,
    colour: egui::Color32,
) {
    let (flip_x, flip_y) = orientation.mirrors();
    // The glyph in unit coordinates, y down: a stem and two arms.
    let strokes = [
        [(0.3, 0.15), (0.3, 0.85)],
        [(0.3, 0.15), (0.72, 0.15)],
        [(0.3, 0.48), (0.62, 0.48)],
    ];
    let at = |(x, y): (f32, f32)| {
        let x = if flip_x { 1.0 - x } else { x };
        let y = if flip_y { 1.0 - y } else { y };
        rect.min + egui::vec2(x * rect.width(), y * rect.height())
    };
    for [a, b] in strokes {
        painter.line_segment([at(a), at(b)], egui::Stroke::new(3.0, colour));
    }
}

/// One orientation's tile: its "F" and its name, wrapping, selected or not.
fn orientation_tile(ui: &mut egui::Ui, orientation: Orientation, selected: bool) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(TILE_WIDTH, 52.0), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::RadioButton,
            true,
            selected,
            orientation.label(),
        )
    });
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact_selectable(&response, selected);
        ui.painter().rect(
            rect,
            6.0,
            visuals.weak_bg_fill,
            visuals.bg_stroke,
            egui::StrokeKind::Inside,
        );
        let glyph =
            egui::Rect::from_min_size(rect.min + egui::vec2(8.0, 8.0), egui::vec2(36.0, 36.0));
        ui.painter()
            .rect_filled(glyph, 4.0, theme::BACKGROUND.color());
        paint_f(
            ui.painter(),
            glyph.shrink(5.0),
            orientation,
            theme::ACCENT.color(),
        );
        let galley = ui.painter().layout(
            orientation.label(),
            egui::FontId::proportional(13.0),
            visuals.text_color(),
            rect.width() - 60.0,
        );
        let at = egui::pos2(rect.left() + 52.0, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(at, galley, visuals.text_color());
    }
    response
}

/// The orientation choice, each with its "F".
fn orientation_choice(ui: &mut egui::Ui, chosen: &mut Orientation) {
    crate::widgets::group(ui, tr!("orientation-title"), |ui| {
        ui.add(egui::Label::new(egui::RichText::new(tr!("orientation-explain")).small()).wrap());
        ui.add_space(4.0);
        for pair in Orientation::ALL.chunks(2) {
            ui.horizontal(|ui| {
                for &orientation in pair {
                    let selected = *chosen == orientation;
                    if orientation_tile(ui, orientation, selected).clicked() {
                        *chosen = orientation;
                    }
                }
            });
        }
        ui.add(egui::Label::new(egui::RichText::new(tr!("orientation-test-hint")).small()).wrap());
    });
}

/// karasu-slicer's validation bounds for a printer entered by hand (the catalog's too).
const LIMITS: PrinterLimits = PrinterLimits {
    min_layer_height_mm: 0.01,
    max_layer_height_mm: 0.2,
    max_exposure_s: 300.0,
    max_speed_mm_s: 20.0,
    max_lift_mm: 20.0,
};

/// A printer's editable facts, for a custom printer and for editing one.
#[derive(Clone)]
pub(super) struct PrinterForm {
    pub(super) name: String,
    vendor: Option<String>,
    columns: f64,
    rows: f64,
    build: [f64; 3],
    /// The fixed time per layer in seconds (TD-122); 0 states none.
    fixed_layer_s: f64,
    orientation: Orientation,
    formats: Vec<String>,
    /// The format menu's search.
    query: String,
    /// The format menu shows every format, not only the vendor's.
    all_formats: bool,
    /// The printer edited, or `None` for a new one.
    base: Option<PrinterProfile>,
}

impl PrinterForm {
    pub(super) fn new(name: String) -> Self {
        Self {
            name,
            vendor: None,
            columns: 2560.0,
            rows: 1440.0,
            build: [120.0, 68.0, 150.0],
            fixed_layer_s: 0.0,
            orientation: Orientation::AsPreview,
            formats: Vec::new(),
            query: String::new(),
            all_formats: false,
            base: None,
        }
    }

    pub(super) fn edit(printer: &PrinterProfile) -> Self {
        Self {
            name: printer.name.clone(),
            vendor: printer.vendor.clone(),
            columns: f64::from(printer.resolution_px[0]),
            rows: f64::from(printer.resolution_px[1]),
            build: printer.build_size_mm,
            fixed_layer_s: printer.fixed_layer_time_s.unwrap_or(0.0),
            orientation: Orientation::of(printer),
            formats: printer.format_ids.clone(),
            query: String::new(),
            all_formats: false,
            base: Some(printer.clone()),
        }
    }

    /// The printer the form describes (identity and revision are the library's to set).
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Fields hold whole pixel counts in range.
    pub(super) fn printer(&self) -> PrinterProfile {
        let (mirror_x, mirror_y) = self.orientation.mirrors();
        let mut printer = self.base.clone().unwrap_or_else(|| PrinterProfile {
            schema_version: LIBRARY_SCHEMA_VERSION,
            id: "new-printer".into(),
            name: String::new(),
            revision: 1,
            build_size_mm: [0.0; 3],
            resolution_px: [0; 2],
            origin_xy_mm: [0.0; 2],
            swap_xy: false,
            mirror_x: false,
            mirror_y: false,
            format_ids: Vec::new(),
            limits: LIMITS,
            vendor: None,
            model: None,
            based_on: None,
            fixed_layer_time_s: None,
        });
        self.name.trim().clone_into(&mut printer.name);
        printer.vendor.clone_from(&self.vendor);
        printer.resolution_px = [self.columns.round() as u32, self.rows.round() as u32];
        printer.build_size_mm = self.build;
        printer.fixed_layer_time_s = (self.fixed_layer_s > 0.0).then_some(self.fixed_layer_s);
        printer.mirror_x = mirror_x;
        printer.mirror_y = mirror_y;
        printer.format_ids.clone_from(&self.formats);
        printer
    }

    /// Whether the form changed the printer it edits.
    pub(super) fn changed(&self) -> bool {
        self.base.as_ref() != Some(&self.printer())
    }

    /// Whether the form describes a printer that can be saved: a name and a format.
    pub(super) fn complete(&self) -> bool {
        !self.name.trim().is_empty() && !self.formats.is_empty()
    }

    pub(super) fn ui(&mut self, ui: &mut egui::Ui, library: &Library) {
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(FORM_COLUMN, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(FORM_COLUMN);
                    self.facts(ui, library);
                    ui.add_space(8.0);
                    self.formats(ui, library);
                },
            );
            ui.add_space(16.0);
            ui.allocate_ui_with_layout(
                egui::vec2(FORM_COLUMN, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(FORM_COLUMN);
                    orientation_choice(ui, &mut self.orientation);
                },
            );
        });
    }

    fn facts(&mut self, ui: &mut egui::Ui, library: &Library) {
        crate::widgets::group(ui, tr!("printer-identity"), |ui| {
            form_grid(ui, "printer-identity", |ui| {
                form_row(ui, tr!("field-name"), |ui| {
                    ui.add(text_field(&mut self.name, WIDE_FIELD))
                });
                form_row(ui, tr!("field-vendor"), |ui| {
                    vendor_choice(ui, "printer-vendor", library, &mut self.vendor)
                });
            });
        });
        crate::widgets::group(ui, tr!("printer-panel"), |ui| {
            form_grid(ui, "printer-panel", |ui| {
                for (value, label) in [
                    (&mut self.columns, tr!("field-pixels-x")),
                    (&mut self.rows, tr!("field-pixels-y")),
                ] {
                    form_row(ui, label, |ui| {
                        ui.add(
                            UnitField::new(value, Unit::Count)
                                .range(1.0..=65_535.0)
                                .step(1.0)
                                .decimals(0),
                        )
                    });
                }
                for (k, label) in [tr!("field-width"), tr!("field-depth"), tr!("field-height")]
                    .into_iter()
                    .enumerate()
                {
                    form_row(ui, label, |ui| {
                        ui.add(
                            UnitField::new(&mut self.build[k], Unit::Millimetre)
                                .range(1.0..=1000.0)
                                .decimals(3),
                        )
                    });
                }
            });
        });
        crate::widgets::group(ui, tr!("printer-timing"), |ui| {
            form_grid(ui, "printer-timing", |ui| {
                form_row(ui, tr!("field-fixed-layer-time"), |ui| {
                    ui.add(
                        UnitField::new(&mut self.fixed_layer_s, Unit::Second)
                            .range(karasu_core::profiles::FIXED_LAYER_TIME_S)
                            .step(0.01)
                            .decimals(2),
                    )
                    .on_hover_text(tr!("field-fixed-layer-time-hint"))
                });
            });
        });
    }

    /// The formats chosen, each with its remove button, then "Add format…", a menu with a search
    /// and the formats grouped by family: the vendor's, or every one.
    fn formats(&mut self, ui: &mut egui::Ui, library: &Library) {
        crate::widgets::group(ui, tr!("field-formats"), |ui| {
            if self.formats.is_empty() {
                ui.label(
                    egui::RichText::new(tr!("formats-none"))
                        .small()
                        .color(theme::WARNING.color()),
                );
            }
            let mut removed = None;
            for (k, id) in self.formats.iter().enumerate() {
                ui.horizontal(|ui| {
                    let icons = crate::icons::Icons::shared(ui.ctx());
                    if crate::widgets::icon_button(
                        ui,
                        icons.as_deref(),
                        crate::icons::Icon::Remove,
                        false,
                        &tr!("formats-remove"),
                    )
                    .clicked()
                    {
                        removed = Some(k);
                    }
                    ui.add(egui::Label::new(format_text(id)).wrap());
                });
            }
            if let Some(k) = removed {
                self.formats.remove(k);
            }
            let vendor = self
                .vendor
                .as_ref()
                .and_then(|v| library.catalog().vendor(v))
                .filter(|v| !v.formats.is_empty());
            let button = ui.button(tr!("formats-add"));
            let opened = button.clicked();
            egui::Popup::from_toggle_button_response(&button)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .width(WIDE_FIELD + 120.0)
                .show(|ui| {
                    let search = ui.add(
                        text_field(&mut self.query, ui.available_width())
                            .hint_text(tr!("catalog-search")),
                    );
                    if opened {
                        search.request_focus();
                    }
                    if let Some(vendor) = vendor {
                        ui.checkbox(&mut self.all_formats, tr!("formats-all"))
                            .on_hover_text(tr!("formats-all-hint", vendor = vendor.name.clone()));
                    }
                    let query = self.query.trim().to_lowercase();
                    let names: Vec<FormatName> = format_names()
                        .into_iter()
                        .filter(|f| !self.formats.contains(&f.id))
                        .filter(|f| {
                            self.all_formats || vendor.is_none_or(|v| v.formats.contains(&f.id))
                        })
                        .filter(|f| {
                            query.is_empty()
                                || format!("{} {} {}", f.family, f.name, f.extension)
                                    .to_lowercase()
                                    .contains(&query)
                        })
                        .collect();
                    ui.separator();
                    crate::focus::scrolled(
                        egui::ScrollArea::vertical()
                            .id_salt("formats-menu")
                            .max_height(320.0),
                        ui,
                        |ui| {
                            let mut family = "";
                            for format in &names {
                                if format.family != family {
                                    family = format.family;
                                    ui.label(
                                        egui::RichText::new(family)
                                            .small()
                                            .strong()
                                            .color(ui.visuals().weak_text_color()),
                                    );
                                }
                                let entry = egui::Button::selectable(
                                    false,
                                    (format_label(format), egui::Atom::grow()),
                                );
                                if ui.add_sized([ui.available_width(), 0.0], entry).clicked() {
                                    self.formats.push(format.id.clone());
                                }
                            }
                            if names.is_empty() {
                                ui.label(tr!("catalog-no-match-short"));
                            }
                        },
                    );
                });
        });
    }
}

/// The vendor menu: none, or one of the catalog's vendors.
fn vendor_choice(
    ui: &mut egui::Ui,
    id: &str,
    library: &Library,
    vendor: &mut Option<String>,
) -> egui::InnerResponse<Option<()>> {
    let label = vendor
        .as_ref()
        .and_then(|v| library.catalog().vendor(v))
        .map_or_else(|| tr!("vendor-none"), |v| v.name.clone());
    egui::ComboBox::from_id_salt(id)
        .selected_text(label)
        .width(WIDE_FIELD)
        .show_ui(ui, |ui| {
            ui.selectable_value(vendor, None, tr!("vendor-none"));
            for v in library.catalog().vendors() {
                ui.selectable_value(vendor, Some(v.id.clone()), &v.name);
            }
        })
}

const KINDS: [ResinKind; 9] = [
    ResinKind::Standard,
    ResinKind::AbsLike,
    ResinKind::WaterWashable,
    ResinKind::Tough,
    ResinKind::Flexible,
    ResinKind::PlantBased,
    ResinKind::Clear,
    ResinKind::Castable,
    ResinKind::Other,
];

pub(super) fn kind_label(kind: Option<ResinKind>) -> String {
    match kind {
        None => tr!("kind-not-stated"),
        Some(ResinKind::Standard) => tr!("kind-standard"),
        Some(ResinKind::AbsLike) => tr!("kind-abs-like"),
        Some(ResinKind::WaterWashable) => tr!("kind-water-washable"),
        Some(ResinKind::Tough) => tr!("kind-tough"),
        Some(ResinKind::Flexible) => tr!("kind-flexible"),
        Some(ResinKind::PlantBased) => tr!("kind-plant-based"),
        Some(ResinKind::Clear) => tr!("kind-clear"),
        Some(ResinKind::Castable) => tr!("kind-castable"),
        Some(ResinKind::Other) => tr!("kind-other"),
    }
}

/// A resin's editable facts, for a custom resin and for editing one.
#[derive(Clone)]
pub(super) struct ResinForm {
    name: String,
    vendor: Option<String>,
    kind: Option<ResinKind>,
    density: f64,
    has_density: bool,
    cost: f64,
    has_cost: bool,
    currency: String,
    colour: String,
    swatch: [u8; 3],
    has_swatch: bool,
    base: Option<ResinMaterial>,
}

impl ResinForm {
    pub(super) fn new() -> Self {
        Self {
            name: String::new(),
            vendor: None,
            kind: None,
            density: 1.1,
            has_density: false,
            cost: 30.0,
            has_cost: false,
            currency: "EUR".into(),
            colour: String::new(),
            swatch: [128, 128, 128],
            has_swatch: false,
            base: None,
        }
    }

    pub(super) fn edit(resin: &ResinMaterial) -> Self {
        Self {
            name: resin.name.clone(),
            vendor: resin.vendor.clone(),
            kind: resin.kind,
            density: resin.density_g_per_ml.unwrap_or(1.1),
            has_density: resin.density_g_per_ml.is_some(),
            cost: resin.cost_per_liter.unwrap_or(30.0),
            has_cost: resin.cost_per_liter.is_some(),
            currency: resin.currency.clone().unwrap_or_else(|| "EUR".into()),
            colour: resin.colour.clone().unwrap_or_default(),
            swatch: resin
                .swatch
                .as_deref()
                .and_then(swatch_rgb)
                .unwrap_or([128, 128, 128]),
            has_swatch: resin.swatch.as_deref().and_then(swatch_rgb).is_some(),
            base: Some(resin.clone()),
        }
    }

    pub(super) fn material(&self) -> ResinMaterial {
        let mut resin = self.base.clone().unwrap_or_else(|| ResinMaterial {
            colour: None,
            swatch: None,
            schema_version: LIBRARY_SCHEMA_VERSION,
            id: "new-resin".into(),
            name: String::new(),
            revision: 1,
            vendor: None,
            kind: None,
            density_g_per_ml: None,
            cost_per_liter: None,
            currency: None,
            based_on: None,
        });
        self.name.trim().clone_into(&mut resin.name);
        resin.vendor.clone_from(&self.vendor);
        resin.kind = self.kind;
        resin.density_g_per_ml = self.has_density.then_some(self.density);
        resin.cost_per_liter = self.has_cost.then_some(self.cost);
        resin.currency = self.has_cost.then(|| self.currency.trim().to_uppercase());
        let colour = self.colour.trim();
        resin.colour = (!colour.is_empty()).then(|| colour.to_owned());
        resin.swatch = self.has_swatch.then(|| {
            let [r, g, b] = self.swatch;
            format!("#{r:02X}{g:02X}{b:02X}")
        });
        resin
    }

    /// Whether the form changed the resin it edits.
    pub(super) fn changed(&self) -> bool {
        self.base.as_ref() != Some(&self.material())
    }

    pub(super) fn ui(&mut self, ui: &mut egui::Ui, library: &Library) {
        form_grid(ui, "resin-form", |ui| {
            form_row(ui, tr!("field-name"), |ui| {
                ui.add(text_field(&mut self.name, WIDE_FIELD))
            });
            form_row(ui, tr!("field-vendor"), |ui| {
                vendor_choice(ui, "resin-vendor", library, &mut self.vendor)
            });
            form_row(ui, tr!("field-kind"), |ui| {
                egui::ComboBox::from_id_salt("resin-kind")
                    .selected_text(kind_label(self.kind))
                    .width(WIDE_FIELD)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.kind, None, kind_label(None));
                        for kind in KINDS {
                            ui.selectable_value(&mut self.kind, Some(kind), kind_label(Some(kind)));
                        }
                    })
            });
            form_row(ui, tr!("field-colour"), |ui| {
                ui.add(text_field(&mut self.colour, WIDE_FIELD))
                    .on_hover_text(tr!("field-colour-hint"))
            });
        });
        crate::widgets::check_value_row(ui, &mut self.has_swatch, tr!("field-swatch"), |ui| {
            ui.color_edit_button_srgb(&mut self.swatch)
                .on_hover_text(tr!("field-swatch-hint"))
        });
        crate::widgets::check_value_row(ui, &mut self.has_density, tr!("field-density"), |ui| {
            ui.add(
                UnitField::new(&mut self.density, Unit::Count)
                    .range(0.5..=3.0)
                    .decimals(2)
                    .step(0.01),
            )
        });
        crate::widgets::check_value_row(ui, &mut self.has_cost, tr!("field-cost"), |ui| {
            ui.horizontal(|ui| {
                let cost = ui.add(
                    UnitField::new(&mut self.cost, Unit::Count)
                        .range(0.0..=10_000.0)
                        .decimals(2),
                );
                let currency = ui.add(text_field(&mut self.currency, 48.0).char_limit(3));
                crate::widgets::name(&currency, &tr!("a11y-currency"));
                // The cost first: the row names it; a change of either is the row's.
                cost.union(currency)
            })
            .inner
        });
    }
}

/// The catalog printer's facts and what has been verified about it.
pub(super) fn catalog_facts(ui: &mut egui::Ui, library: &Library, id: &str) {
    let Some(entry) = library.catalog().printer(id) else {
        return;
    };
    let p = &entry.profile;
    let [w, d, h] = p.build_size_mm;
    let pitch = |mm: f64, px: u32| mm * 1000.0 / f64::from(px);
    fact_row(
        ui,
        &tr!("field-panel"),
        &format!("{} × {}", p.resolution_px[0], p.resolution_px[1]),
    );
    fact_row(
        ui,
        &tr!("field-pixel"),
        &format!(
            "{:.1} × {:.1} µm",
            pitch(w, p.resolution_px[0]),
            pitch(d, p.resolution_px[1])
        ),
    );
    fact_row(
        ui,
        &tr!("field-build-volume"),
        &format!("{w} × {d} × {h} mm"),
    );
    let formats: Vec<String> = p.format_ids.iter().map(|id| format_text(id)).collect();
    fact_row(ui, &tr!("field-formats"), &formats.join("\n"));
    fact_row(
        ui,
        &tr!("field-release"),
        &match entry.release {
            karasu_core::profiles::catalog::Release::ZLift => tr!("release-z-lift"),
            karasu_core::profiles::catalog::Release::Tilt => tr!("release-tilt"),
        },
    );
    fact_row(
        ui,
        &tr!("field-orientation"),
        &if entry.orientation_verified {
            tr!("orientation-verified")
        } else {
            tr!("orientation-not-verified")
        },
    )
    .on_hover_text(tr!("orientation-hint"));
    fact_row(ui, &tr!("field-sources"), &entry.sources);
}

#[cfg(test)]
mod tests {
    use super::ResinForm;

    #[test]
    fn a_resins_colour_and_swatch_are_kept_and_left_out_when_empty() {
        let mut form = ResinForm::new();
        form.name = "Standard".into();
        let plain = form.material();
        assert_eq!((plain.colour, plain.swatch), (None, None));
        form.colour = "  Grey ".into();
        form.has_swatch = true;
        form.swatch = [0x7A, 0x7B, 0x0C];
        let grey = form.material();
        assert_eq!(grey.colour.as_deref(), Some("Grey"));
        assert_eq!(grey.swatch.as_deref(), Some("#7A7B0C"));
        // Edited again, the form shows them as saved, and changes nothing by itself.
        let again = ResinForm::edit(&grey);
        assert_eq!(again.material(), grey);
        assert!(!again.changed());
    }
}
