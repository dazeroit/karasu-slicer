//! The Braces tool's values (owner mockup "Braces tool", 2026-10-29): the diameter of every new
//! brace, and where automatic braces go between neighbouring supports
//! ([`karasu_core::supports::plan_braces`]). The Automatic tool adds its braces with them when
//! asked (TD-112), and the command line's automatic braces take them too (TD-157).
use karasu_core::supports::{Brace, BracePattern, BracePlan};
use serde::{Deserialize, Serialize};

/// A diameter the document accepts for a brace, in mm.
pub const DIAMETER_MM: std::ops::RangeInclusive<f64> = 0.1..=10.0;
/// The distance in plan within which automatic braces join supports, in mm.
pub const DISTANCE_MM: std::ops::RangeInclusive<f64> = 1.0..=100.0;
/// The lowest automatic braces' height above the plate or the raft, in mm.
pub const LOWEST_MM: std::ops::RangeInclusive<f64> = 0.0..=200.0;
/// The height between automatic braces, in mm.
pub const STEP_MM: std::ops::RangeInclusive<f64> = 0.5..=100.0;

/// How automatic braces join a pair of supports, as the settings keep it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pattern {
    /// A level brace at each height.
    Horizontal,
    /// One diagonal per step, alternating sides.
    #[default]
    Zigzag,
    /// Both diagonals per step.
    Cross,
}

impl Pattern {
    /// The patterns, in the panel's order.
    pub const ALL: [Self; 3] = [Self::Horizontal, Self::Zigzag, Self::Cross];

    /// The core's pattern.
    #[must_use]
    pub fn core(self) -> BracePattern {
        match self {
            Self::Horizontal => BracePattern::Horizontal,
            Self::Zigzag => BracePattern::Zigzag,
            Self::Cross => BracePattern::Cross,
        }
    }
}

/// What new braces are made with: the diameter of every new brace, and where automatic ones go
/// (owner's mockup values). Tool values: they start from the settings each time the tool opens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NewBraces {
    /// Each new brace is as thick as the thinner of its two shafts where it meets them, up to
    /// `diameter_mm` (owner, 2026-11-06, TD-136), and follows them (owner, 2026-10-29);
    /// otherwise it takes `diameter_mm`.
    pub auto_diameter: bool,
    /// A new brace's diameter, or an automatic one's largest, in mm.
    pub diameter_mm: f64,
    /// Automatic braces join supports this close in plan, in mm.
    pub max_distance_mm: f64,
    /// The lowest automatic braces' height above the plate or the raft, in mm.
    pub lowest_mm: f64,
    /// The height between automatic braces, in mm.
    pub step_mm: f64,
    /// How automatic braces join a pair of supports.
    pub pattern: Pattern,
}

/// The values new braces start from when the settings hold none (owner's mockup).
pub const DEFAULT_BRACES: NewBraces = NewBraces {
    auto_diameter: true,
    diameter_mm: 1.0,
    max_distance_mm: 25.0,
    lowest_mm: 6.0,
    step_mm: 6.0,
    pattern: Pattern::Zigzag,
};

impl Default for NewBraces {
    fn default() -> Self {
        DEFAULT_BRACES
    }
}

impl NewBraces {
    /// These values if every one is in its range, else the built-in ones.
    #[must_use]
    pub fn valid(self) -> Self {
        if DIAMETER_MM.contains(&self.diameter_mm)
            && DISTANCE_MM.contains(&self.max_distance_mm)
            && LOWEST_MM.contains(&self.lowest_mm)
            && STEP_MM.contains(&self.step_mm)
        {
            self
        } else {
            DEFAULT_BRACES
        }
    }

    /// The automatic braces these values plan.
    #[must_use]
    pub fn plan(&self) -> BracePlan {
        BracePlan {
            max_distance_mm: self.max_distance_mm,
            lowest_mm: self.lowest_mm,
            step_mm: self.step_mm,
            pattern: self.pattern.core(),
            diameter_mm: self.diameter(),
            max_diameter_mm: self.largest(),
        }
    }

    /// A new brace's diameter: `None` when automatic.
    #[must_use]
    pub fn diameter(&self) -> Option<f64> {
        (!self.auto_diameter).then_some(self.diameter_mm)
    }

    /// A new automatic brace's largest diameter: `None` when it has a diameter of its own.
    #[must_use]
    pub fn largest(&self) -> Option<f64> {
        self.auto_diameter.then_some(self.diameter_mm)
    }

    /// A new brace `id` between `from` and `to`, of these values' diameter.
    #[must_use]
    pub fn brace(&self, id: u64, from: End, to: End) -> Brace {
        Brace {
            id,
            from,
            to,
            diameter_mm: self.diameter(),
            max_diameter_mm: self.largest(),
        }
    }
}

/// A brace's end on a shaft: the support and the world height.
pub type End = (u64, f64);

#[cfg(test)]
mod tests {
    use super::*;
    use karasu_core::supports::{BaseSpec, Preset, SupportDocument};

    #[test]
    fn new_brace_values_out_of_range_are_the_built_in_ones() {
        assert_eq!(DEFAULT_BRACES.valid(), DEFAULT_BRACES);
        let wrong = NewBraces {
            step_mm: 0.0,
            ..DEFAULT_BRACES
        };
        assert_eq!(wrong.valid(), DEFAULT_BRACES);
        // The built-in values make a brace the document accepts.
        let mut document = SupportDocument::new(Preset::Medium);
        for id in [1, 2] {
            document
                .supports
                .push(karasu_core::supports::ManualSupport {
                    id,
                    tip_mm: [f64::from(u8::try_from(id).unwrap()) * 5.0, 0.0, 10.0],
                    tip_axis: karasu_core::supports::AxisSpec::Normal,
                    bends: Vec::new(),
                    base: BaseSpec::Vertical,
                    parameters: Preset::Medium.parameters(),
                    accept_collision: false,
                    origin: karasu_core::supports::SupportOrigin::Placed,
                    kind: karasu_core::supports::SupportKind::Standard,
                });
        }
        document
            .braces
            .push(DEFAULT_BRACES.brace(4, (1, 6.0), (2, 6.0)));
        document.braces.push(Brace {
            id: 3,
            from: (1, 4.0),
            to: (2, 4.0),
            diameter_mm: Some(DEFAULT_BRACES.diameter_mm),
            max_diameter_mm: None,
        });
        assert!(document.validated().is_ok());
    }
}
