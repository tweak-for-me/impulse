use std::{fs, sync::Arc, time::Duration};

use futures::future::BoxFuture;
use model::{Args, Attach, Cheat, Failure, FailureKind, Kind, Launch, Pick, Plan, Ref, Runner, Target, Trainer};

use super::{PINNED, Sheet, Shelf, Span, Step, TOP_TICK, Values, changes, round_to, step};
use crate::{
    Load,
    library::{GameId, Stamp},
    profile::{Profiles, TrainerProfile},
    storage::Paths,
};

pub(crate) fn cheat(id: &str, kind: Kind, args: Args) -> Cheat {
    Cheat {
        id: id.into(),
        key: None,
        name: String::new(),
        category: String::new(),
        category_name: None,
        kind,
        args,
        hotkeys: Vec::new(),
        instructions: None,
        description: None,
    }
}

#[test]
fn kept_values_change_only_what_they_can() {
    let cheats = [
        cheat("on", Kind::Toggle, Args::default()),
        cheat("same", Kind::Toggle, Args::default()),
        cheat("far", Kind::Slider, Args { min: Some(1.), max: Some(5.), ..Args::default() }),
        cheat("pick", Kind::Selection, Args { options: vec!["a".into(), "b".into()], ..Args::default() }),
        cheat("press", Kind::Button, Args::default()),
    ];
    let wanted: Values =
        [("on", 1.), ("same", 1.), ("far", 9.), ("pick", 2.), ("press", 1.), ("gone", 1.)].map(|(id, value)| (id.to_owned(), value)).into();
    assert_eq!(changes(&cheats, &[0., 1., 1., 0., 0.], &wanted), [(0, 1.), (2, 5.)]);
}

#[test]
fn a_pair_steps_by_its_step_and_stops_at_the_ends() {
    let speed = cheat("speed", Kind::Slider, Args { min: Some(0.5), max: Some(2.5), step: Some(1.), ..Args::default() });
    assert_eq!(step(&speed, 0.5, 1), Some(Step::Set(1.5)));
    assert_eq!(step(&speed, 1.5, 0), Some(Step::Set(0.5)));
    assert_eq!(step(&speed, 2.5, 1), None);
    assert_eq!(step(&speed, 0.5, 0), None);
    let unstepped = cheat("gold", Kind::Number, Args { step: Some(0.), ..Args::default() });
    assert_eq!(step(&unstepped, 3., 1), Some(Step::Set(4.)));
}

#[test]
fn a_selection_wraps_and_an_empty_one_does_nothing() {
    let pick = cheat("pick", Kind::Selection, Args { options: vec!["a".into(), "b".into(), "c".into()], ..Args::default() });
    assert_eq!(step(&pick, 2., 1), Some(Step::Set(0.)));
    assert_eq!(step(&pick, 0., 0), Some(Step::Set(2.)));
    assert_eq!(step(&cheat("none", Kind::Selection, Args::default()), 0., 1), None);
}

#[test]
fn a_toggle_flips_a_button_presses_and_an_unknown_does_nothing() {
    assert_eq!(step(&cheat("god", Kind::Toggle, Args::default()), 0., 0), Some(Step::Set(1.)));
    assert_eq!(step(&cheat("god", Kind::Toggle, Args::default()), 1., 0), Some(Step::Set(0.)));
    assert_eq!(step(&cheat("heal", Kind::Button, Args { pressed: 7., ..Args::default() }), 0., 0), Some(Step::Press(7.)));
    assert_eq!(step(&cheat("odd", Kind::Unknown, Args::default()), 0., 0), None);
}

#[test]
fn hotkey_steps_keep_the_origin_and_do_not_drift() {
    assert_eq!(round_to(0.5 + 1., 0.5, 1.), 1.5);
    assert_eq!(round_to(0.2 + 0.1, 0., 0.1), 0.3);
}

#[test]
fn a_slider_value_lands_on_a_step_without_drift() {
    let span = Span { min: 0.5, max: 10., step: 0.1 };
    assert_eq!([2., 1.23, 0.1, 12.].map(|typed| span.snap(typed)), [2., 1.2, 0.5, 10.]);
    assert!((0..=TOP_TICK).all(|tick| span.value(tick) == (span.value(tick) * 10.).round() / 10.));
}

#[test]
fn steps_count_from_min() {
    let span = Span { min: 3., max: 21., step: 2. };
    assert_eq!(span.value(0), 3.);
    assert_eq!(span.value(TOP_TICK), 21.);
    assert!((0..=TOP_TICK).all(|tick| (span.value(tick) - 3.) % 2. == 0.));
}

#[test]
fn wheel_moves_a_coarse_step() {
    let span = Span { min: 0., max: 10., step: 5. };
    assert_eq!(span.nudge(0., true), 5.);
    assert_eq!(span.nudge(5., true), 10.);
    assert_eq!(span.nudge(10., true), 10.);
    assert_eq!(span.nudge(5., false), 0.);
}

struct Idle;

impl Runner for Idle {
    fn prepare(&self) -> BoxFuture<'_, Result<Box<dyn Attach>, Failure>> {
        Box::pin(async { Err(Failure::new(FailureKind::Internal)) })
    }
}

fn trainer() -> Ref {
    Ref::parse("s:t").unwrap()
}

fn stamp(build: u32) -> Stamp {
    Stamp { build: Some(build), ..Stamp::default() }
}

fn game(id: &str) -> GameId {
    GameId::of(&Ref::parse(&format!("s:{id}")).unwrap())
}

fn sheet_of(build: u32, profile: &TrainerProfile) -> Sheet {
    let target = Target { image: None, minimum_age: Duration::ZERO, pick: Pick::Biggest, arguments: None };
    let plan = Plan { target, launch: Launch::User, launch_arguments: None, launch_within: None, runner: Arc::new(Idle) };
    let cheats = vec![Cheat { category: "player".into(), ..cheat("god", Kind::Toggle, Args::default()) }];
    let trainer = Trainer { cheats, pinned: Vec::new(), location: false, teleport: false, notes: None, plan };
    Sheet::new(trainer, self::trainer(), stamp(build), profile)
}

fn sheet(build: u32) -> Sheet {
    sheet_of(build, &TrainerProfile::default())
}

/// Profiles in a fresh folder, the trainer's pins set when `pinned`.
fn profiles(name: &str, pinned: bool) -> Profiles {
    let dir = std::env::temp_dir().join(format!("impulse-shelf-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let mut profiles = Profiles::load(&Paths::new(dir));
    if pinned {
        profiles.edit_trainer(&trainer(), |profile| profile.pinned = Some(["god".to_owned()].into()));
    }
    profiles
}

#[test]
fn a_held_game_shows_its_newer_copy_once_every_hold_is_released() {
    let mut shelf = Shelf::default();
    let (g, h) = (game("g"), game("h"));
    assert!(shelf.put(g.clone(), sheet(1)));
    shelf.hold(g.clone());
    shelf.hold(g.clone());
    assert!(!shelf.put(g.clone(), sheet(2)));
    assert_eq!(shelf.sheet(&g).unwrap().stamp, stamp(1));
    let pinned = profiles("held", true);
    shelf.hold(h.clone());
    assert!(!shelf.put(h.clone(), sheet(3)));
    assert!(!shelf.release(&g, &pinned));
    assert!(shelf.release(&h, &pinned));
    assert_eq!(shelf.sheet(&g).unwrap().stamp, stamp(1));
    assert!(shelf.release(&g, &pinned));
    let shown = shelf.sheet(&g).unwrap();
    assert!(shown.stamp == stamp(2) && shown.pinned.contains(&0));
    assert!(!shelf.release(&g, &pinned));
}

#[test]
fn a_failed_refresh_keeps_a_copy_stale_and_fails_without_one() {
    let mut shelf = Shelf::default();
    let (g, h) = (game("g"), game("h"));
    shelf.put(g.clone(), sheet(1));
    shelf.hold(g.clone());
    shelf.put(g.clone(), sheet(2));
    shelf.failed(g.clone(), Failure::new(FailureKind::Offline));
    assert!(!shelf.is_current(&g, &stamp(1)));
    shelf.release(&g, &profiles("stale", false));
    assert!(shelf.sheet(&g).is_some_and(|sheet| sheet.stale));
    shelf.pending(h.clone());
    shelf.failed(h.clone(), Failure::new(FailureKind::Offline));
    assert!(matches!(shelf.get(&h), Some(Load::Failed(_))));
}

#[test]
fn another_language_outdates_every_copy_shown_or_waiting() {
    let mut shelf = Shelf::default();
    let (g, h) = (game("g"), game("h"));
    shelf.put(g.clone(), sheet(1));
    shelf.put(h.clone(), sheet(1));
    shelf.hold(h.clone());
    shelf.put(h.clone(), sheet(2));
    shelf.outdate(Some(&g));
    // The waiting copy is the newest: it decides.
    assert!(!shelf.is_current(&g, &stamp(1)) && shelf.is_current(&h, &stamp(2)));
    shelf.outdate(None);
    assert!(!shelf.is_current(&h, &stamp(2)));
    shelf.release(&h, &profiles("outdated", false));
    assert!(shelf.sheet(&h).is_some_and(|sheet| sheet.stale && sheet.stamp == stamp(2)));
}

#[test]
fn another_trainer_drops_the_copies_but_a_held_one() {
    let mut shelf = Shelf::default();
    let (g, h) = (game("g"), game("h"));
    shelf.put(g.clone(), sheet(1));
    shelf.put(h.clone(), sheet(1));
    shelf.hold(h.clone());
    shelf.put(h.clone(), sheet(2));
    assert!(shelf.forget(&g) && !shelf.forget(&h));
    assert!(shelf.get(&g).is_none());
    assert!(shelf.is_current(&h, &stamp(1)) && !shelf.release(&h, &profiles("forgotten", false)));
}

#[test]
fn a_v1_fold_label_becomes_an_id_and_an_unknown_one_stays() {
    let mut profile = TrainerProfile { folded_labels: ["PLAYER", "PINNED", "GONE"].map(str::to_owned).into(), ..TrainerProfile::default() };
    let mut sheet = sheet_of(1, &profile);
    assert_eq!(sheet.folded, ["player", PINNED].map(str::to_owned).into());
    sheet.toggle_fold("player", &mut profile);
    assert_eq!(profile.folded, [PINNED.to_owned()].into());
    assert_eq!(profile.folded_labels, ["GONE".to_owned()].into());
}

#[test]
fn sections_put_the_pins_first_then_categories_in_order() {
    let mut sheet = sheet(1);
    let of = |id, category: &str| Cheat { category: category.into(), ..cheat(id, Kind::Toggle, Args::default()) };
    sheet.cheats = vec![of("a", "world"), of("b", "player"), of("c", "world")];
    sheet.pinned = [1].into();
    assert_eq!(sheet.sections(), [(PINNED, vec![1]), ("world", vec![0, 2])]);
}
