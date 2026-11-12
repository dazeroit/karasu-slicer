//! The interface's icons (M6.5): original line icons drawn by `tools/assets/make_icons.py`
//! into `assets/ui/icons/`, bundled in the executable.
//!
//! Each icon is a 96 x 96 PNG, white, with the strokes' coverage as alpha. It is uploaded once as
//! a texture, with mipmaps so it stays smooth at the small sizes it is drawn at, and tinted when
//! drawn, so one image serves every state and follows the theme's colours.
use eframe::egui;

/// Decoded straight-alpha RGBA pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    /// Width and height in pixels.
    pub size: [usize; 2],
    /// Row-major RGBA bytes.
    pub rgba: Vec<u8>,
}

/// Decode an 8-bit RGBA PNG.
///
/// # Errors
/// A message when the PNG is malformed or not 8-bit RGBA.
pub fn decode_png(bytes: &[u8]) -> Result<Image, String> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let size = reader.output_buffer_size().ok_or("image is too large")?;
    let mut rgba = vec![0; size];
    let info = reader.next_frame(&mut rgba).map_err(|e| e.to_string())?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Err("image must be 8-bit RGBA".to_owned());
    }
    rgba.truncate(info.buffer_size());
    Ok(Image {
        size: [info.width as usize, info.height as usize],
        rgba,
    })
}

/// The context key of the shared icons.
const SHARED: &str = "karasu-icons";

/// An icon of the interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    Select,
    Transform,
    Repair,
    Hollow,
    DrainHoles,
    /// A support under a model: the Automatic tool and the Manual tool's Place.
    Supports,
    Slice,
    /// The Layers tool's (M6.5): one layer seen from above.
    Layers,
    /// A larger view.
    Enlarge,
    /// The Transform panel's (M6.5).
    Rotate,
    Scale,
    LockOpen,
    LockClosed,
    Reset,
    Link,
    /// Notification severities (M6.5).
    Info,
    Success,
    Warning,
    Error,
    /// The library's rows (TD-106).
    Edit,
    Duplicate,
    Export,
    /// Keeping values into what exists: a support preset updated (TD-108).
    Save,
    /// The tool strip's (owner, 2026-10-29): the outside, the cavity, going round the model, a
    /// knee following the foot, the overhangs.
    SurfaceOutside,
    SurfaceCavity,
    AvoidModel,
    Knee,
    Overhangs,
    /// The cavity view: the outside as a ghost around the cavity (owner, 2026-10-29).
    CavityView,
    /// A tool's list window (owner, 2026-10-29).
    List,
    /// The Raft tool: pads on a slab.
    Raft,
    /// Only the supports' tips shown (owner, 2026-10-29).
    TipsOnly,
    /// New supports are mini supports (owner, 2026-10-30).
    Mini,
    /// The Braces tool and placing braces one by one, in a chain, or automatically (owner,
    /// 2026-10-29).
    Braces,
    BracesChain,
    BracesAuto,
    /// The Islands tool and its markers, and the local minima (owner, 2026-10-29).
    Islands,
    Minima,
    SupportIslands,
    /// The suction cups of the Drain holes tool (M6.4j).
    SuctionCups,
    /// Placing hollowing blockers (owner, 2026-11-02).
    Blockers,
    /// The Orient tool and its protected points (TD-119).
    Orient,
    Protect,
    /// A support with a plus: the Manual tool (TD-108; swapped with [`Icon::Supports`] by the
    /// owner, 2026-10-30).
    Automatic,
    LeftUnsupported,
    /// Trees of supports: merging trunks, and joining a support to another (owner, 2026-10-30).
    MergeTrunks,
    JoinSupport,
    /// Supports on a grid (TD-138): a flat underside seen from below, its tips in rows.
    GridSupports,
    Delete,
    /// Moving through layers.
    ChevronLeft,
    ChevronRight,
    /// Folding the plates bar, and opening it again (TD-135).
    ChevronUp,
    ChevronDown,
    First,
    Last,
    /// Zooming.
    Plus,
    Minus,
    /// Closing a window or a notification.
    Close,
    /// Removing an entry from a list.
    Remove,
    /// A model shown, or hidden.
    Eye,
    EyeOff,
}

impl Icon {
    /// Every icon.
    pub const ALL: [Self; 61] = [
        Self::Select,
        Self::Transform,
        Self::Repair,
        Self::Hollow,
        Self::DrainHoles,
        Self::Supports,
        Self::Slice,
        Self::Layers,
        Self::Enlarge,
        Self::Rotate,
        Self::Scale,
        Self::LockOpen,
        Self::LockClosed,
        Self::Reset,
        Self::Link,
        Self::Info,
        Self::Success,
        Self::Warning,
        Self::Error,
        Self::Edit,
        Self::Duplicate,
        Self::Export,
        Self::Save,
        Self::SurfaceOutside,
        Self::SurfaceCavity,
        Self::AvoidModel,
        Self::Knee,
        Self::Overhangs,
        Self::CavityView,
        Self::List,
        Self::Raft,
        Self::TipsOnly,
        Self::Mini,
        Self::Braces,
        Self::BracesChain,
        Self::BracesAuto,
        Self::Islands,
        Self::Minima,
        Self::SupportIslands,
        Self::SuctionCups,
        Self::Blockers,
        Self::Orient,
        Self::Protect,
        Self::Automatic,
        Self::LeftUnsupported,
        Self::MergeTrunks,
        Self::JoinSupport,
        Self::GridSupports,
        Self::Delete,
        Self::ChevronLeft,
        Self::ChevronRight,
        Self::ChevronUp,
        Self::ChevronDown,
        Self::First,
        Self::Last,
        Self::Plus,
        Self::Minus,
        Self::Close,
        Self::Remove,
        Self::Eye,
        Self::EyeOff,
    ];

    /// The icon's PNG, as bundled.
    fn png(self) -> &'static [u8] {
        match self {
            Self::Select => include_bytes!("../../../assets/ui/icons/select.png"),
            Self::Transform => include_bytes!("../../../assets/ui/icons/transform.png"),
            Self::Repair => include_bytes!("../../../assets/ui/icons/repair.png"),
            Self::Hollow => include_bytes!("../../../assets/ui/icons/hollow.png"),
            Self::DrainHoles => include_bytes!("../../../assets/ui/icons/drain-holes.png"),
            Self::Supports => include_bytes!("../../../assets/ui/icons/supports.png"),
            Self::Slice => include_bytes!("../../../assets/ui/icons/slice.png"),
            Self::Layers => include_bytes!("../../../assets/ui/icons/layers.png"),
            Self::Enlarge => include_bytes!("../../../assets/ui/icons/enlarge.png"),
            Self::Rotate => include_bytes!("../../../assets/ui/icons/rotate.png"),
            Self::Scale => include_bytes!("../../../assets/ui/icons/scale.png"),
            Self::LockOpen => include_bytes!("../../../assets/ui/icons/lock-open.png"),
            Self::LockClosed => include_bytes!("../../../assets/ui/icons/lock-closed.png"),
            Self::Reset => include_bytes!("../../../assets/ui/icons/reset.png"),
            Self::Link => include_bytes!("../../../assets/ui/icons/link.png"),
            Self::Info => include_bytes!("../../../assets/ui/icons/info.png"),
            Self::Success => include_bytes!("../../../assets/ui/icons/success.png"),
            Self::Warning => include_bytes!("../../../assets/ui/icons/warning.png"),
            Self::Error => include_bytes!("../../../assets/ui/icons/error.png"),
            Self::Edit => include_bytes!("../../../assets/ui/icons/edit.png"),
            Self::Duplicate => include_bytes!("../../../assets/ui/icons/duplicate.png"),
            Self::Export => include_bytes!("../../../assets/ui/icons/export.png"),
            Self::Save => include_bytes!("../../../assets/ui/icons/save.png"),
            Self::SurfaceOutside => include_bytes!("../../../assets/ui/icons/surface-outside.png"),
            Self::SurfaceCavity => include_bytes!("../../../assets/ui/icons/surface-cavity.png"),
            Self::AvoidModel => include_bytes!("../../../assets/ui/icons/avoid-model.png"),
            Self::Knee => include_bytes!("../../../assets/ui/icons/knee.png"),
            Self::Overhangs => include_bytes!("../../../assets/ui/icons/overhangs.png"),
            Self::CavityView => include_bytes!("../../../assets/ui/icons/cavity-view.png"),
            Self::List => include_bytes!("../../../assets/ui/icons/list.png"),
            Self::Raft => include_bytes!("../../../assets/ui/icons/raft.png"),
            Self::TipsOnly => include_bytes!("../../../assets/ui/icons/tips-only.png"),
            Self::Mini => include_bytes!("../../../assets/ui/icons/mini.png"),
            Self::Braces => include_bytes!("../../../assets/ui/icons/braces.png"),
            Self::BracesChain => include_bytes!("../../../assets/ui/icons/braces-chain.png"),
            Self::BracesAuto => include_bytes!("../../../assets/ui/icons/braces-auto.png"),
            Self::Islands => include_bytes!("../../../assets/ui/icons/islands.png"),
            Self::SuctionCups => include_bytes!("../../../assets/ui/icons/suction-cups.png"),
            Self::Blockers => include_bytes!("../../../assets/ui/icons/blockers.png"),
            Self::Orient => include_bytes!("../../../assets/ui/icons/orient.png"),
            Self::Protect => include_bytes!("../../../assets/ui/icons/protect.png"),
            Self::Minima => include_bytes!("../../../assets/ui/icons/minima.png"),
            Self::SupportIslands => {
                include_bytes!("../../../assets/ui/icons/support-islands.png")
            }
            Self::Automatic => include_bytes!("../../../assets/ui/icons/automatic.png"),
            Self::LeftUnsupported => {
                include_bytes!("../../../assets/ui/icons/left-unsupported.png")
            }
            Self::MergeTrunks => include_bytes!("../../../assets/ui/icons/merge-trunks.png"),
            Self::JoinSupport => include_bytes!("../../../assets/ui/icons/join-support.png"),
            Self::GridSupports => include_bytes!("../../../assets/ui/icons/grid-supports.png"),
            Self::Delete => include_bytes!("../../../assets/ui/icons/delete.png"),
            Self::ChevronLeft => include_bytes!("../../../assets/ui/icons/chevron-left.png"),
            Self::ChevronRight => include_bytes!("../../../assets/ui/icons/chevron-right.png"),
            Self::ChevronUp => include_bytes!("../../../assets/ui/icons/chevron-up.png"),
            Self::ChevronDown => include_bytes!("../../../assets/ui/icons/chevron-down.png"),
            Self::First => include_bytes!("../../../assets/ui/icons/first.png"),
            Self::Last => include_bytes!("../../../assets/ui/icons/last.png"),
            Self::Plus => include_bytes!("../../../assets/ui/icons/plus.png"),
            Self::Minus => include_bytes!("../../../assets/ui/icons/minus.png"),
            Self::Close => include_bytes!("../../../assets/ui/icons/close.png"),
            Self::Remove => include_bytes!("../../../assets/ui/icons/remove.png"),
            Self::Eye => include_bytes!("../../../assets/ui/icons/eye.png"),
            Self::EyeOff => include_bytes!("../../../assets/ui/icons/eye-off.png"),
        }
    }
}

/// The icons' textures, uploaded once.
#[derive(Clone)]
pub struct Icons {
    textures: Vec<(Icon, egui::TextureHandle)>,
}

impl Icons {
    /// Upload every icon; an icon that cannot be decoded is left out and logged, and its buttons
    /// show their label alone.
    #[must_use]
    pub fn load(ctx: &egui::Context) -> Self {
        let options = egui::TextureOptions {
            mipmap_mode: Some(egui::TextureFilter::Linear),
            ..egui::TextureOptions::LINEAR
        };
        let textures = Icon::ALL
            .into_iter()
            .filter_map(|icon| match decode_png(icon.png()) {
                Ok(image) => {
                    let pixels = egui::ColorImage::from_rgba_unmultiplied(image.size, &image.rgba);
                    let name = format!("icon-{icon:?}");
                    Some((icon, ctx.load_texture(name, pixels, options)))
                }
                Err(error) => {
                    log::error!(target: "app", "icon {icon:?}: {error}");
                    None
                }
            })
            .collect();
        let icons = Self { textures };
        // Shared through the context too, for the dialogs drawn far from the window's state.
        ctx.data_mut(|d| d.insert_temp(egui::Id::new(SHARED), std::sync::Arc::new(icons.clone())));
        icons
    }

    /// The icons uploaded for `ctx`, if they were: for code that is not handed them, such as
    /// the dialogs' frame.
    #[must_use]
    pub fn shared(ctx: &egui::Context) -> Option<std::sync::Arc<Self>> {
        ctx.data(|d| d.get_temp(egui::Id::new(SHARED)))
    }

    /// Draw `icon` filling `rect`, tinted `colour`.
    pub fn paint(
        &self,
        painter: &egui::Painter,
        icon: Icon,
        rect: egui::Rect,
        colour: egui::Color32,
    ) {
        if let Some((_, texture)) = self.textures.iter().find(|(i, _)| *i == icon) {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            painter.image(texture.id(), rect, uv, colour);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_a_white_square_with_transparent_corners_and_drawn_strokes() {
        for icon in Icon::ALL {
            let image = decode_png(icon.png()).unwrap();
            assert_eq!(image.size, [96, 96], "{icon:?}");
            let pixels: Vec<&[u8]> = image.rgba.chunks(4).collect();
            // White wherever anything shows, so tinting gives the theme's colour exactly.
            assert!(
                pixels
                    .iter()
                    .all(|p| p[3] == 0 || p[..3] == [255, 255, 255]),
                "{icon:?}"
            );
            // Something is drawn, and the corners are empty.
            let drawn = pixels.iter().filter(|p| p[3] > 0).count();
            assert!(drawn > 300, "{icon:?}: {drawn}");
            for corner in [0, 95, 96 * 95, 96 * 96 - 1] {
                assert_eq!(pixels[corner][3], 0, "{icon:?}");
            }
        }
    }
}
