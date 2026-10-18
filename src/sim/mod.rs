pub mod platform;
pub mod save;
pub mod state;

use bevy_ecs::prelude::*;
use glam::Vec2;

use crate::sim::state::{
    BURST_LIFE, InputState, LOOK_LERP, Orb, PLAYER_R, PLAYER_SPEED, POP_LIFE, PlayerBody, Pops,
    RunState, SfxCue, end_run, queue_sfx,
};
use repame_sim::rng::{Rng as _, SimRng};

pub fn build_schedule() -> Schedule {
    let mut s = Schedule::default();
    s.add_systems((player_system, pickup_system, pops_system, run_clock).chain());
    s
}

fn player_system(world: &mut World) {
    if !world.resource::<RunState>().playing() {
        return;
    }
    let dt = world.resource::<repame_sim::SimTime>().delta_secs;
    let dir = world.resource::<InputState>().dir;
    let Some(entity) = crate::sim::state::player_entity(world) else {
        return;
    };
    let field = world.resource::<crate::sim::state::Field>();
    let (fw, fh) = (field.width, field.height);
    let mut body = world.entity_mut(entity);
    let mut pos = body.get_mut::<PlayerBody>().expect("player").pos;
    if dir.length_squared() > 1.0 {
        pos += dir.normalize_or_zero() * (PLAYER_SPEED * dt);
    } else {
        pos += dir * (PLAYER_SPEED * dt);
    }
    pos = pos.clamp(
        Vec2::splat(PLAYER_R),
        Vec2::new(fw - PLAYER_R, fh - PLAYER_R),
    );
    body.get_mut::<PlayerBody>().expect("player").pos = pos;
    let target = dir.normalize_or_zero();
    let k = 1.0 - (-LOOK_LERP * dt).exp();
    let look = body.get_mut::<PlayerBody>().expect("player").look;
    body.get_mut::<PlayerBody>().expect("player").look = look.lerp(target, k.clamp(0.0, 1.0));
}

fn pickup_system(world: &mut World) {
    if !world.resource::<RunState>().playing() {
        return;
    }
    let player = {
        let Some(entity) = crate::sim::state::player_entity(world) else {
            return;
        };
        world.get::<PlayerBody>(entity).map(|b| b.pos)
    };
    let Some(player) = player else {
        return;
    };
    let reach = PLAYER_R + crate::sim::state::ORB_R;
    let mut collected = Vec::new();
    let mut orbs = world.query::<(Entity, &Orb)>();
    for (entity, orb) in orbs.iter(world) {
        if player.distance(orb.pos) <= reach {
            collected.push((entity, orb.pos));
        }
    }
    if collected.is_empty() {
        return;
    }
    let mut rng = SimRng::seeded(
        0x5EED_ABCD ^ (world.resource::<RunState>().score as u64).wrapping_mul(2_654_435_761),
    );
    for (entity, at) in collected {
        let spot = free_orb_spot(&mut rng, world);
        let mut orb = world.entity_mut(entity);
        let mut slot = orb.get_mut::<Orb>().expect("orb");
        slot.pos = spot;
        slot.age = 0.0;
        drop(slot);
        let mut run = world.resource_mut::<RunState>();
        run.score += 1;
        world.resource_mut::<Pops>().list.push((at, POP_LIFE));
        world.resource_mut::<Pops>().bursts.push((at, BURST_LIFE));
        queue_sfx(world, SfxCue::Pickup);
    }
}

fn pops_system(world: &mut World) {
    let dt = world.resource::<repame_sim::SimTime>().delta_secs;
    let mut pops = world.resource_mut::<Pops>();
    for pop in pops.list.iter_mut() {
        pop.1 -= dt;
    }
    pops.list.retain(|pop| pop.1 > 0.0);
    for burst in pops.bursts.iter_mut() {
        burst.1 -= dt;
    }
    pops.bursts.retain(|burst| burst.1 > 0.0);
    drop(pops);
    for mut orb in world.query::<&mut Orb>().iter_mut(world) {
        orb.age += dt;
    }
}

fn run_clock(world: &mut World) {
    if !world.resource::<RunState>().playing() {
        return;
    }
    let dt = world.resource::<repame_sim::SimTime>().delta_secs;
    let left = world.resource::<RunState>().run_time_left - dt;
    if left > 0.0 {
        world.resource_mut::<RunState>().run_time_left = left;
        return;
    }
    world.resource_mut::<RunState>().run_time_left = 0.0;
    queue_sfx(world, SfxCue::Death);
    end_run(world);
}

fn free_orb_spot(rng: &mut SimRng, world: &mut World) -> Vec2 {
    let field = world.resource::<crate::sim::state::Field>();
    let (fw, fh) = (field.width, field.height);
    let taken = world
        .query::<&Orb>()
        .iter(world)
        .map(|orb| orb.pos)
        .collect::<Vec<_>>();
    let player = world
        .query::<&PlayerBody>()
        .iter(world)
        .next()
        .map(|b| b.pos)
        .unwrap_or(Vec2::new(fw * 0.5, fh * 0.5));
    for _ in 0..32 {
        let candidate = Vec2::new(
            rng.random_range(crate::sim::state::SPAWN_MARGIN..fw - crate::sim::state::SPAWN_MARGIN),
            rng.random_range(crate::sim::state::SPAWN_MARGIN..fh - crate::sim::state::SPAWN_MARGIN),
        );
        if taken
            .iter()
            .all(|pos| pos.distance(candidate) > crate::sim::state::ORB_R * 6.0)
            && player.distance(candidate) > PLAYER_R * 4.0
        {
            return candidate;
        }
    }
    Vec2::new(fw * 0.5, fh * 0.5)
}
