//! Run state machine and the demo game. Replace the demo, keep the phase
//! machine and the ad calls: that is the template's contract.

use bevy_ecs::prelude::*;
use glam::Vec2;

use super::platform::Platform;
use super::save::SaveGame;

/// The fixed design view, in design units. The contain-fit letterboxes it
/// into any window and the UI grid scales off the same numbers.
pub const WORLD: [f32; 2] = [960.0, 540.0];
pub const RUN_SECONDS: f32 = 60.0;
/// What a rewarded revive pays back.
pub const REVIVE_SECONDS: f32 = 15.0;
pub const PLAYER_R: f32 = 17.0;
pub const PLAYER_SPEED: f32 = 360.0;
pub const ORB_R: f32 = 9.0;
pub const ORB_COUNT: usize = 10;
/// Spawn margin so an orb never straddles the wall the player is clamped to.
pub const SPAWN_MARGIN: f32 = 48.0;
/// Score popup lifetime, seconds.
pub const POP_LIFE: f32 = 0.7;
/// Pickup burst ring lifetime, seconds.
pub const BURST_LIFE: f32 = 0.32;
/// The revival is offered once per run.
pub const MAX_REVIVES: i32 = 1;

/// Interstitial placement ids, the keys `playgama-bridge-config.json` maps.
pub const PLACEMENT_GAME_OVER: &str = "game_over";
pub const PLACEMENT_REVIVE: &str = "revive";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// The menu owns the screen; `panel` says which one.
    Menu,
    Playing,
    /// An ad is up. `then` is where to go once it ends, `t` the watchdog.
    Break(BreakThen),
}

/// Where a finished break lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BreakThen {
    GameOver,
    Revive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    Main,
    Death,
    /// Run in progress: no panel.
    None,
}

#[derive(Resource, Clone, Debug)]
pub struct RunState {
    pub phase: Phase,
    pub panel: Panel,
    pub paused: bool,
    /// Counts down while `Phase::Playing`.
    pub run_time_left: f32,
    pub score: i32,
    pub best: i32,
    /// The run that just ended beat the stored best.
    pub record: bool,
    pub revives_left: i32,
    /// A wipe is mid-flight.
    pub transition: bool,
    /// A rewarded ad is in flight: the button is gone until it answers.
    pub revive_pending: bool,
    /// Break watchdog left, in seconds.
    pub break_t: f32,
}

impl Default for RunState {
    fn default() -> Self {
        Self {
            phase: Phase::Menu,
            panel: Panel::Main,
            paused: false,
            run_time_left: RUN_SECONDS,
            score: 0,
            best: 0,
            record: false,
            revives_left: MAX_REVIVES,
            transition: false,
            revive_pending: false,
            break_t: 0.0,
        }
    }
}

impl RunState {
    pub fn playing(&self) -> bool {
        self.phase == Phase::Playing && !self.paused
    }

    pub fn transitional(&self) -> bool {
        self.modal() || self.transition
    }

    /// Something modal covers the run but is not the player's own pause.
    pub fn modal(&self) -> bool {
        matches!(self.phase, Phase::Break(_)) || self.revive_pending
    }
}

#[derive(Component, Clone, Copy, Debug)]
pub struct PlayerBody {
    pub pos: Vec2,
}

#[derive(Component, Clone, Copy, Debug)]
pub struct Orb {
    pub pos: Vec2,
    /// Bob phase, radians; keeps a still field from looking dead.
    pub bob: f32,
}

/// Live input, fed by the shell from the keyboard and the touch stick.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct InputState {
    pub dir: Vec2,
}

/// Scene changes run through a gold-disc wipe, the way portals present a level
/// change: a flat gold circle grows in from the left, holds, then shrinks back
/// from the centre to uncover the incoming scene. While it runs the schedule
/// is gated and input is dropped.
#[derive(Resource, Clone, Debug, Default)]
pub struct Transition {
    pub phase: TransitionPhase,
    pub t: f32,
    /// What the hold applies: the action the player asked for.
    pub pending: Option<PendingAction>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TransitionPhase {
    #[default]
    None,
    Filling,
    Holding,
    Revealing,
}

/// The scene change a wipe carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendingAction {
    Start,
    Menu,
}

/// The wipe timings, in seconds.
pub const FILL_TIME: f32 = 0.22;
pub const HOLD_TIME: f32 = 0.08;
pub const REVEAL_TIME: f32 = 0.35;

/// Radius that covers the design view from any phase centre, in design units.
pub const IRIS_MAX_R: f32 = 760.0;
/// Where the disc grows from, as a fraction of the design view.
pub const IRIS_IN_X: f32 = 0.38;

impl Transition {
    pub fn running(&self) -> bool {
        self.phase != TransitionPhase::None
    }

    /// The disc as the overlay draws it: centre and radius in design units,
    /// or `None` once the wipe is done and nothing is drawn.
    pub fn disc(&self) -> Option<([f32; 2], f32)> {
        let max = IRIS_MAX_R;
        match self.phase {
            TransitionPhase::None => None,
            TransitionPhase::Filling => {
                let t = (self.t / FILL_TIME).clamp(0.0, 1.0);
                Some(([WORLD[0] * IRIS_IN_X, WORLD[1] * 0.5], max * t))
            }
            TransitionPhase::Holding => Some(([WORLD[0] * 0.5, WORLD[1] * 0.5], max)),
            TransitionPhase::Revealing => {
                let t = (self.t / REVEAL_TIME).clamp(0.0, 1.0);
                Some(([WORLD[0] * 0.5, WORLD[1] * 0.5], max * (1.0 - t)))
            }
        }
    }
}

/// Start a wipe carrying `action`. The action applies at the hold, so the
/// scene swaps while the disc covers the screen.
pub fn begin(world: &mut World, action: PendingAction) {
    let mut transition = world.resource_mut::<Transition>();
    transition.phase = TransitionPhase::Filling;
    transition.t = 0.0;
    transition.pending = Some(action);
}

/// Advance the wipe and apply the action at the hold.
pub fn transition_system(world: &mut World) {
    let phase = world.resource::<Transition>().phase;
    if phase == TransitionPhase::None {
        return;
    }
    let dt = world.resource::<repame_sim::SimTime>().delta_secs;
    // Advance and re-phase in one short borrow, then step out before touching
    // the rest of the world.
    let action = {
        let mut transition = world.resource_mut::<Transition>();
        transition.t += dt;
        let mut finished = false;
        match phase {
            TransitionPhase::Filling if transition.t >= FILL_TIME => {
                transition.phase = TransitionPhase::Holding;
                transition.t = 0.0;
            }
            TransitionPhase::Holding if transition.t >= HOLD_TIME => {
                transition.phase = TransitionPhase::Revealing;
                transition.t = 0.0;
            }
            TransitionPhase::Revealing if transition.t >= REVEAL_TIME => {
                transition.phase = TransitionPhase::None;
                transition.t = 0.0;
                finished = true;
            }
            _ => {}
        }
        if finished {
            transition.pending.take()
        } else {
            None
        }
    };
    let running = world.resource::<Transition>().running();
    world.resource_mut::<RunState>().transition = running;
    match action {
        Some(PendingAction::Start) => start_run(world),
        Some(PendingAction::Menu) => to_menu(world),
        None => {}
    }
}

/// One-frame actions queued by UI buttons, menus and the game shortcuts.
/// Applied once per frame, outside the pause gate, like the Godot original's
/// signal callbacks.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct Intents {
    pub play: bool,
    pub retry: bool,
    pub revive: bool,
    pub menu: bool,
    pub toggle_pause: bool,
    pub toggle_mute: bool,
}

/// Score popups and pickup bursts. The burst is the cheap juice: an ink ring
/// that expands over `BURST_LIFE` where the coin was.
#[derive(Resource, Clone, Debug, Default)]
pub struct Pops {
    pub list: Vec<(Vec2, f32)>,
    pub bursts: Vec<(Vec2, f32)>,
}

/// Sounds the sim asked for, drained by the app's audio director.
#[derive(Resource, Clone, Debug, Default)]
pub struct SfxQueue {
    pub events: Vec<SfxCue>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SfxCue {
    Pickup,
    Ui,
    Death,
    Revive,
}

/// Menu → run: reset everything the run touches.
pub fn start_run(world: &mut World) {
    {
        let mut run = world.resource_mut::<RunState>();
        run.phase = Phase::Playing;
        run.panel = Panel::None;
        run.paused = false;
        run.run_time_left = RUN_SECONDS;
        run.score = 0;
        run.record = false;
        run.revives_left = MAX_REVIVES;
        run.revive_pending = false;
    }
    world.resource_mut::<Pops>().list.clear();
    world.resource_mut::<Platform>().gameplay_start();
    respawn_orbs(world);
    queue_sfx(world, SfxCue::Ui);
}

/// Run ends (the clock ran out): bank the score, then show the interstitial.
pub fn end_run(world: &mut World) {
    let score = world.resource::<RunState>().score;
    let best = {
        let mut save = world.resource_mut::<SaveGame>();
        save.data.best_score = save.data.best_score.max(score);
        save.save_data();
        save.data.best_score
    };
    {
        let mut run = world.resource_mut::<RunState>();
        run.record = score > 0 && score >= best;
        run.best = best;
        run.paused = false;
        run.revive_pending = false;
    }
    // A break is already up only if one was requested this frame; the run
    // clock cannot fire during a break, so this is the first request.
    world
        .resource_mut::<Platform>()
        .commercial_break(PLACEMENT_GAME_OVER);
    let mut run = world.resource_mut::<RunState>();
    run.phase = Phase::Break(BreakThen::GameOver);
    run.break_t = super::platform::BREAK_TIMEOUT;
}

/// The revive button: a rewarded break buys back the clock.
pub fn request_revive(world: &mut World) {
    let run = world.resource::<RunState>();
    if run.phase != Phase::Menu
        || run.panel != Panel::Death
        || run.revive_pending
        || run.revives_left <= 0
    {
        return;
    }
    if !world
        .resource_mut::<Platform>()
        .rewarded_break(PLACEMENT_REVIVE)
    {
        return;
    }
    let mut run = world.resource_mut::<RunState>();
    run.revive_pending = true;
    run.phase = Phase::Break(BreakThen::Revive);
    run.break_t = super::platform::REWARD_TIMEOUT;
}

/// The rewarded ad paid out: keep the run going.
fn apply_revive(world: &mut World) {
    world.resource_mut::<Platform>().gameplay_start();
    {
        let mut run = world.resource_mut::<RunState>();
        run.phase = Phase::Playing;
        run.panel = Panel::None;
        run.run_time_left = REVIVE_SECONDS;
        run.revives_left -= 1;
        run.revive_pending = false;
    }
    respawn_orbs(world);
    queue_sfx(world, SfxCue::Revive);
}

/// The ad is over and paid nothing: stay dead, one revive gone.
fn deny_revive(world: &mut World) {
    let mut run = world.resource_mut::<RunState>();
    run.phase = Phase::Menu;
    run.panel = Panel::Death;
    run.revives_left = 0;
    run.revive_pending = false;
}

pub fn to_menu(world: &mut World) {
    world.resource_mut::<Platform>().gameplay_stop();
    let mut run = world.resource_mut::<RunState>();
    run.phase = Phase::Menu;
    run.panel = Panel::Main;
    run.paused = false;
    run.revive_pending = false;
}

fn to_death_panel(world: &mut World) {
    world.resource_mut::<Platform>().gameplay_stop();
    world.resource_mut::<Pops>().list.clear();
    let mut run = world.resource_mut::<RunState>();
    run.phase = Phase::Menu;
    run.panel = Panel::Death;
    run.paused = false;
    run.revive_pending = false;
}

/// Every one-shot action the UI and the shortcuts queued. Runs once per
/// frame from the shell, never while an ad is up.
pub fn apply_intents(world: &mut World) {
    let intents = *world.resource::<Intents>();
    if intents == Intents::default() {
        return;
    }
    *world.resource_mut::<Intents>() = Intents::default();
    let phase = world.resource::<RunState>().phase;
    let panel = world.resource::<RunState>().panel;
    if intents.toggle_pause && phase == Phase::Playing && !world.resource::<RunState>().modal() {
        world.resource_mut::<RunState>().paused ^= true;
        queue_sfx(world, SfxCue::Ui);
    }
    if intents.toggle_mute {
        let muted = {
            let mut save = world.resource_mut::<SaveGame>();
            save.data.muted = !save.data.muted;
            save.save_data();
            save.data.muted
        };
        log::info!("web-game: muted {muted}");
    }
    let transitioning = world.resource::<Transition>().running();
    if intents.play && phase == Phase::Menu && panel != Panel::None && !transitioning {
        begin(world, PendingAction::Start);
    }
    if intents.retry && phase == Phase::Menu && !transitioning {
        begin(world, PendingAction::Start);
    }
    if intents.revive && !transitioning {
        request_revive(world);
    }
    if intents.menu && (phase != Phase::Menu || panel == Panel::Death) && !transitioning {
        begin(world, PendingAction::Menu);
    }
}

/// The player entity, for tests and systems that need it directly.
pub fn player_entity(world: &mut World) -> Option<Entity> {
    let mut q = world.query_filtered::<Entity, With<PlayerBody>>();
    q.iter(world).next()
}

pub fn spawn_world(world: &mut World) {
    let mut rng = crate::sim::rng::Rng::new(0x5EED_1234);
    if let Some(entity) = player_entity(world) {
        world.entity_mut(entity).despawn();
    }
    for entity in orb_entities(world) {
        world.entity_mut(entity).despawn();
    }
    world.spawn(PlayerBody {
        pos: Vec2::new(WORLD[0] * 0.5, WORLD[1] * 0.5),
    });
    for i in 0..ORB_COUNT {
        let pos = free_spot(&mut rng, world);
        world.spawn(Orb {
            pos,
            bob: i as f32 * 0.7,
        });
    }
}

/// A spawn point no orb occupies yet.
fn free_spot(rng: &mut crate::sim::rng::Rng, world: &mut World) -> Vec2 {
    let mut existing = world
        .query::<&Orb>()
        .iter(world)
        .map(|orb| orb.pos)
        .collect::<Vec<_>>();
    for _ in 0..32 {
        let candidate = Vec2::new(
            rng.range(SPAWN_MARGIN, WORLD[0] - SPAWN_MARGIN),
            rng.range(SPAWN_MARGIN, WORLD[1] - SPAWN_MARGIN),
        );
        if existing
            .iter()
            .all(|pos| pos.distance(candidate) > ORB_R * 6.0)
        {
            return candidate;
        }
        existing.push(candidate);
    }
    Vec2::new(WORLD[0] * 0.5, WORLD[1] * 0.5)
}

pub fn respawn_orbs(world: &mut World) {
    for entity in orb_entities(world) {
        world.entity_mut(entity).despawn();
    }
    let mut rng =
        crate::sim::rng::Rng::new(0x5EED_1234 ^ world.resource::<RunState>().score as u64);
    for i in 0..ORB_COUNT {
        let pos = free_spot(&mut rng, world);
        world.spawn(Orb {
            pos,
            bob: i as f32 * 0.7,
        });
    }
}

fn orb_entities(world: &mut World) -> Vec<Entity> {
    let mut q = world.query_filtered::<Entity, With<Orb>>();
    q.iter(world).collect()
}

pub fn queue_sfx(world: &mut World, cue: SfxCue) {
    world.resource_mut::<SfxQueue>().events.push(cue);
}

/// The break watchdog and the ad verdict. Wall time, so an ad that never
/// reports back still ends the wait.
pub fn break_system(world: &mut World) {
    let wall = world.resource::<repame_sim::SimTime>().delta_secs;
    let (phase, break_t) = {
        let run = world.resource::<RunState>();
        (run.phase, run.break_t)
    };
    let Phase::Break(then) = phase else {
        return;
    };
    // `pending` clears when the ad closed, failed or its watchdog fired; the
    // reward verdict rides on `reward_granted`.
    let still_pending = world
        .get_resource::<Platform>()
        .and_then(Platform::pending)
        .is_some();
    if !still_pending {
        resolve_break(world, then);
        return;
    }
    if break_t - wall <= 0.0 {
        // The watchdog fired: the platform swallowed its own callbacks, so
        // end the break here. A hosted game must not freeze on a silent SDK.
        world.resource_mut::<Platform>().break_finished();
        resolve_break(world, then);
        return;
    }
    world.resource_mut::<RunState>().break_t = break_t - wall;
}

/// Land the break where `then` says, with the reward the platform reported.
fn resolve_break(world: &mut World, then: BreakThen) {
    let granted = world.resource::<Platform>().reward_granted();
    {
        let mut run = world.resource_mut::<RunState>();
        run.break_t = 0.0;
        run.revive_pending = false;
        run.phase = Phase::Menu;
        run.panel = Panel::Main;
        run.paused = false;
    }
    match then {
        BreakThen::GameOver => to_death_panel(world),
        BreakThen::Revive => {
            if granted {
                apply_revive(world);
            } else {
                deny_revive(world);
            }
        }
    }
}
