mod hud;
pub mod menus;
pub mod palette;
mod touch;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use bevy_ecs::prelude::*;
use glam::Vec2;
use repose_core::prelude::{AlignItems, JustifyContent, Modifier, View};
use repose_ui::ViewExt as _;
use repose_ui::{Box as RBox, Spacer, ZStack};

use crate::sim::save::SaveGame;
use crate::sim::state::{BreakThen, Panel, Phase, Pops, RunState};
use palette::{CREAM, label, rect};

pub const DESIGN: [f32; 2] = crate::sim::state::WORLD;

pub type Sink = Arc<Mutex<Vec<UiAction>>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiAction {
    Play,
    Retry,
    Revive,
    Menu,
    Pause,
    Mute,
}

#[derive(Resource, Clone, Default)]
pub struct UiActions(pub Sink);

impl UiActions {
    pub fn push(&self, action: UiAction) {
        self.0.lock().unwrap().push(action);
    }
}

pub fn drain_actions(world: &mut World) -> Vec<UiAction> {
    match world.get_resource_mut::<UiActions>() {
        Some(slot) => std::mem::take(&mut *slot.0.lock().unwrap()),
        None => Vec::new(),
    }
}

pub type TouchHandle = Rc<RefCell<touch::TouchState>>;

#[derive(Clone)]
pub struct SharedUi {
    pub score: i32,
    pub time_left: f32,
    pub best: i32,
    pub record: bool,
    pub muted: bool,
    pub paused: bool,
    pub modal: bool,
    pub phase: Phase,
    pub panel: Panel,
    pub revive_pending: bool,
    pub revives_left: i32,
    pub viewport: Vec2,
    pub touch: TouchHandle,
    pub pops: Vec<PopView>,
}

impl SharedUi {
    pub fn ui_scale(&self) -> f32 {
        (self.viewport.x / DESIGN[0])
            .min(self.viewport.y / DESIGN[1])
            .max(1e-6)
    }

    pub fn world_scale(&self) -> f32 {
        (self.viewport.x / DESIGN[0])
            .max(self.viewport.y / DESIGN[1])
            .max(1e-6)
    }
}

impl Default for SharedUi {
    fn default() -> Self {
        Self {
            score: 0,
            time_left: 0.0,
            best: 0,
            record: false,
            muted: false,
            paused: false,
            modal: false,
            phase: Phase::Menu,
            panel: Panel::Main,
            revive_pending: false,
            revives_left: 1,
            viewport: Vec2::new(DESIGN[0], DESIGN[1]),
            touch: Default::default(),
            pops: Vec::new(),
        }
    }
}

pub fn sync_shared_ui(world: &World, ui: &mut SharedUi) {
    let Some(run) = world.get_resource::<RunState>() else {
        return;
    };
    ui.score = run.score;
    ui.time_left = run.run_time_left;
    ui.best = run.best;
    ui.record = run.record;
    ui.paused = run.paused;
    ui.modal = run.modal();
    ui.phase = run.phase;
    ui.panel = run.panel;
    ui.revive_pending = run.revive_pending;
    ui.revives_left = run.revives_left;
    if let Some(save) = world.get_resource::<SaveGame>() {
        ui.muted = save.data.muted;
    }
    if let Some(pops) = world.get_resource::<Pops>() {
        ui.pops = pops
            .list
            .iter()
            .map(|(pos, t)| PopView {
                pos: [pos.x, pos.y],
                t: *t,
            })
            .collect();
    }
}

#[derive(Clone, Copy)]
pub struct PopView {
    pub pos: [f32; 2],
    pub t: f32,
}

fn pops(ui: &SharedUi, s: f32, vp: [f32; 2]) -> View {
    if ui.pops.is_empty() {
        return Spacer();
    }
    let children: Vec<View> = ui
        .pops
        .iter()
        .map(|pop| {
            let frac = (pop.t / crate::sim::state::POP_LIFE).clamp(0.0, 1.0);
            let lift = 18.0 + 28.0 * (1.0 - frac);
            let half = 44.0;
            let alpha = if frac > 0.8 {
                (1.0 - (frac - 0.8) / 0.2).clamp(0.0, 1.0)
            } else if frac < 0.2 {
                frac / 0.2
            } else {
                1.0
            };
            RBox(
                rect(
                    vp,
                    s,
                    [0.0; 4],
                    [
                        pop.pos[0] - half,
                        pop.pos[1] - lift - half,
                        pop.pos[0] + half,
                        pop.pos[1] - lift + half,
                    ],
                )
                .hit_passthrough()
                .align_items(AlignItems::CENTER)
                .justify_content(JustifyContent::CENTER),
            )
            .child(label(
                Modifier::new().alpha(alpha),
                "+1",
                20.0 + 9.0 * (1.0 - frac),
                CREAM,
                s,
            ))
        })
        .collect();
    ZStack(Modifier::new().fill_max_size()).child(children)
}

pub fn compose_root(ui: &SharedUi, sink: Sink) -> View {
    let s = ui.ui_scale();
    let vp = [ui.viewport.x, ui.viewport.y];
    let base = hud::compose(ui, s, vp, &sink);
    let touch = touch::compose(ui, s, vp);
    let overlay = match ui.phase {
        Phase::Menu => match ui.panel {
            Panel::Main => menus::compose_main(ui, s, vp, &sink),
            Panel::Death => menus::compose_death(ui, s, vp, &sink),
            Panel::None => touch.clone(),
        },
        Phase::Break(BreakThen::Revive) if ui.revive_pending => {
            menus::compose_death(ui, s, vp, &sink)
        }
        Phase::Playing if ui.paused => menus::compose_pause(ui, s, vp, &sink),
        _ => touch.clone(),
    };
    let pops = pops(ui, ui.world_scale(), vp);
    repose_ui::ZStack(Modifier::new().fill_max_size()).child((base, pops, touch, overlay))
}
