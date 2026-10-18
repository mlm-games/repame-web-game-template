//! Simulation: fixed-step race simulation with data-driven progression.

pub mod ai;
pub mod car;
pub mod data;
pub mod meta;
pub mod obstacles;
pub mod platform;
pub mod race;
pub mod seed;
pub mod track;

use bevy_ecs::prelude::*;

use crate::sim::car::Input;
use crate::sim::meta::{Progress, UpgradeKind};
use crate::sim::race::{Cue, Phase, Session, TrackWorld};

#[derive(Resource, Default)]
pub struct Controls(pub Input);

#[derive(Clone, Debug)]
pub enum Intent {
    StartLevel(usize),
    SelectLevel(usize),
    SelectCar(usize),
    BuyCar(usize),
    Upgrade(UpgradeKind),
    ToGarage,
    ToMapSelect,
    Restart,
    ToggleMute,
    SetMuted(bool),
    NoAds(bool),
}

#[derive(Resource, Default)]
pub struct Intents(pub Vec<Intent>);

pub fn push_intent(world: &mut World, intent: Intent) {
    if let Some(mut slot) = world.get_resource_mut::<Intents>() {
        slot.0.push(intent);
    }
}

pub fn apply_intents(world: &mut World) {
    let intents = match world.get_resource_mut::<Intents>() {
        Some(mut slot) => std::mem::take(&mut slot.0),
        None => return,
    };
    for intent in intents {
        match intent {
            Intent::StartLevel(index) => {
                // `resource_scope` keeps the save in place: a missing save is
                // never silently replaced with defaults here.
                let started = world.resource_scope(|world, mut progress: Mut<Progress>| {
                    if !progress.try_open_level(index) {
                        return None;
                    }
                    let car = progress.selected_car();
                    let session = Session::new(index, car, &progress);
                    let level = data::level_at(index);
                    world.insert_resource(TrackWorld::load(&level));
                    Some(session)
                });
                if let Some(session) = started {
                    world.insert_resource(session);
                }
            }
            Intent::SelectLevel(index) => {
                world.resource_scope(|world, progress: Mut<Progress>| {
                    let car = progress.selected_car();
                    let session = Session::new(index, car, &progress);
                    let level = data::level_at(index);
                    world.insert_resource(TrackWorld::load(&level));
                    world.insert_resource(session);
                });
            }
            Intent::SelectCar(index) => {
                if let Some(mut progress) = world.get_resource_mut::<Progress>() {
                    progress.select_car(index);
                }
            }
            Intent::BuyCar(index) => {
                if let Some(mut progress) = world.get_resource_mut::<Progress>()
                    && progress.buy_car(index)
                {
                    progress.select_car(index);
                }
            }
            Intent::Upgrade(kind) => {
                if let Some(mut progress) = world.get_resource_mut::<Progress>() {
                    let car = data::car_at(progress.selected_car() as i32);
                    progress.buy_upgrade(car.id.as_str(), kind);
                }
            }
            Intent::ToGarage => set_phase(world, Phase::Garage),
            Intent::ToMapSelect => set_phase(world, Phase::MapSelect),
            Intent::Restart => restart(world),
            Intent::ToggleMute => {
                let muted = {
                    let progress = world.resource::<Progress>();
                    !progress.data.muted
                };
                if let Some(mut progress) = world.get_resource_mut::<Progress>() {
                    progress.set_muted(muted);
                }
            }
            Intent::SetMuted(muted) => {
                if let Some(mut progress) = world.get_resource_mut::<Progress>() {
                    progress.set_muted(muted);
                }
            }
            Intent::NoAds(value) => {
                if let Some(mut progress) = world.get_resource_mut::<Progress>() {
                    progress.set_no_ads(value);
                }
            }
        }
    }
}

fn set_phase(world: &mut World, phase: Phase) {
    if let Some(mut session) = world.get_resource_mut::<Session>() {
        session.phase = phase;
    }
    if let Some(mut q) = world.get_resource_mut::<race::CueQueue>() {
        q.0 = Some(Cue::Click);
    }
}

fn restart(world: &mut World) {
    let Some(session) = world.get_resource::<Session>() else {
        return;
    };
    let (level, car) = (session.level, session.car);
    world.resource_scope(|world, progress: Mut<Progress>| {
        let new_session = Session::new(level, car, &progress);
        let level_data = data::level_at(level);
        world.insert_resource(TrackWorld::load(&level_data));
        world.insert_resource(new_session);
    });
}

/// Record finished/failed runs into the save exactly once.
pub fn results_system(world: &mut World) {
    let Some((level, results, finished)) = world
        .get_resource::<Session>()
        .filter(|s| matches!(s.phase, Phase::Finished | Phase::Failed) && !s.recorded)
        .map(|s| (s.level, s.results, s.phase == Phase::Finished))
    else {
        return;
    };

    if let Some(mut session) = world.get_resource_mut::<Session>() {
        session.recorded = true;
    }
    if let Some(results) = results {
        if finished {
            let mut progress = world.resource_mut::<Progress>();
            let already = progress.data.completed(level);
            progress.record_results(level, &results);
            if !already
                && let Some(mut session) = world.get_resource_mut::<Session>()
                && let Some(mut r) = session.results
            {
                r.first_completion = true;
                session.results = Some(r);
            }
        } else {
            let progress = world.resource::<Progress>();
            let best = progress.data.best_score(level).unwrap_or(0);
            if let Some(mut session) = world.get_resource_mut::<Session>()
                && let Some(mut r) = session.results
            {
                r.score = r.score.max(best);
                session.results = Some(r);
            }
        }
    }
}

/// Drain the cue queue for the audio director.
pub fn take_cue(world: &mut World) -> Option<Cue> {
    world
        .get_resource_mut::<race::CueQueue>()
        .and_then(|mut q| q.0.take())
}

pub fn build_schedule() -> Schedule {
    let mut s = Schedule::default();
    s.add_systems((race::countdown_system, race::step_world, results_system).chain());
    s
}

pub fn subscribe(world: &mut World) {
    world.init_resource::<Intents>();
    world.init_resource::<Controls>();
    world.init_resource::<race::CueQueue>();
    // The platform host drives ads, audio policy and cloud saves through this
    // resource; every frame reads it, so it must exist before the first tick.
    world.init_resource::<platform::Platform>();
    world.init_resource::<crate::scene::SceneCache>();
    if world.get_resource::<Progress>().is_none() {
        world.insert_resource(Progress::new());
    }
}
