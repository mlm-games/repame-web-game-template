use bevy_ecs::prelude::*;
use glam::Vec2;
use repame_view2d::{PlayerHostRef, actors_view, host_from_str};
use repose_core::RenderContext;
use repose_core::{
    Alignment,
    prelude::{Dp, Modifier, View},
};
use repose_ui::{Box as RBox, ViewExt as _, ZStack};

use crate::sim::state::{
    BURST_LIFE, ORB_COUNT, ORB_R, ORB_SPAWN_LIFE, Orb, PLAYER_R, PlayerBody, Pops, RunState,
};
use crate::ui::palette::rect;

const BACKDROP: &str = include_str!("../assets/rigs/backdrop.ren");
const BLOB: &str = include_str!("../assets/rigs/blob.ren");
const BLOB_PUPILS: &str = include_str!("../assets/rigs/blob_pupils.ren");

const PUPIL_LOOK: f32 = 2.6;
const COIN: &str = include_str!("../assets/rigs/coin.ren");
const BURST: &str = include_str!("../assets/rigs/burst.ren");

const BURST_SLOTS: usize = 12;

pub struct Rigs {
    backdrop: PlayerHostRef,
    player: PlayerHostRef,
    player_pupils: PlayerHostRef,
    coins: Vec<PlayerHostRef>,
    bursts: Vec<PlayerHostRef>,
}

impl Default for Rigs {
    fn default() -> Self {
        Self::new()
    }
}

impl Rigs {
    pub fn new() -> Self {
        let rig = |name: &str, source: &str| {
            host_from_str(source)
                .unwrap_or_else(|error| panic!("rig {name} did not parse: {error}"))
        };
        Self {
            backdrop: rig("backdrop", BACKDROP),
            player: rig("blob", BLOB),
            player_pupils: rig("blob_pupils", BLOB_PUPILS),
            coins: (0..ORB_COUNT).map(|_| rig("coin", COIN)).collect(),
            bursts: (0..BURST_SLOTS).map(|_| rig("burst", BURST)).collect(),
        }
    }

    pub fn tick(&mut self, steps: u32) {
        if steps == 0 {
            return;
        }
        let dt = f64::from(steps) / 60.0;
        for host in [&self.backdrop, &self.player, &self.player_pupils]
            .into_iter()
            .chain(&self.coins)
            .chain(&self.bursts)
        {
            host.borrow_mut().tick_playback_fixed(dt);
        }
    }

    pub fn world_view(&self, world: &mut World, ctx: &RenderContext, vp: [f32; 2]) -> View {
        let s = grid_scale(vp);
        let backdrop = RBox(cover_box(vp)).child(actors_view(self.backdrop.clone(), ctx.clone()));
        let mut layers = vec![backdrop];
        if world.resource::<RunState>().playing() {
            let clock = world.resource::<RunState>().run_time_left;
            let mut orbs = world.query::<&Orb>();
            for (orb, host) in orbs.iter(world).zip(&self.coins) {
                let bob = (clock * 3.0 + orb.bob).sin() * 0.5 + 0.5;
                let r = ORB_R * (1.0 + 0.10 * bob) * entrance(orb.age);
                layers.push(actor(host, orb.pos, r * 2.0, 1.0, vp, s, ctx));
            }
        }
        for (index, (pos, t)) in world.resource::<Pops>().bursts.iter().enumerate() {
            let frac = 1.0 - (t / BURST_LIFE).clamp(0.0, 1.0);
            let r = ORB_R * (RING_START + frac * RING_GROWTH);
            let host = &self.bursts[index % BURST_SLOTS];
            layers.push(actor(host, *pos, r * 2.0, burst_alpha(frac), vp, s, ctx));
        }
        if let Some((pos, look)) = player_body(world) {
            let squash = 1.0 + 0.05 * (world.resource::<RunState>().run_time_left * 6.0).sin();
            let size = PLAYER_R * squash * 2.0;
            layers.push(actor(&self.player, pos, size, 1.0, vp, s, ctx));
            let eyes = pos + look * PUPIL_LOOK;
            layers.push(actor(&self.player_pupils, eyes, size, 1.0, vp, s, ctx));
        }
        ZStack(
            Modifier::new()
                .fill_max_size()
                .content_alignment(Alignment::Center),
        )
        .child(layers)
    }
}

fn actor(
    host: &PlayerHostRef,
    pos: Vec2,
    size: f32,
    alpha: f32,
    vp: [f32; 2],
    s: f32,
    ctx: &RenderContext,
) -> View {
    let half = size * 0.5;
    let mut box_modifier = rect(
        vp,
        s,
        [0.0; 4],
        [pos.x - half, pos.y - half, pos.x + half, pos.y + half],
    );
    if alpha < 1.0 {
        box_modifier = box_modifier.alpha(alpha.clamp(0.0, 1.0));
    }
    RBox(box_modifier).child(actors_view(host.clone(), ctx.clone()))
}

pub fn grid_scale(vp: [f32; 2]) -> f32 {
    (vp[0] / crate::ui::DESIGN[0])
        .max(vp[1] / crate::ui::DESIGN[1])
        .max(1e-6)
}

pub fn field_size(vp: [f32; 2]) -> [f32; 2] {
    let s = grid_scale(vp);
    [vp[0] / s, vp[1] / s]
}

fn cover_box(vp: [f32; 2]) -> Modifier {
    let aspect = crate::ui::DESIGN[0] / crate::ui::DESIGN[1];
    let w = vp[0].max(vp[1] * aspect);
    let h = vp[1].max(vp[0] / aspect);
    Modifier::new().size(Dp(w), Dp(h))
}

fn entrance(age: f32) -> f32 {
    let t = (age / ORB_SPAWN_LIFE).clamp(0.0, 1.0);
    const C1: f32 = 1.70158;
    const C3: f32 = C1 + 1.0;
    let u = t - 1.0;
    1.0 + C3 * u * u * u + C1 * u * u
}

const RING_START: f32 = 1.35;
const RING_GROWTH: f32 = 1.15;

fn burst_alpha(frac: f32) -> f32 {
    const FADE_START: f32 = 0.45;
    if frac <= FADE_START {
        return 1.0;
    }
    let u = (frac - FADE_START) / (1.0 - FADE_START);
    (1.0 - u * u).clamp(0.0, 1.0)
}

fn player_body(world: &mut World) -> Option<(Vec2, Vec2)> {
    let mut q = world.query::<&PlayerBody>();
    q.iter(world).next().map(|b| (b.pos, b.look))
}
