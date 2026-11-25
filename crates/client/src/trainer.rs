//! A trainer as the app holds it: the cheats, their values, the pins, the folded sections and the hotkeys in
//! effect, and how a hotkey steps a value.

mod shelf;

use std::collections::HashSet;

use model::{Args, Cheat, Chord, Kind, Plan, Ref, Trainer};
pub use shelf::Shelf;

use crate::{
    library::Stamp,
    profile::{TrainerProfile, Values},
};

/// The pinned section's id; the others are their category.
pub const PINNED: &str = "app:pinned";
/// The notes card's fold id.
pub const NOTES: &str = "app:notes";

pub struct Sheet {
    /// Its profile's key.
    pub trainer: Ref,
    pub cheats: Vec<Cheat>,
    /// By cheat, then slot: the source's bindings, with the user's choices once their owner lays them over.
    pub hotkeys: Vec<Vec<Option<Chord>>>,
    /// What it was asked for; another stamp on the game means it is stale.
    pub stamp: Stamp,
    /// The disk copy was stale and its refresh failed: the next open asks again.
    pub stale: bool,
    /// One per cheat: 0 or 1 for a toggle, the option index for a selection, else the number.
    pub values: Vec<f64>,
    pub pinned: HashSet<usize>,
    /// Section ids.
    pub folded: HashSet<String>,
    /// Whether a run tells where the player is, and can move them.
    pub location: bool,
    pub teleport: bool,
    /// Markdown to read before playing.
    pub notes: Option<String>,
    pub plan: Plan,
}

/// What a hotkey does to a cheat.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step {
    /// The new value.
    Set(f64),
    /// A button's press: sent, kept nowhere.
    Press(f64),
}

impl Sheet {
    pub fn new(trainer: Trainer, id: Ref, stamp: Stamp, profile: &TrainerProfile) -> Self {
        let Trainer { cheats, pinned, location, teleport, notes, plan } = trainer;
        let slots = |cheat: &Cheat| (0..cheat.kind.slots()).map(|slot| cheat.hotkeys.get(slot).copied().flatten()).collect();
        let mut sheet = Self {
            trainer: id,
            values: cheats.iter().map(initial).collect(),
            pinned: (0..cheats.len()).filter(|&index| pinned.contains(&cheats[index].id)).collect(),
            hotkeys: cheats.iter().map(slots).collect(),
            cheats,
            stamp,
            stale: false,
            folded: HashSet::new(),
            location,
            teleport,
            notes,
            plan,
        };
        sheet.restore(profile);
        sheet
    }

    /// The user's pins, once there are any, replace the source's; the folds are theirs.
    pub fn restore(&mut self, profile: &TrainerProfile) {
        if let Some(own) = &profile.pinned {
            self.pinned = (0..self.cheats.len()).filter(|&index| own.contains(&self.cheats[index].id)).collect();
        }
        let labels = profile.folded_labels.iter().filter_map(|label| self.section(label));
        let folded = profile.folded.iter().cloned().chain(labels).collect();
        self.folded = folded;
    }

    /// The section v1 printed as `label`.
    fn section(&self, label: &str) -> Option<String> {
        if label == "PINNED" {
            return Some(PINNED.to_owned());
        }
        self.cheats.iter().find(|cheat| cheat.category.to_uppercase() == label).map(|cheat| cheat.category.clone())
    }

    /// The cheats by section id as the list shows them: `PINNED` first, then each category in the trainer's order.
    pub fn sections(&self) -> Vec<(&str, Vec<usize>)> {
        let mut sections = vec![(PINNED, Vec::new())];
        for (index, cheat) in self.cheats.iter().enumerate() {
            let at = match self.pinned.contains(&index) {
                true => 0,
                false => sections.iter().position(|(id, _)| *id == cheat.category).unwrap_or_else(|| {
                    sections.push((&cheat.category, Vec::new()));
                    sections.len() - 1
                }),
            };
            sections[at].1.push(index);
        }
        sections
    }

    /// Pins or unpins a cheat, and keeps the pins in `profile`.
    pub fn toggle_pin(&mut self, index: usize, profile: &mut TrainerProfile) {
        if !self.pinned.remove(&index) {
            self.pinned.insert(index);
        }
        profile.pinned = Some(self.pinned.iter().map(|&index| self.cheats[index].id.clone()).collect());
    }

    /// Folds or unfolds a section, and keeps the folds in `profile`; the labels it has are ids now.
    pub fn toggle_fold(&mut self, section: &str, profile: &mut TrainerProfile) {
        if !self.folded.remove(section) {
            self.folded.insert(section.to_owned());
        }
        profile.folded = self.folded.iter().cloned().collect();
        profile.folded_labels.retain(|label| self.section(label).is_none());
    }

    pub fn reset(&mut self) {
        self.values = self.cheats.iter().map(initial).collect();
    }

    /// Whether the cheat was moved away from its resting value.
    pub fn is_live(&self, index: usize) -> bool {
        self.values[index] != initial(&self.cheats[index])
    }

    pub fn active(&self) -> usize {
        (0..self.cheats.len()).filter(|&index| self.is_live(index)).count()
    }

    pub fn live_values(&self) -> Values {
        let live = (0..self.cheats.len()).filter(|&index| self.is_live(index) && self.values[index].is_finite());
        live.map(|index| (self.cheats[index].id.clone(), self.values[index])).collect()
    }

    /// The rest values of the cheats in `values`.
    pub fn rest(&self, values: &Values) -> Values {
        let named = self.cheats.iter().filter(|cheat| values.contains_key(&cheat.id));
        named.map(|cheat| (cheat.id.clone(), initial(cheat))).collect()
    }

    /// What setting `values` changes, by cheat index.
    pub fn changes(&self, values: &Values) -> Vec<(usize, f64)> {
        changes(&self.cheats, &self.values, values)
    }

    /// Every cheat of `values` is at its value.
    pub fn holds(&self, values: &Values) -> bool {
        !values.is_empty() && self.changes(values).is_empty()
    }

    pub fn step(&self, index: usize, slot: usize) -> Option<Step> {
        step(&self.cheats[index], self.values[index], slot)
    }
}

/// Numbers stop at their bounds; selections wrap. Returns `None` when nothing changes.
fn step(cheat: &Cheat, value: f64, slot: usize) -> Option<Step> {
    let up = slot == 1;
    let next = match cheat.kind {
        Kind::Toggle => 1. - value.min(1.),
        Kind::Button => return Some(Step::Press(cheat.args.pressed)),
        Kind::Number | Kind::Slider => {
            let step = cheat.args.stride();
            cheat.args.clamp(round_to(if up { value + step } else { value - step }, cheat.args.min.unwrap_or(0.), step))
        }
        Kind::Selection => {
            let count = cheat.args.options.len() as f64;
            if count == 0. {
                return None;
            }
            (value + if up { 1. } else { -1. }).rem_euclid(count)
        }
        Kind::Unknown => return None,
    };
    (next != value).then_some(Step::Set(next))
}

/// `wanted` against `current`: each within its cheat's bounds. A button keeps no value.
fn changes(cheats: &[Cheat], current: &[f64], wanted: &Values) -> Vec<(usize, f64)> {
    let change = |(index, cheat): (usize, &Cheat)| {
        let value = cheat.args.clamp(*wanted.get(&cheat.id)?);
        let fits = match cheat.kind {
            Kind::Button | Kind::Unknown => false,
            Kind::Selection => value >= 0. && (value as usize) < cheat.args.options.len(),
            Kind::Toggle | Kind::Number | Kind::Slider => true,
        };
        (fits && value != current[index]).then_some((index, value))
    };
    cheats.iter().enumerate().filter_map(change).collect()
}

pub fn initial(cheat: &Cheat) -> f64 {
    cheat.args.min.unwrap_or(0.)
}

/// A slider's ticks are 0 to this.
pub const TOP_TICK: u8 = 9;

/// A slider's bounds; trainers may leave any of them out.
#[derive(Clone, Copy)]
pub struct Span {
    pub(crate) min: f64,
    pub(crate) max: f64,
    pub(crate) step: f64,
}

impl Span {
    pub fn of(args: &Args) -> Self {
        let min = args.min.unwrap_or(0.);
        Self { min, max: args.max.filter(|max| *max > min).unwrap_or(min + 100.), step: args.stride() }
    }

    /// Which of the ten ticks `value` lights up to.
    pub fn tick(self, value: f64) -> u8 {
        let share = (value - self.min) / (self.max - self.min);
        (share * f64::from(TOP_TICK)).round() as u8
    }

    pub fn value(self, tick: u8) -> f64 {
        self.snap(self.min + (self.max - self.min) * f64::from(tick) / f64::from(TOP_TICK))
    }

    /// A wheel notch: at least one step, so a coarse step never rounds back to where it was.
    pub fn nudge(self, value: f64, up: bool) -> f64 {
        let by = self.step.max((self.max - self.min) / f64::from(TOP_TICK));
        self.snap(if up { value + by } else { value - by })
    }

    /// Steps count from `min`, not from zero; a typed 2 on a 0.1 step stays 2.
    pub(crate) fn snap(self, value: f64) -> f64 {
        round_to(self.min + ((value - self.min) / self.step).round() * self.step, self.min, self.step).clamp(self.min, self.max)
    }
}

/// Keeps the precision of both the step and `min`: adding 1 to 0.5 should give 1.5, not 2.
fn round_to(value: f64, min: f64, step: f64) -> f64 {
    let decimals = |x: f64| x.to_string().split_once('.').map_or(0, |(_, fraction)| fraction.len());
    let scale = 10_f64.powi(decimals(step).max(decimals(min)) as i32);
    let rounded = (value * scale).round() / scale;
    // Far past f64's range the scale overflows: the step is then too coarse to drift.
    if rounded.is_finite() { rounded } else { value }
}

#[cfg(test)]
pub(crate) mod tests;
