//! The app shell: sim, platform host, audio, frame loop. Desktop and web
//! run the same shell; only the platform host differs.

mod audio;
mod platform;

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use bevy_ecs::schedule::Schedule;
use glam::Vec2;
use repame_shell::{
    SharedEdges, Staging, install_game_shortcuts, shared_edges, take_shortcut_edges,
};
use repame_sim::Sim;
use repose_core::Dp;
use repose_core::input::PhysicalKey;
use repose_core::prelude::{FocusRequester, Modifier, View};
use repose_core::{RenderContext, Scheduler, request_frame};
use repose_ui::{Box as RBox, Spacer, ViewExt as _};
use web_time::{Duration, Instant};

use crate::rigs::Rigs;
use crate::sim::save::SaveGame;
use crate::sim::state::{Intents, RunState, Transition, apply_intents, break_system, spawn_world};
use crate::sim::{platform as sim_platform, state};
use crate::ui::palette;
use crate::ui::{
    SharedUi, Sink, TouchHandle, UiAction, UiActions, compose_root, drain_actions, sync_shared_ui,
};

use self::audio::AudioDirector;
use self::platform::PlatformHost;

const FREDOKA_BOLD: &[u8] = include_bytes!("../../assets/fonts/Fredoka-Bold.ttf");

const SHORTCUT_PAUSE: &str = "web-game.pause";
const SHORTCUT_RESTART: &str = "web-game.restart";
const SHORTCUT_CONFIRM: &str = "web-game.confirm";

fn ensure_fonts() {
    static ONCE: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    ONCE.get_or_init(|| {
        repose_text::register_font_data(FREDOKA_BOLD);
    });
}

pub struct App {
    sim: Sim,
    platform: PlatformHost,
    schedule: Schedule,
    staging: Rc<RefCell<Staging>>,
    shortcut_edges: SharedEdges,
    ui: SharedUi,
    rigs: Rigs,
    audio: AudioDirector,
    held: HashSet<PhysicalKey>,
    viewport_dp: [f32; 2],
}

impl App {
    /// The desktop build: no platform behind the game.
    pub fn new() -> Self {
        Self::with_host(PlatformHost::offline())
    }

    /// The web build, wired to the Playgama bridge.
    pub fn with_bridge(bridge: playgama_bridge::Bridge) -> Self {
        platform::watch_foreground();
        Self::with_host(PlatformHost::web(bridge))
    }

    fn with_host(host: PlatformHost) -> Self {
        ensure_fonts();
        let mut sim = Sim::with_default_step();
        let save = SaveGame::new();
        let muted = save.data.muted;
        let best = save.data.best_score;
        sim.world.insert_resource(save);
        sim.world.init_resource::<UiActions>();
        sim.world.insert_resource(RunState {
            best,
            ..RunState::default()
        });
        sim.world.insert_resource(state::InputState::default());
        sim.world.insert_resource(Intents::default());
        sim.world.insert_resource(state::Pops::default());
        sim.world.insert_resource(state::SfxQueue::default());
        sim.world.insert_resource(sim_platform::Platform::default());
        sim.world.insert_resource(Transition::default());
        spawn_world(&mut sim.world);
        let touch = TouchHandle::default();
        Self {
            sim,
            platform: host,
            schedule: crate::sim::build_schedule(),
            staging: Staging::shared(),
            shortcut_edges: shared_edges(),
            ui: SharedUi {
                touch,
                ..SharedUi::default()
            },
            rigs: Rigs::new(),
            audio: AudioDirector::new(muted),
            held: HashSet::new(),
            viewport_dp: state::WORLD,
        }
    }

    /// One frame. `dt` is wall time; the sim banks it and steps at its fixed
    /// rate.
    pub fn view(&mut self, sched: &mut Scheduler, ctx: &RenderContext, dt: Duration) -> View {
        request_frame();
        install_game_shortcuts(
            &self.shortcut_edges,
            SHORTCUT_PAUSE,
            SHORTCUT_RESTART,
            SHORTCUT_CONFIRM,
        );
        self.staging.borrow_mut().feed_polled(sched);
        self.held = self.staging.borrow().held.clone();
        self.viewport_dp = prepare_viewport(sched);
        self.ui.viewport = Vec2::new(self.viewport_dp[0], self.viewport_dp[1]);
        self.feed_shortcuts();
        self.feed_input();
        // The platform folds in ads, pause and cloud saves. The rigs are
        // parsed at boot, so the first frame is playable and `game_ready` may
        // go out.
        self.platform
            .update(&mut self.sim.world, dt.as_secs_f32(), true);
        if !self.platform.suspends_gameplay() {
            for action in drain_actions(&mut self.sim.world) {
                self.apply_action(action);
            }
            apply_intents(&mut self.sim.world);
        }
        let steps = self.advance(dt);
        self.rigs.tick(steps);
        self.sync_audio(dt);
        sync_shared_ui(&self.sim.world, &mut self.ui);
        self.compose(ctx)
    }

    /// Shortcut edges (Escape / R / Enter) become sim intents, the same route
    /// the on-screen buttons take.
    fn feed_shortcuts(&mut self) {
        let (pause, restart, confirm) = take_shortcut_edges(&self.shortcut_edges);
        let mut intents = self.sim.world.resource_mut::<Intents>();
        if pause {
            intents.toggle_pause = true;
        }
        if restart {
            intents.retry = true;
        }
        if confirm {
            intents.play = true;
        }
    }

    fn feed_input(&mut self) {
        let x = i32::from(self.key(PhysicalKey::KeyD) || self.key(PhysicalKey::ArrowRight))
            - i32::from(self.key(PhysicalKey::KeyA) || self.key(PhysicalKey::ArrowLeft));
        let y = i32::from(self.key(PhysicalKey::KeyS) || self.key(PhysicalKey::ArrowDown))
            - i32::from(self.key(PhysicalKey::KeyW) || self.key(PhysicalKey::ArrowUp));
        let mut dir = Vec2::new(x as f32, y as f32) + self.ui.touch.borrow().value;
        if dir.length() > 1.0 {
            dir = dir.normalize();
        }
        let playing = self.sim.world.resource::<RunState>().playing();
        // A finger that is still down when the run ends must not keep walking
        // the player through the menu.
        if !playing {
            self.ui.touch.borrow_mut().release();
        }
        self.sim.world.resource_mut::<state::InputState>().dir = dir;
    }

    fn key(&self, key: PhysicalKey) -> bool {
        self.held.contains(&key)
    }

    fn apply_action(&mut self, action: UiAction) {
        let mut intents = self.sim.world.resource_mut::<Intents>();
        match action {
            UiAction::Play => intents.play = true,
            UiAction::Retry => intents.retry = true,
            UiAction::Revive => intents.revive = true,
            UiAction::Menu => intents.menu = true,
            UiAction::Pause => intents.toggle_pause = true,
            UiAction::Mute => intents.toggle_mute = true,
        }
    }

    /// The sim always banks wall time: the break watchdog needs ticks while an
    /// ad is up, and only the gameplay schedule itself is gated on the run
    /// being live. That is the Godot original's `PAUSABLE` gate: the schedule
    /// stops, the shell keeps running.
    fn advance(&mut self, dt: Duration) -> u32 {
        let run = self.sim.world.resource::<RunState>();
        let live = run.playing() && !run.transitional();
        let suspended = self.platform.suspends_gameplay() && !run.transitional();
        self.sim.time_scale = if suspended { 0.0 } else { 1.0 };
        let schedule = &mut self.schedule;
        self.sim.step_with(dt, |world| {
            break_system(world);
            // The wipe runs outside the gameplay schedule: it owns the
            // screen while nothing else is live.
            crate::sim::state::transition_system(world);
            if live {
                schedule.run(world);
            }
        })
    }

    fn sync_audio(&mut self, dt: Duration) {
        let muted = self.sim.world.resource::<SaveGame>().data.muted || self.platform.silenced();
        if self.audio.muted() != muted {
            self.audio.set_muted(muted);
        }
        let cues = std::mem::take(&mut self.sim.world.resource_mut::<state::SfxQueue>().events);
        for cue in cues {
            self.audio.play(cue);
        }
        self.audio.update(dt.as_secs_f32());
    }

    fn compose(&mut self, ctx: &RenderContext) -> View {
        let world = self
            .rigs
            .world_view(&mut self.sim.world, ctx, self.viewport_dp);
        let wipe = self.wipe_layer();
        let actions = self
            .sim
            .world
            .get_resource::<UiActions>()
            .cloned()
            .unwrap_or_default();
        let sink: Sink = actions.0.clone();
        let root = compose_root(&self.ui, sink);

        let focus = repose_core::remember(FocusRequester::new);
        let focus_trigger = (*focus).clone();
        let staging = self.staging.clone();
        let key_staging = self.staging.clone();
        repose_ui::ZStack(
            Modifier::new()
                .fill_max_size()
                .focusable(true)
                .focus_requester((*focus).clone())
                .on_globally_positioned(move |_| focus_trigger.request_focus())
                .on_focus_changed(move |focused| {
                    staging.borrow_mut().set_window_focused(focused);
                })
                .on_key_event(move |ke| {
                    key_staging.borrow_mut().handle_key(&ke);
                    false
                }),
        )
        .child((world, root, wipe))
    }

    /// The gold disc of the scene wipe. It is a square box pinned by
    /// `.size()` + a single corner offset and rounded to half its side, so
    /// it stays a true circle centred on the viewport at any window shape;
    /// a four-sided offset stretch would skew it and drag it off centre. Its
    /// radius is a fraction of the viewport's half diagonal, so full cover
    /// always hides the scene swap.
    fn wipe_layer(&self) -> View {
        let Some(frac) = self.sim.world.resource::<Transition>().disc() else {
            return Spacer();
        };
        let vp = self.ui.viewport;
        // A hair past the half diagonal, so at full cover the anti-aliased
        // edge is off-screen and no corner peeks through at the swap.
        let cover = (vp.x * vp.x + vp.y * vp.y).sqrt() * 0.5 * 1.02 * frac;
        let side = cover * 2.0;
        let x = vp.x * 0.5 - cover;
        let y = vp.y * 0.5 - cover;
        RBox(
            Modifier::new()
                .absolute()
                .offset(Some(Dp(x)), Some(Dp(y)), None, None)
                .size(Dp(side), Dp(side))
                .background(palette::GOLD)
                .clip_rounded(Dp(cover)),
        )
        .child(Spacer())
    }
}

/// The design view in dp: the polled surface divided by the density scale, so
/// a phone and a desktop window lay out the same grid.
fn prepare_viewport(sched: &Scheduler) -> [f32; 2] {
    let density = repose_core::locals::effective_density_scale().max(1e-6);
    if sched.size.0 == 0 || sched.size.1 == 0 {
        state::WORLD
    } else {
        [sched.size.0 as f32 / density, sched.size.1 as f32 / density]
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

/// Wall-clock driver: `dt` capped so a backgrounded tab does not resume with
/// a catch-up burst.
struct AppRuntime {
    app: App,
    previous: Instant,
}

impl AppRuntime {
    fn view(&mut self, sched: &mut Scheduler, ctx: &RenderContext) -> View {
        let now = Instant::now();
        let dt = now
            .duration_since(self.previous)
            .min(Duration::from_millis(250));
        self.previous = now;
        self.app.view(sched, ctx, dt)
    }
}

/// Desktop entry: window, renderer and platform stand-ins.
pub fn run_desktop() -> anyhow::Result<()> {
    let _ = env_logger::try_init();
    let mut runtime = AppRuntime {
        app: App::new(),
        previous: Instant::now(),
    };
    repame_shell::run_desktop("Web Game", (1280, 720), move |sched, ctx| {
        runtime.view(sched, ctx)
    })
}

/// Web entry: the bridge is handed over by `lib.rs` once the page has it.
#[cfg(target_arch = "wasm32")]
pub fn run_web(bridge: Option<playgama_bridge::Bridge>) -> Result<(), wasm_bindgen::JsValue> {
    let app = match bridge {
        Some(bridge) => App::with_bridge(bridge),
        None => App::new(),
    };
    let mut runtime = AppRuntime {
        app,
        previous: Instant::now(),
    };
    repame_shell::run_web(move |sched, ctx| runtime.view(sched, ctx))
}
