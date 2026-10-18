mod audio;
mod platform;

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use bevy_ecs::change_detection::Mut;
use bevy_ecs::schedule::Schedule;
use glam::Vec2;
use repame_shell::{
    SharedEdges, Staging, install_game_shortcuts, shared_edges, take_shortcut_edges,
};
use repame_sim::Sim;
use repame_view3d::GeomHandle;
use repose_core::input::PhysicalKey;
use repose_core::prelude::{FocusRequester, Modifier, View};
use repose_core::{RenderContext, Scheduler, request_frame};
use repose_ui::{ViewExt as _, ZStack};
use web_time::{Duration, Instant};

use crate::scene;
use crate::sim::car::Input;
use crate::sim::data;
use crate::sim::meta::Progress;
use crate::sim::race::Phase;
use crate::sim::{Intent, Intents};
use crate::ui::{self, SharedUi, Sink, UiAction, UiActions, drain_actions, sync_shared_ui};

use self::audio::AudioDirector;
use self::platform::PlatformHost;

const FREDOKA_BOLD: &[u8] = include_bytes!("../../assets/fonts/Fredoka-Bold.ttf");

/// Playtest hook: `DD_AUTOPLAY=<level index>` starts that level immediately and
/// drives the player with the same controller the bots use, printing a trace.
/// `DD_AUTOPLAY_TRACE=0` silences the trace. Development only, never shipped on.
pub struct Autoplay {
    level: usize,
    driver: crate::sim::ai::AiDriver,
    started: bool,
    ticks: u32,
    trace: bool,
}

impl Autoplay {
    fn from_env(sim: &mut Sim) -> Option<Self> {
        let raw = std::env::var("DD_AUTOPLAY").ok()?;
        let level = raw.parse::<usize>().ok()?;
        // Autoplay unlocks the table so any level can be inspected.
        if let Some(mut progress) = sim.world.get_resource_mut::<Progress>() {
            progress.data.coins = 50_000_000;
        }
        let car = data::car_at(0);
        let driver = crate::sim::ai::AiDriver::new(
            crate::sim::car::CarSim::new(
                car,
                glam::Vec3::ZERO,
                0.0,
                crate::sim::car::Upgrades::stock(),
            ),
            1.0,
            data::ai_config(false),
        );
        Some(Self {
            level,
            driver,
            started: false,
            ticks: 0,
            trace: std::env::var("DD_AUTOPLAY_TRACE")
                .map(|v| v != "0")
                .unwrap_or(true),
        })
    }
}

/// Placement id for the between-runs interstitial, as the bridge config names it.
const PLACEMENT_GAME_OVER: &str = "game_over";

/// Placement id for the rewarded-ad coin grant, as the bridge config names it.
const PLACEMENT_FREE_COINS: &str = "free_coins";

const SHORTCUT_PAUSE: &str = "deadly-descent.pause";
/// Coins the rewarded placement pays; the reference pays coin packs, the exact
/// grant is not in the build, so this matches the shop's small pack.
const REWARDED_COIN_GRANT: i32 = 500;

const SHORTCUT_RESTART: &str = "deadly-descent.restart";
const SHORTCUT_CONFIRM: &str = "deadly-descent.confirm";

fn ensure_fonts() {
    static ONCE: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    ONCE.get_or_init(|| {
        repose_text::register_font_data(FREDOKA_BOLD);
    });
}

pub struct App {
    pub(crate) sim: Sim,
    platform: PlatformHost,
    schedule: Schedule,
    staging: Rc<RefCell<Staging>>,
    shortcut_edges: SharedEdges,
    pub(crate) ui: SharedUi,
    geom: GeomHandle,
    audio: AudioDirector,
    held: HashSet<PhysicalKey>,
    /// Shop item waiting on the platform's purchase result.
    pending_purchase: Option<String>,
    /// Edge state for the camera key.
    camera_held: bool,
    /// Development autoplay driver, when `DD_AUTOPLAY` is set.
    autoplay: Option<Autoplay>,
    /// Run whose end-of-race ads and score post have already been handled.
    reported_run: bool,
    interstitial_shown_for: Option<usize>,
}

impl App {
    pub fn new() -> Self {
        Self::with_host(PlatformHost::offline())
    }

    pub fn with_bridge(bridge: playgama_bridge::Bridge) -> Self {
        platform::watch_foreground();
        Self::with_host(PlatformHost::web(bridge))
    }

    fn with_host(host: PlatformHost) -> Self {
        ensure_fonts();
        let _ = &host;
        let mut sim = Sim::with_default_step();
        crate::sim::subscribe(&mut sim.world);
        sim.world.init_resource::<crate::scene::CameraChoice>();
        let muted = sim.world.resource::<Progress>().data.muted;
        sim.world.init_resource::<UiActions>();
        let autoplay = Autoplay::from_env(&mut sim);
        let ui = SharedUi {
            muted,
            viewport: Vec2::new(ui::DESIGN[0], ui::DESIGN[1]),
            ..SharedUi::default()
        };
        // The image skin binds on the first frame, when a render context exists.
        // (sync below, once the context is available in `view`.)
        Self {
            sim,
            platform: host,
            schedule: crate::sim::build_schedule(),
            staging: Staging::shared(),
            shortcut_edges: shared_edges(),
            ui,
            geom: GeomHandle::new(),
            audio: AudioDirector::new(muted),
            held: HashSet::new(),
            pending_purchase: None,
            camera_held: false,
            autoplay,
            reported_run: false,
            interstitial_shown_for: None,
        }
    }

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
        let viewport = prepare_viewport(sched);
        self.ui.viewport = Vec2::new(viewport[0], viewport[1]);
        let (pause, restart, confirm) = take_shortcut_edges(&self.shortcut_edges);
        if restart {
            self.sim
                .world
                .resource_mut::<Intents>()
                .0
                .push(Intent::Restart);
        }
        if confirm {
            self.start_selected_level();
        }
        if pause {
            self.ui.paused = !self.ui.paused;
        }
        self.feed_input();
        // Autoplay drives last: it owns `Controls` when it is on.
        self.autoplay_drive();
        self.platform
            .update(&mut self.sim.world, dt.as_secs_f32(), true);
        if !self.platform.suspends_gameplay() {
            for action in drain_actions(&mut self.sim.world) {
                self.apply_action(action);
            }
            crate::sim::apply_intents(&mut self.sim.world);
        }
        let steps = self.advance(dt);
        let _ = steps;
        self.collect_rewards();
        self.drain_purchases();
        self.handle_run_end();
        self.ui.elapsed += dt.as_secs_f32();
        self.advance_tutorial();
        self.sync_audio(dt);
        sync_shared_ui(&mut self.sim.world, &mut self.ui, ctx);
        if let Some(mut platform) = self
            .sim
            .world
            .get_resource_mut::<crate::sim::platform::Platform>()
            && platform.take_score_posted()
        {
            self.ui.leaderboard_posted = true;
        }
        self.compose(ctx, viewport)
    }

    /// Walk the tutorial forward as its steps are performed: RACE, then a map,
    /// then a nitro in the first race.
    fn advance_tutorial(&mut self) {
        let stage = self.ui.tutorial_stage;
        let phase = self.ui.phase;
        let nitro_used = self
            .sim
            .world
            .get_resource::<crate::sim::Controls>()
            .map(|c| c.0.nitro)
            .unwrap_or(false);
        let done = match stage {
            0 => matches!(phase, Phase::MapSelect),
            1 => matches!(phase, Phase::Countdown | Phase::Racing),
            2 => nitro_used,
            _ => false,
        };
        if done {
            if let Some(mut progress) = self.sim.world.get_resource_mut::<Progress>() {
                progress.advance_tutorial(stage);
            }
            self.ui.tutorial_stage = stage + 1;
        }
    }

    /// Apply a purchase the platform reported, then clear the slot.
    fn drain_purchases(&mut self) {
        let purchased = self
            .sim
            .world
            .get_resource_mut::<crate::sim::platform::Platform>()
            .and_then(|mut platform| platform.take_purchase());
        let Some(id) = purchased else {
            return;
        };
        let wanted = self.pending_purchase.clone();
        self.pending_purchase = None;
        // Offline hosts report a generic id; treat the in-flight request as what
        // came back.
        let id = if id == "purchase" {
            wanted.unwrap_or(id)
        } else {
            id
        };
        if let Some(mut progress) = self.sim.world.get_resource_mut::<Progress>() {
            if id == "no_ads" {
                progress.set_no_ads(true);
            } else if let Some(coins) = Progress::pack_coins(&id) {
                progress.grant_coins(coins);
            }
        }
    }

    /// At the end of a run: post the score once, and show the platform's
    /// interstitial once when ads are not removed.
    fn handle_run_end(&mut self) {
        let (phase, level, score, finished) = self
            .sim
            .world
            .get_resource::<crate::sim::race::Session>()
            .map(|s| {
                (
                    s.phase,
                    s.level,
                    s.results.map(|r| r.score).unwrap_or(0),
                    matches!(s.phase, Phase::Finished),
                )
            })
            .unwrap_or((Phase::Garage, 0, 0, false));
        if !matches!(phase, Phase::Finished | Phase::Failed) {
            self.reported_run = false;
            return;
        }
        if self.reported_run {
            return;
        }
        self.reported_run = true;
        let no_ads = self
            .sim
            .world
            .get_resource::<Progress>()
            .map(|p| p.data.no_ads)
            .unwrap_or(false);
        let id = format!("level_{}", level);
        self.ui.leaderboard_id = id.clone();
        let mut platform = match self
            .sim
            .world
            .get_resource_mut::<crate::sim::platform::Platform>()
        {
            Some(platform) => platform,
            None => return,
        };
        if finished {
            platform.submit_score(id.as_str(), score as f64);
        }
        if !no_ads && self.interstitial_shown_for != Some(level) {
            self.interstitial_shown_for = Some(level);
            platform.commercial_break(PLACEMENT_GAME_OVER);
        }
    }

    /// A rewarded ad that finished grants coins once, as the reference's
    /// "Get Coins" flow does.
    fn collect_rewards(&mut self) {
        let granted = self
            .sim
            .world
            .get_resource::<crate::sim::platform::Platform>()
            .map(|p| p.reward_granted() && p.pending().is_none())
            .unwrap_or(false);
        if !granted {
            return;
        }
        if let Some(mut progress) = self.sim.world.get_resource_mut::<Progress>() {
            progress.add_coins(REWARDED_COIN_GRANT);
            progress.persist();
        }
        if let Some(mut platform) = self
            .sim
            .world
            .get_resource_mut::<crate::sim::platform::Platform>()
        {
            platform.break_finished();
        }
        self.ui.reward_pending = false;
    }

    fn start_selected_level(&mut self) {
        let level = self.ui.level;
        self.sim
            .world
            .resource_mut::<Intents>()
            .0
            .push(Intent::StartLevel(level));
    }

    pub(crate) fn feed_input(&mut self) {
        let steering = i32::from(self.key(PhysicalKey::KeyD) || self.key(PhysicalKey::ArrowRight))
            - i32::from(self.key(PhysicalKey::KeyA) || self.key(PhysicalKey::ArrowLeft));
        let throttle = i32::from(self.key(PhysicalKey::KeyW) || self.key(PhysicalKey::ArrowUp));
        let brake = i32::from(self.key(PhysicalKey::KeyS) || self.key(PhysicalKey::ArrowDown));
        let handbrake = self.key(PhysicalKey::Space);
        let nitro = self.key(PhysicalKey::ShiftLeft) || self.key(PhysicalKey::KeyN);
        // The shipped control set: C changes the camera view.
        if self.key(PhysicalKey::KeyC) && !self.camera_held {
            self.camera_held = true;
            let mut choice = self.sim.world.resource_mut::<crate::scene::CameraChoice>();
            choice.mode = match choice.mode {
                crate::scene::CameraMode::Chase => crate::scene::CameraMode::Cockpit,
                crate::scene::CameraMode::Cockpit => crate::scene::CameraMode::Chase,
            };
        } else if !self.key(PhysicalKey::KeyC) {
            self.camera_held = false;
        }
        let live = self
            .sim
            .world
            .get_resource::<crate::sim::race::Session>()
            .map(|s| matches!(s.phase, Phase::Countdown | Phase::Racing))
            .unwrap_or(false);
        let paused = self.ui.paused;
        // Touch (or a mouse driving the same HUD) merges with the keyboard: each
        // axis takes whichever source is asking for more.
        let touch = {
            let mut state = self.ui.touch.borrow_mut();
            state.step(1.0 / 60.0);
            (
                state.steer_input(),
                state.throttle,
                state.brake,
                state.handbrake,
                state.nitro,
                state.visible,
            )
        };
        let (touch_steer, touch_throttle, touch_brake, touch_hand, touch_nitro, touch_visible) =
            touch;
        let keyboard_steer = steering as f32;
        self.sim.world.resource_mut::<crate::sim::Controls>().0 = if live && !paused {
            Input {
                throttle: (throttle as f32).max(touch_throttle),
                brake: (brake as f32).max(touch_brake),
                steer: if keyboard_steer != 0.0 {
                    keyboard_steer
                } else if touch_visible {
                    touch_steer
                } else {
                    0.0
                },
                handbrake: handbrake || (touch_visible && touch_hand),
                nitro: nitro || (touch_visible && touch_nitro),
            }
        } else {
            Input::default()
        };
    }

    /// Start the configured level, then steer with the bot controller and report
    /// a once-a-second trace of the run.
    pub(crate) fn autoplay_drive(&mut self) {
        let Some(autoplay) = self.autoplay.as_mut() else {
            return;
        };
        if !autoplay.started {
            autoplay.started = true;
            let level = autoplay.level;
            self.sim
                .world
                .resource_mut::<Intents>()
                .0
                .push(Intent::StartLevel(level));
            self.ui.paused = false;
            return;
        }
        let Some(session) = self.sim.world.get_resource::<crate::sim::race::Session>() else {
            return;
        };
        if !matches!(session.phase, Phase::Racing) {
            return;
        }
        let level_max = session.max_bot_speed();
        // The driver must see the car it is driving: mirror the player's state
        // into the controller before asking for inputs.
        autoplay.driver.sim = session.player.clone();
        let controls =
            self.sim
                .world
                .resource_scope(|_world, tw: Mut<crate::sim::race::TrackWorld>| {
                    autoplay.driver.control(&tw, level_max, 1.0 / 60.0)
                });
        self.sim.world.resource_mut::<crate::sim::Controls>().0 = controls;
        autoplay.ticks += 1;
        if autoplay.trace && autoplay.ticks % 60 == 0 {
            let session = self.sim.world.resource::<crate::sim::race::Session>();
            let p = &session.player;
            let target = autoplay.driver.target_speed_for_trace();
            eprintln!(
                "autoplay t={:.0}s s={:.0}m v={:.0}km/h target={:.0}km/h offroad={:.1} dmg={:.0}/{:.0} gear={} rpm={:.0} phase={:?}",
                session.time,
                p.pos.z,
                p.speed() * 3.6,
                target * 3.6,
                p.off_road,
                p.damage,
                p.max_damage(),
                p.gear + 1,
                p.rpm,
                session.phase
            );
        }
    }

    fn key(&self, key: PhysicalKey) -> bool {
        self.held.contains(&key)
    }

    fn apply_action(&mut self, action: UiAction) {
        let mut intents = self.sim.world.resource_mut::<Intents>();
        match action {
            UiAction::StartLevel(index) => {
                intents.0.push(Intent::StartLevel(index));
            }
            UiAction::SelectCar(index) => intents.0.push(Intent::SelectCar(index)),
            UiAction::BuyCar(index) => intents.0.push(Intent::BuyCar(index)),
            UiAction::UpgradeEngine => intents
                .0
                .push(Intent::Upgrade(crate::sim::meta::UpgradeKind::Engine)),
            UiAction::UpgradeSteer => intents
                .0
                .push(Intent::Upgrade(crate::sim::meta::UpgradeKind::Steer)),
            UiAction::UpgradeDurability => intents
                .0
                .push(Intent::Upgrade(crate::sim::meta::UpgradeKind::Durability)),
            UiAction::ToGarage => {
                intents.0.push(Intent::ToGarage);
                self.ui.paused = false;
            }
            UiAction::ToMapSelect => {
                intents.0.push(Intent::ToMapSelect);
                self.ui.paused = false;
            }
            UiAction::Restart => {
                intents.0.push(Intent::Restart);
                self.ui.paused = false;
            }
            UiAction::NextLevel => {
                let last = crate::sim::data::levels().len().saturating_sub(1);
                let next = (self.ui.level + 1).min(last);
                intents.0.push(Intent::StartLevel(next));
                self.ui.paused = false;
            }
            UiAction::ToggleMute => intents.0.push(Intent::ToggleMute),
            UiAction::ClaimLogin => {
                let today = ui::today_index();
                if let Some(mut progress) = self.sim.world.get_resource_mut::<Progress>() {
                    progress.claim_login_reward(today);
                }
            }
            UiAction::BuyPack(index) => {
                let id = crate::sim::meta::COIN_PACKS
                    .get(index)
                    .map(|(id, _)| *id)
                    .unwrap_or_default();
                self.pending_purchase = Some(id.to_string());
                if let Some(mut platform) = self
                    .sim
                    .world
                    .get_resource_mut::<crate::sim::platform::Platform>()
                {
                    platform.purchase(id);
                }
            }
            UiAction::BuyNoAds => {
                self.pending_purchase = Some("no_ads".to_string());
                if let Some(mut platform) = self
                    .sim
                    .world
                    .get_resource_mut::<crate::sim::platform::Platform>()
                {
                    platform.purchase("no_ads");
                }
            }
            UiAction::ShowLeaderboard => {
                let id = self.ui.leaderboard_id.clone();
                if let Some(mut platform) = self
                    .sim
                    .world
                    .get_resource_mut::<crate::sim::platform::Platform>()
                {
                    platform.show_leaderboard(id.as_str());
                }
            }
            UiAction::RewardedCoins => {
                let started = self
                    .sim
                    .world
                    .resource_mut::<crate::sim::platform::Platform>()
                    .rewarded_break(PLACEMENT_FREE_COINS);
                self.ui.reward_pending = started;
            }
            UiAction::ToggleTouchPad => {
                let mut touch = self.ui.touch.borrow_mut();
                touch.visible = !touch.visible;
                if !touch.visible {
                    touch.release_all();
                }
            }
            UiAction::TogglePause => self.ui.paused = !self.ui.paused,
            UiAction::Resume => self.ui.paused = false,
        }
    }

    pub(crate) fn advance(&mut self, dt: Duration) -> u32 {
        let live = self
            .sim
            .world
            .get_resource::<crate::sim::race::Session>()
            .map(|s| !matches!(s.phase, Phase::Garage | Phase::MapSelect))
            .unwrap_or(false);
        let paused = self.ui.paused;
        let suspended = self.platform.suspends_gameplay();
        self.sim.time_scale = if paused || suspended { 0.0 } else { 1.0 };
        let schedule = &mut self.schedule;
        self.sim.step_with(dt, |world| {
            if live {
                schedule.run(world);
            }
        })
    }

    pub(crate) fn sync_audio(&mut self, dt: Duration) {
        let muted = self.sim.world.resource::<Progress>().data.muted || self.platform.silenced();
        if self.audio.muted() != muted {
            self.audio.set_muted(muted);
        }
        if let Some(cue) = crate::sim::take_cue(&mut self.sim.world) {
            self.audio.play(cue);
        }
        let selected = self
            .sim
            .world
            .get_resource::<Progress>()
            .map(|p| p.selected_car())
            .unwrap_or(0);
        let engine = data::engine_dir(data::car_at(selected as i32).id.as_str());
        self.audio.set_engine(engine);
        let (rpm_frac, speed_kmh) = self
            .sim
            .world
            .get_resource::<crate::sim::race::Session>()
            .map(|s| {
                (
                    s.player.rpm / s.player.spec.max_rpm.max(1.0),
                    s.player.top_speed.max(s.player.speed() * 3.6),
                )
            })
            .unwrap_or((0.0, 0.0));
        let throttle = self.sim.world.resource::<crate::sim::Controls>().0.throttle;
        self.audio.engine(rpm_frac, throttle, speed_kmh);
        self.audio.update(dt.as_secs_f32());
    }

    pub(crate) fn compose(&mut self, ctx: &RenderContext, viewport: [f32; 2]) -> View {
        let scene = scene::frame_for(&mut self.sim.world, ctx, viewport, &self.geom);
        let actions = self
            .sim
            .world
            .get_resource::<UiActions>()
            .cloned()
            .unwrap_or_default();
        let sink: Sink = actions.0.clone();
        let root = ui::compose_root(&self.ui, sink);
        let focus = repose_core::remember(FocusRequester::new);
        let focus_trigger = (*focus).clone();
        let staging = self.staging.clone();
        let key_staging = self.staging.clone();
        ZStack(
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
        .child((scene, root))
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

fn prepare_viewport(sched: &Scheduler) -> [f32; 2] {
    let density = repose_core::locals::effective_density_scale().max(1e-6);
    if sched.size.0 == 0 || sched.size.1 == 0 {
        ui::DESIGN
    } else {
        [sched.size.0 as f32 / density, sched.size.1 as f32 / density]
    }
}

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

#[cfg(not(target_arch = "wasm32"))]
pub fn run_desktop() -> anyhow::Result<()> {
    let _ = env_logger::try_init();
    // Offscreen capture: no window, no compositor, PNGs on disk.
    let mut app = App::new();
    if let Some(request) = crate::shot::CaptureRequest::from_env() {
        return app.run_capture(&request);
    }
    let mut runtime = AppRuntime {
        app,
        previous: Instant::now(),
    };
    repame_shell::run_desktop("Deadly Descent", (1280, 720), move |sched, ctx| {
        runtime.view(sched, ctx)
    })
}

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
