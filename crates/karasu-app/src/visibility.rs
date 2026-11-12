//! Which of the plate's objects the scene hides (owner, 2026-10-26): hiding one, isolating one
//! (only it shown), and showing them all again.
//!
//! Visibility is a state of the view, not of the plate: it is not saved with the project, is
//! not part of the undo history, and a hidden object is still sliced and exported (the Slice
//! panel says so). A hidden object is not drawn and cannot be picked in the viewport.
//!
//! The plate's objects have no identity of their own, only their place in the list, which
//! deletions, undo and redo change. Each flag is therefore kept with its object's model:
//! [`Visibility::sync`] carries the flags over to a changed list by model, in order, and an
//! object it cannot match is shown. Deleting an object says which one went
//! ([`Visibility::removed`]), so copies of one model keep their own flags.
use karasu_project::project::ModelId;

/// The objects' visibility, in the plate's order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Visibility {
    /// Per object: its model, and whether it is hidden.
    objects: Vec<(ModelId, bool)>,
    /// While one object is isolated: that object, and the flags to restore when it ends.
    isolation: Option<(usize, Vec<bool>)>,
}

impl Visibility {
    /// Follow the plate's objects, given by their models in order. A changed list keeps each
    /// object's flag when an earlier entry of its model is left to take it from; any other
    /// object is shown, and an isolation ends (its flags no longer fit the list).
    pub fn sync(&mut self, models: &[ModelId]) {
        if self.objects.iter().map(|(m, _)| m).eq(models) {
            return;
        }
        let mut old = std::mem::take(&mut self.objects);
        self.objects = models
            .iter()
            .map(|model| {
                let hidden = old
                    .iter()
                    .position(|(m, _)| m == model)
                    .is_some_and(|k| old.remove(k).1);
                (*model, hidden)
            })
            .collect();
        self.isolation = None;
    }

    /// Object `index` is about to be deleted: its flag goes with it, and the others keep
    /// theirs. Deleting the isolated object ends the isolation, as if it had been ended first.
    pub fn removed(&mut self, index: usize) {
        if index >= self.objects.len() {
            return;
        }
        if let Some((isolated, mut saved)) = self.isolation.take() {
            if isolated == index {
                self.restore(&saved);
            } else {
                saved.remove(index);
                let isolated = if isolated > index {
                    isolated - 1
                } else {
                    isolated
                };
                self.isolation = Some((isolated, saved));
            }
        }
        self.objects.remove(index);
    }

    /// Whether object `index` is hidden, if it is still the object of `model` (an object the
    /// last sync did not see is shown).
    #[must_use]
    pub fn is_hidden(&self, index: usize, model: ModelId) -> bool {
        self.objects
            .get(index)
            .is_some_and(|&(m, hidden)| m == model && hidden)
    }

    /// The isolated object, while one is.
    #[must_use]
    pub fn isolated(&self) -> Option<usize> {
        self.isolation.as_ref().map(|(index, _)| *index)
    }

    /// Hide object `index`, or show it again. Changing one object by hand ends an isolation,
    /// keeping what is shown now.
    pub fn toggle(&mut self, index: usize) {
        if let Some((_, hidden)) = self.objects.get_mut(index) {
            *hidden = !*hidden;
            self.isolation = None;
        }
    }

    /// Hide objects `indices`, or show them (owner, 2026-11-10: the models checked in the
    /// sidebar). As a change by hand, it ends an isolation, keeping what is shown now.
    pub fn set_hidden(&mut self, indices: &[usize], hidden: bool) {
        for &index in indices {
            if let Some((_, flag)) = self.objects.get_mut(index) {
                *flag = hidden;
                self.isolation = None;
            }
        }
    }

    /// Show only object `index`; on the isolated object, end the isolation and show again what
    /// was shown before it. Isolating another object while one is isolated keeps the flags of
    /// before the first.
    pub fn isolate(&mut self, index: usize) {
        if index >= self.objects.len() {
            return;
        }
        match self.isolation.take() {
            Some((isolated, saved)) if isolated == index => self.restore(&saved),
            previous => {
                let saved = previous.map_or_else(
                    || self.objects.iter().map(|(_, hidden)| *hidden).collect(),
                    |(_, saved)| saved,
                );
                for (k, (_, hidden)) in self.objects.iter_mut().enumerate() {
                    *hidden = k != index;
                }
                self.isolation = Some((index, saved));
            }
        }
    }

    /// Show every object, ending an isolation.
    pub fn show_all(&mut self) {
        for (_, hidden) in &mut self.objects {
            *hidden = false;
        }
        self.isolation = None;
    }

    fn restore(&mut self, saved: &[bool]) {
        for ((_, hidden), was) in self.objects.iter_mut().zip(saved) {
            *hidden = *was;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(n: u8) -> ModelId {
        ModelId([n; 32])
    }

    fn hidden(visibility: &Visibility, models: &[ModelId]) -> Vec<bool> {
        models
            .iter()
            .enumerate()
            .map(|(k, m)| visibility.is_hidden(k, *m))
            .collect()
    }

    #[test]
    fn checked_objects_are_hidden_and_shown_together_ending_an_isolation() {
        let mut visibility = Visibility::default();
        let models: Vec<ModelId> = (0..3).map(model).collect();
        visibility.sync(&models);
        visibility.isolate(1);
        visibility.set_hidden(&[0, 2], true);
        assert_eq!(visibility.isolated(), None);
        let hidden: Vec<bool> = (0..3).map(|k| visibility.is_hidden(k, models[k])).collect();
        assert_eq!(hidden, [true, false, true]);
        visibility.set_hidden(&[0, 2, 9], false);
        assert!((0..3).all(|k| !visibility.is_hidden(k, models[k])));
    }

    #[test]
    fn hiding_isolating_and_showing_all_are_reversible() {
        let models = [model(1), model(2), model(3)];
        let mut visibility = Visibility::default();
        visibility.sync(&models);
        assert_eq!(hidden(&visibility, &models), [false; 3]);
        visibility.toggle(2);
        assert_eq!(hidden(&visibility, &models), [false, false, true]);
        // Isolating shows one object alone; isolating it again brings back what was shown,
        // the object hidden by hand included.
        visibility.isolate(0);
        assert_eq!(visibility.isolated(), Some(0));
        assert_eq!(hidden(&visibility, &models), [false, true, true]);
        visibility.isolate(1);
        assert_eq!(hidden(&visibility, &models), [true, false, true]);
        visibility.isolate(1);
        assert_eq!(visibility.isolated(), None);
        assert_eq!(hidden(&visibility, &models), [false, false, true]);
        visibility.show_all();
        assert_eq!(hidden(&visibility, &models), [false; 3]);
    }

    #[test]
    fn flags_follow_their_objects_through_deletions_and_list_changes() {
        // Two copies of one model: deleting the first keeps the second's flag.
        let models = [model(1), model(1), model(2)];
        let mut visibility = Visibility::default();
        visibility.sync(&models);
        visibility.toggle(1);
        visibility.removed(0);
        let left = [model(1), model(2)];
        visibility.sync(&left);
        assert_eq!(hidden(&visibility, &left), [true, false]);
        // A list changed elsewhere (undo, a new model) keeps the flags by model, and shows what
        // it cannot match.
        let undone = [model(3), model(1), model(2)];
        visibility.sync(&undone);
        assert_eq!(hidden(&visibility, &undone), [false, true, false]);
        // An object that is no longer the one flagged reads as shown.
        assert!(!visibility.is_hidden(1, model(2)));
    }

    #[test]
    fn deleting_during_an_isolation_keeps_or_ends_it_sensibly() {
        let models = [model(1), model(2), model(3)];
        let mut visibility = Visibility::default();
        visibility.sync(&models);
        visibility.toggle(0);
        visibility.isolate(2);
        // Deleting another object: still isolated, and ending it restores the rest.
        visibility.removed(1);
        let left = [model(1), model(3)];
        visibility.sync(&left);
        assert_eq!(visibility.isolated(), Some(1));
        visibility.isolate(1);
        assert_eq!(hidden(&visibility, &left), [true, false]);
        // Deleting the isolated object ends the isolation as if ended first.
        visibility.isolate(0);
        visibility.removed(0);
        let last = [model(3)];
        visibility.sync(&last);
        assert_eq!(visibility.isolated(), None);
        assert_eq!(hidden(&visibility, &last), [false]);
    }
}
