//! The art layer: renamite rigs. `.ren` text is the source of truth and what
//! ships (`include_str!`); `repame-actors` paints each rig as a Repose view,
//! so art is vector and resolution independent. Pack to `.renb` once the
//! published renamite runtime reads the local CLI's binary format.
//!
//! Hosts are `Rc<RefCell<..>>` and therefore `!Send`: they live here in the
//! shell, never in the sim world. The sim owns positions; this file turns
//! them into surfaces and ticks playback.

use bevy_ecs::prelude::*;
use glam::Vec2;
use repame_actors::{PlayerHostRef, actors_view, host_from_str};
use repose_core::RenderContext;
use repose_core::prelude::{Modifier, View};
use repose_ui::{Box as RBox, ViewExt as _, ZStack};

use crate::sim::state::{BURST_LIFE, ORB_COUNT, ORB_R, Orb, PLAYER_R, PlayerBody, Pops, RunState};
use crate::ui::palette::rect;

const BACKDROP: &str = include_str!("../assets/rigs/backdrop.ren");
const BLOB: &str = include_str!("../assets/rigs/blob.ren");
const COIN: &str = include_str!("../assets/rigs/coin.ren");
const BURST: &str = include_str!("../assets/rigs/burst.ren");

/// Burst ring slots. A burst lives `BURST_LIFE` seconds, so a dozen slots
/// cover any realistic pickup rate; slots wrap.
const BURST_SLOTS: usize = 12;

pub struct Rigs {
    backdrop: PlayerHostRef,
    player: PlayerHostRef,
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
            coins: (0..ORB_COUNT).map(|_| rig("coin", COIN)).collect(),
            bursts: (0..BURST_SLOTS).map(|_| rig("burst", BURST)).collect(),
        }
    }

    /// Sim-rate playback tick: a paused game freezes the art instead of
    /// banking wall-clock time. `steps` is the number of fixed ticks that ran.
    pub fn tick(&mut self, steps: u32) {
        if steps == 0 {
            return;
        }
        let dt = f64::from(steps) / 60.0;
        for host in [&self.backdrop, &self.player]
            .into_iter()
            .chain(&self.coins)
            .chain(&self.bursts)
        {
            host.borrow_mut().tick_playback_fixed(dt);
        }
    }

    /// Everything below the UI: sky, coins, pickup rings, player. `vp` is the
    /// live surface in dp; the grid scales through `s`.
    pub fn world_view(&self, world: &mut World, ctx: &RenderContext, vp: [f32; 2]) -> View {
        let s = grid_scale(vp);
        let backdrop = RBox(Modifier::new().fill_max_size())
            .child(actors_view(self.backdrop.clone(), ctx.clone()));
        let mut layers = vec![backdrop];
        if world.resource::<RunState>().playing() {
            let clock = world.resource::<RunState>().run_time_left;
            let bob = (clock * 3.0).sin() * 0.5 + 0.5;
            let mut orbs = world.query::<&Orb>();
            for (orb, host) in orbs.iter(world).zip(&self.coins) {
                let r = ORB_R * (1.0 + 0.10 * bob);
                layers.push(actor(host, orb.pos, r * 2.0, vp, s, ctx));
            }
        }
        for (index, (pos, t)) in world.resource::<Pops>().bursts.iter().enumerate() {
            let frac = 1.0 - (t / BURST_LIFE).clamp(0.0, 1.0);
            let r = ORB_R * (1.0 + frac * 2.6);
            let host = &self.bursts[index % BURST_SLOTS];
            layers.push(actor(host, *pos, r * 2.0, vp, s, ctx));
        }
        if let Some(pos) = player_pos(world) {
            let squash = 1.0 + 0.05 * (world.resource::<RunState>().run_time_left * 6.0).sin();
            layers.push(actor(
                &self.player,
                pos,
                PLAYER_R * squash * 2.0,
                vp,
                s,
                ctx,
            ));
        }
        ZStack(Modifier::new().fill_max_size()).child(layers)
    }
}

/// One rig surface on the design grid: a square box centred on `pos`, `size`
/// design units across. Artboards are square by construction, so `Exact` fit
/// never letterboxes.
fn actor(
    host: &PlayerHostRef,
    pos: Vec2,
    size: f32,
    vp: [f32; 2],
    s: f32,
    ctx: &RenderContext,
) -> View {
    let half = size * 0.5;
    RBox(rect(
        vp,
        s,
        [0.0; 4],
        [pos.x - half, pos.y - half, pos.x + half, pos.y + half],
    ))
    .child(actors_view(host.clone(), ctx.clone()))
}

fn grid_scale(vp: [f32; 2]) -> f32 {
    (vp[0] / crate::ui::DESIGN[0])
        .min(vp[1] / crate::ui::DESIGN[1])
        .max(1e-6)
}

fn player_pos(world: &mut World) -> Option<Vec2> {
    let mut q = world.query::<&PlayerBody>();
    q.iter(world).next().map(|b| b.pos)
}
