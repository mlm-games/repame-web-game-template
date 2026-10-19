//! Race lifecycle: sessions, AI opponents, checkpoints, results and rewards.
//!
//! Rewards and unlock costs are the shipped `Game.Level` values. The *shape* of a
//! run (start on a grid, checkpoints when the level requires them, finish at the
//! bottom, bonus for speed / time / damage) is read from the same table; anything
//! the build does not expose is marked DESIGNED in the README.

use bevy_ecs::prelude::*;
use glam::Vec3;

use crate::sim::car::{CarSim, Upgrades};
use crate::sim::data::{self, Level};
use crate::sim::meta::Progress;
use crate::sim::obstacles::{self, Obstacle};
use crate::sim::track::Track;

/// Checkpoints a `requireCheckpoints` level marks, as the shipped tracks do.
pub const CHECKPOINTS_PER_LEVEL: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Garage,
    MapSelect,
    Countdown,
    Racing,
    Finished,
    Failed,
}

/// Which screen the game is on. Its own resource because the garage and the map
/// screen exist before any level has been chosen, so there is no `Session` yet.
#[derive(bevy_ecs::prelude::Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Screen(pub Phase);

impl Default for Screen {
    fn default() -> Self {
        Screen(Phase::Garage)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Results {
    pub finish_time: f32,
    pub top_speed_kmh: f32,
    pub damage: f32,
    pub max_damage: f32,
    pub place: usize,
    pub racers: usize,
    pub coins: i32,
    pub completion_bonus: i32,
    pub speed_bonus: i32,
    pub time_bonus: i32,
    pub damage_bonus: i32,
    pub first_completion: bool,
    pub score: i32,
}

pub use crate::sim::ai::AiDriver as AiCar;

#[derive(Resource)]
pub struct TrackWorld {
    pub track: Track,
    pub obstacles: Vec<Obstacle>,
}

impl TrackWorld {
    pub fn load(level: &Level) -> Self {
        let mut track = Track::build(level);
        let placed = obstacles::place(level, track.length, track.half_width, |s| track.centre(s).x);
        // Holes and ramps are surface, not contact: the wheels have to find the
        // gap or the lip, otherwise the car is stopped by the art of a jump.
        for ob in &placed {
            let half = ob.kind.half_extents();
            match ob.kind {
                obstacles::Kind::Hole => track.features.push(crate::sim::track::SurfaceFeature {
                    kind: crate::sim::track::SurfaceKind::Hole,
                    s: ob.s,
                    lateral: 0.0,
                    half_width: track.half_width,
                    half_length: half.z,
                    height: 0.0,
                }),
                obstacles::Kind::Ramp => track.features.push(crate::sim::track::SurfaceFeature {
                    kind: crate::sim::track::SurfaceKind::Ramp,
                    s: ob.s,
                    lateral: ob.lateral,
                    half_width: half.x,
                    half_length: half.z,
                    height: half.y * 2.0,
                }),
                _ => {}
            }
        }
        Self {
            track,
            obstacles: placed,
        }
    }

    pub fn surface(&self, x: f32, z: f32) -> Option<f32> {
        self.track.surface(x, z)
    }
}

#[derive(Resource)]
pub struct Session {
    pub phase: Phase,
    pub level: usize,
    pub car: usize,
    /// `Game.Level.maxBotSpeed`, cached so the tick does not clone the row.
    pub max_bot_speed: f32,
    /// Cues are queued through the world, not the session.
    pub time: f32,
    pub countdown: f32,
    pub player: CarSim,
    pub ai: Vec<AiCar>,
    pub checkpoints: Vec<bool>,
    pub results: Option<Results>,
    /// Set once the run's results have been written to the save, so a finished
    /// run cannot pay out again on later ticks.
    pub recorded: bool,
}

/// Sound cues produced by the simulation, drained by the app each frame.
#[derive(Resource, Default)]
pub struct CueQueue(pub Option<Cue>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cue {
    Countdown,
    Go,
    Crash,
    Break,
    Checkpoint,
    Finish,
    Fail,
    Coin,
    Click,
}

impl Session {
    pub fn new(level_index: usize, car_index: usize, progress: &Progress) -> Self {
        let level = data::level_at(level_index);
        let car = data::car_at(car_index as i32);
        let upgrades = progress.upgrades_for(car.id.as_str());
        let start = Vec3::new(0.0, 0.0, 0.0);
        let player = CarSim::new(car, start, 0.0, upgrades);
        let ai = level
            .bot_configs
            .iter()
            .enumerate()
            .map(|(i, bot)| {
                let spec = data::car_at(bot.car);
                let skill = 0.82 + 0.045 * bot.skill as f32;
                // Grid: level flags use negative skill to mean the precise profile.
                let precise = bot.skill < 0;
                let config = data::ai_config(precise);
                let lane = if i % 2 == 0 { -3.5 } else { 3.5 } * (1.0 + (i / 2) as f32 * 0.35);
                let mut sim = CarSim::new(spec, start, 0.0, Upgrades::stock());
                sim.pos.x = lane;
                AiCar::new(sim, skill.min(1.0), config)
            })
            .collect();
        let checkpoints = vec![
            false;
            if level.require_checkpoints {
                CHECKPOINTS_PER_LEVEL
            } else {
                0
            }
        ];
        Self {
            phase: Phase::Countdown,
            level: level_index,
            car: car_index,
            max_bot_speed: level.max_bot_speed,
            time: 0.0,
            countdown: 3.0,
            player,
            ai,
            checkpoints,
            results: None,
            recorded: false,
        }
    }

    pub fn level(&self) -> Level {
        data::level_at(self.level)
    }

    /// `maxBotSpeed` without cloning the level row.
    pub fn max_bot_speed(&self) -> f32 {
        self.max_bot_speed
    }
}

/// Physics + race systems, run once per fixed step.
pub fn step_world(world: &mut World) {
    let Some(session) = world.get_resource::<Session>() else {
        return;
    };
    if session.phase != Phase::Racing {
        return;
    }
    let dt = world.resource::<repame_sim::SimTime>().delta_secs;
    let input = world.resource::<crate::sim::Controls>().0;
    let Some(mut session) = world.remove_resource::<Session>() else {
        return;
    };
    let Some(mut track_world) = world.remove_resource::<TrackWorld>() else {
        world.insert_resource(session);
        return;
    };
    let mut cues = world.remove_resource::<CueQueue>().unwrap_or_default();

    obstacles::update(
        &mut track_world.obstacles,
        session.time,
        track_world.track.half_width,
    );

    // Player
    session.player.step(dt, input, &track_world.track);
    resolve_collisions(
        &mut session.player,
        &mut track_world.obstacles,
        &track_world.track,
        session.time,
        &mut cues,
    );
    // AI
    let level_max = session.max_bot_speed();
    for ai in session.ai.iter_mut() {
        let controls = ai.control(&track_world, level_max, dt);
        ai.sim.step(dt, controls, &track_world.track);
        resolve_collisions(
            &mut ai.sim,
            &mut track_world.obstacles,
            &track_world.track,
            session.time,
            &mut cues,
        );
    }

    session.time += dt;
    // Checkpoints
    let cp_count = session.checkpoints.len();
    if cp_count > 0 {
        let z = session.player.pos.z;
        for i in 0..cp_count {
            let at = track_world
                .track
                .checkpoints
                .get(i)
                .copied()
                .unwrap_or(track_world.track.length * (i + 1) as f32 / (cp_count + 1) as f32);
            if !session.checkpoints[i] && z >= at {
                session.checkpoints[i] = true;
                cues.0 = Some(Cue::Checkpoint);
            }
        }
    }
    // Finish and failure
    if session.player.destroyed && session.phase == Phase::Racing {
        session.phase = Phase::Failed;
        crate::sim::set_phase(world, Phase::Failed);
        cues.0 = Some(Cue::Fail);
        let results = build_results(&session, &track_world, false);
        session.results = Some(results);
    } else if session.player.pos.z >= track_world.track.finish && session.phase == Phase::Racing {
        let passes = cp_count == 0 || session.checkpoints.iter().all(|c| *c);
        let phase = if passes {
            Phase::Finished
        } else {
            Phase::Failed
        };
        session.phase = phase;
        crate::sim::set_phase(world, phase);
        cues.0 = Some(if passes { Cue::Finish } else { Cue::Fail });
        let results = build_results(&session, &track_world, passes);
        session.results = Some(results);
    }

    world.insert_resource(session);
    world.insert_resource(track_world);
    world.insert_resource(cues);
}

pub fn countdown_system(world: &mut World) {
    let Some(session) = world.get_resource::<Session>() else {
        return;
    };
    if session.phase != Phase::Countdown {
        return;
    }
    let dt = world.resource::<repame_sim::SimTime>().delta_secs;
    let Some(mut session) = world.remove_resource::<Session>() else {
        return;
    };
    let mut cues = world.remove_resource::<CueQueue>().unwrap_or_default();
    let before = session.countdown.ceil();
    session.countdown -= dt;
    let after = session.countdown.ceil();
    if session.countdown <= 0.0 {
        session.phase = Phase::Racing;
        crate::sim::set_phase(world, Phase::Racing);
        cues.0 = Some(Cue::Go);
    } else if before != after {
        cues.0 = Some(Cue::Countdown);
    }
    world.insert_resource(session);
    world.insert_resource(cues);
}

fn build_results(session: &Session, track_world: &TrackWorld, finished: bool) -> Results {
    let level = session.level();
    let finish_time = session.time;
    let top_speed = session.player.top_speed.max(session.player.speed() * 3.6);
    let damage = session.player.damage;
    let max_damage = session.player.max_damage();
    let mut rank = 1;
    for ai in &session.ai {
        if ai.sim.distance > session.player.distance {
            rank += 1;
        }
    }
    let mut coins = 0;
    let mut completion = 0;
    let mut speed_bonus = 0;
    let mut time_bonus = 0;
    let mut damage_bonus = 0;
    if finished {
        completion = level.reward_for_completion;
        if top_speed >= level.speed_to_get_reward as f32 {
            speed_bonus = level.reward_for_speed;
        }
        if finish_time <= level.time_to_get_reward as f32 {
            time_bonus = level.reward_for_time;
        }
        if damage >= max_damage * 0.85 {
            damage_bonus = level.reward_for_max_damage;
        }
        coins = completion + speed_bonus + time_bonus + damage_bonus;
    }
    let score = if finished {
        let speed_component = if speed_bonus > 0 { 250 } else { 0 };
        let time_component = if time_bonus > 0 { 250 } else { 0 };
        level.score_for_completion + speed_component + time_component
    } else {
        (session.player.distance * 2.0) as i32
    };
    let _ = track_world;
    Results {
        finish_time,
        top_speed_kmh: top_speed,
        damage,
        max_damage,
        place: rank,
        racers: session.ai.len() + 1,
        coins,
        completion_bonus: completion,
        speed_bonus,
        time_bonus,
        damage_bonus,
        first_completion: false,
        score,
    }
}

/// Impulse-based obstacle contact shared by the player and the AI.
/// Obstacle placements are sorted by distance along the track, so only the
/// window around a car can touch it (contact reach is ~12 m).
pub const CONTACT_WINDOW: f32 = 16.0;

pub fn resolve_collisions(
    car: &mut CarSim,
    obstacles: &mut [Obstacle],
    track: &Track,
    now: f32,
    cues: &mut CueQueue,
) {
    if car.destroyed {
        return;
    }
    let half_w = car.spec.track_width() * 0.5;
    let from = obstacles.partition_point(|ob| ob.s < car.pos.z - CONTACT_WINDOW);
    let to = obstacles.partition_point(|ob| ob.s <= car.pos.z + CONTACT_WINDOW);
    for ob in obstacles[from..to].iter_mut() {
        if ob.broken {
            continue;
        }
        if matches!(ob.kind, obstacles::Kind::Hole | obstacles::Kind::Ramp) {
            continue;
        }
        let centre = track.centre(ob.s) + ob.offset;
        let cx = centre.x + ob.lateral;
        let half = ob.kind.half_extents();
        let dx = (car.pos.x - cx).abs();
        let dz = (car.pos.z - ob.s).abs();
        let dy = (car.pos.y + 0.5 - centre.y).abs();
        if dx < half.x + half_w && dz < half.z + 2.2 && dy < half.y + 1.2 {
            if now - ob.hit < obstacles::HIT_COOLDOWN {
                continue;
            }
            ob.hit = now;
            let speed = car.speed();
            let mass = car.spec.mass;
            // Damage is the collision magnitude the shipped controller reads,
            // clamped to its ceiling: a full-speed hit costs about a twentieth
            // of the car.
            let magnitude = speed.min(obstacles::MAX_COLLISION_MAGNITUDE);
            if ob.kind.is_breakable() {
                if obstacles::should_break(ob, mass, speed) {
                    ob.broken = true;
                    car.damage += magnitude * 0.25;
                    car.vel *= 0.94;
                    cues.0 = Some(Cue::Break);
                } else {
                    car.damage += magnitude * 0.6;
                    car.vel *= 0.45;
                    cues.0 = Some(Cue::Crash);
                }
            } else if ob.kind.kills_on_impact() {
                car.damage += if speed > 6.0 {
                    car.max_damage()
                } else {
                    magnitude * 0.5
                };
                car.vel *= 0.35;
                cues.0 = Some(Cue::Crash);
            } else {
                car.damage += magnitude * 0.5;
                car.vel *= 0.6;
                cues.0 = Some(Cue::Crash);
            }
            if car.damage >= car.max_damage() {
                car.destroyed = true;
            }
        }
    }
}
