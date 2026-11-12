//! Which of a tool's items the viewport shows (owner, 2026-10-31): with many supports, braces or
//! drain holes, a wrong one is hard to see and to reach among the others.
//!
//! - The tool strip's **Show** hides the others: every item, only those with errors, or only the
//!   selected one; in the Manual tool also only the mini supports (owner, 2026-11-04).
//! - The list window's filter (**With errors**, in the Manual tool also **Mini**, owner R8
//!   2026-11-10) turns the others into ghosts: faint, drawn through, so where the listed ones
//!   stand still reads.
//!
//! A hidden item and a ghost cannot be picked. The selected item always shows solid: an item
//! fixed while it is selected stays in view until another is chosen. The model itself is never changed: it must be
//! seen well while the items are edited. Only which draws are made changes, so nothing is
//! uploaded again.
use crate::i18n::tr;
use serde::{Deserialize, Serialize};

/// What the tool strip's Show keeps of a tool's items, kept with the settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shown {
    /// Every item.
    #[default]
    All,
    /// Only the items with errors, and the selected one.
    Errors,
    /// Only the selected item.
    Selected,
    /// Only the mini supports, and the selected item (the Manual tool's).
    Minis,
}

impl Shown {
    /// The choices, in the button's options order.
    pub const CHOICES: [Self; 3] = [Self::All, Self::Errors, Self::Selected];
    /// The Manual tool's choices: the others, and only the mini supports.
    pub const SUPPORT_CHOICES: [Self; 4] = [Self::All, Self::Errors, Self::Minis, Self::Selected];

    /// The option's label.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::All => tr!("show-all"),
            Self::Errors => tr!("show-errors"),
            Self::Selected => tr!("show-selected"),
            Self::Minis => tr!("show-minis"),
        }
    }

    /// The button's click: back to every item from a filter, or only the errors from every
    /// item.
    #[must_use]
    pub const fn toggled(self) -> Self {
        match self {
            Self::All => Self::Errors,
            Self::Errors | Self::Selected | Self::Minis => Self::All,
        }
    }

    /// The tool strip's button: lit while it leaves items out, with `choices` as its options.
    #[must_use]
    pub fn button(self, choices: &[Self]) -> crate::tool_strip::Item {
        let filtering = self != Self::All;
        crate::tool_strip::Item::button(
            crate::actions::Action::Show,
            if filtering {
                crate::icons::Icon::EyeOff
            } else {
                crate::icons::Icon::Eye
            },
            filtering,
            true,
        )
        .with_options(self.options(choices))
    }

    /// The button's options among `choices`, the chosen one marked.
    #[must_use]
    pub fn options(self, choices: &[Self]) -> Vec<(String, bool)> {
        choices
            .iter()
            .map(|&choice| (choice.label(), choice == self))
            .collect()
    }
}

/// How one item shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seen {
    /// As always.
    Solid,
    /// A faint ghost, which cannot be picked.
    Ghost,
    /// Not drawn, and cannot be picked.
    Hidden,
}

impl Seen {
    /// Whether a click can pick it.
    #[must_use]
    pub const fn pickable(self) -> bool {
        matches!(self, Self::Solid)
    }
}

/// How an item shows: `wrong` when it has an error, `selected` when it is the selected one, `mini`
/// when it is a mini support, `shown` the strip's choice and `listed`, while the list window
/// filters its items, whether this one is among those it lists (the others are ghosts).
#[must_use]
#[allow(clippy::fn_params_excessive_bools)] // Independent facts of one item, each a yes or no.
pub fn seen(shown: Shown, listed: Option<bool>, wrong: bool, selected: bool, mini: bool) -> Seen {
    let kept = match shown {
        Shown::All => true,
        Shown::Errors => wrong || selected,
        Shown::Selected => selected,
        Shown::Minis => mini || selected,
    };
    if !kept {
        Seen::Hidden
    } else if listed == Some(false) && !selected {
        Seen::Ghost
    } else {
        Seen::Solid
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_strip_hides_the_others_and_the_list_makes_them_ghosts() {
        // Every item, no list filter: all solid.
        for (wrong, selected) in [(false, false), (true, false), (false, true)] {
            assert_eq!(seen(Shown::All, None, wrong, selected, false), Seen::Solid);
        }
        // With errors in the list: the others are ghosts, the selected one stays solid.
        assert_eq!(
            seen(Shown::All, Some(false), false, false, false),
            Seen::Ghost
        );
        assert_eq!(
            seen(Shown::All, Some(true), true, false, false),
            Seen::Solid
        );
        assert_eq!(
            seen(Shown::All, Some(false), false, true, false),
            Seen::Solid
        );
        // Only the errors: the others hidden, whatever the list.
        assert_eq!(seen(Shown::Errors, None, false, false, false), Seen::Hidden);
        assert_eq!(
            seen(Shown::Errors, Some(false), false, false, false),
            Seen::Hidden
        );
        assert_eq!(seen(Shown::Errors, None, true, false, false), Seen::Solid);
        // ... but the selected one, fixed or not, until another is chosen.
        assert_eq!(
            seen(Shown::Errors, Some(false), false, true, false),
            Seen::Solid
        );
        // Only the selected one.
        assert_eq!(
            seen(Shown::Selected, None, true, false, false),
            Seen::Hidden
        );
        assert_eq!(
            seen(Shown::Selected, Some(false), false, true, false),
            Seen::Solid
        );
        assert!(!Seen::Ghost.pickable() && !Seen::Hidden.pickable() && Seen::Solid.pickable());
        // Only the mini supports: the others hidden but the selected one (owner, 2026-11-04).
        assert_eq!(seen(Shown::Minis, None, false, false, true), Seen::Solid);
        assert_eq!(seen(Shown::Minis, None, true, false, false), Seen::Hidden);
        assert_eq!(seen(Shown::Minis, None, false, true, false), Seen::Solid);
        assert_eq!(Shown::Minis.toggled(), Shown::All);
        // The list's Mini: the others are ghosts, the errors among them too (owner R8).
        assert_eq!(
            seen(Shown::All, Some(true), false, false, true),
            Seen::Solid
        );
        assert_eq!(
            seen(Shown::All, Some(false), true, false, false),
            Seen::Ghost
        );
        assert_eq!(
            seen(Shown::All, Some(false), true, true, false),
            Seen::Solid
        );
        // Offered by the Manual tool only.
        assert_eq!(Shown::All.options(&Shown::CHOICES).len(), 3);
        assert_eq!(
            Shown::Minis.options(&Shown::SUPPORT_CHOICES)[2],
            (tr!("show-minis"), true)
        );
        // The button: on to the errors, off back to all.
        assert_eq!(Shown::All.toggled(), Shown::Errors);
        assert_eq!(Shown::Selected.toggled(), Shown::All);
        assert_eq!(
            Shown::Errors
                .options(&Shown::CHOICES)
                .iter()
                .filter(|o| o.1)
                .count(),
            1
        );
    }
}
