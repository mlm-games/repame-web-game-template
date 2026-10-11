
use bevy_ecs::prelude::*;
use glam::Vec2;

use super::platform::Platform;
use super::save::SaveGame;

pub const WORLD: [f32; 2] = [960.0, 540.0];
pub const RUN_SECONDS: f32 = 60.0;
pub const REVIVE_SECONDS: f32 = 15.0;
pub const PLAYER_R: f32 = 17.0;
pub const PLAYER_SPEED: f32 = 360.0;
pub const ORB_R: f32 = 9.0;
pub const ORB_COUNT: usize = 10;
pub const SPAWN_MARGIN: f32 = 48.0;
pub const POP_LIFE: f32 = 0.7;
pub const BURST_LIFE: f32 = 0.32;
pub const ORB_SPAWN_LIFE: f32 = 0.26;
pub const LOOK_LERP: f32 = 14.0;
pub const MAX_REVIVES: i32 = 1;

pub const PLACEMENT_GAME_OVER: &str = "game_over";
pub const PLACEMENT_REVIVE: &str = "revive";

#[derive(Resource, Clone, Copy, Debug)]
pub struct Field {
    pub width: f32,
    pub height: f32,
}

impl Field {
    pub fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Menu,
    Playing,
    Break(BreakThen),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BreakThen {
    GameOver,
    Revive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    Main,
    Death,
    None,
}

#[derive(Resource, Clone, Debug)]
pub struct RunState {
    pub phase: Phase,
    pub panel: Panel,
    pub paused: bool,
    pub run_time_left: f32,
    pub score: i32,
    pub best: i32,
    pub record: bool,
    pub revives_left: i32,
    pub transition: bool,
    pub revive_pending: bool,
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

    pub fn modal(&self) -> bool {
        matches!(self.phase, Phase::Break(_)) || self.revive_pending
    }
}

#[derive(Component, Clone, Copy, Debug)]
pub struct PlayerBody {
    pub pos: Vec2,
    pub look: Vec2,
}

#[derive(Component, Clone, Copy, Debug)]
pub struct Orb {
    pub pos: Vec2,
    pub bob: f32,
    pub age: f32,
}

#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct InputState {
    pub dir: Vec2,
}

#[derive(Resource, Clone, Debug, Default)]
pub struct Transition {
    pub phase: TransitionPhase,
    pub t: f32,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendingAction {
    Start,
    Menu,
}

pub const FILL_TIME: f32 = 0.22;
pub const HOLD_TIME: f32 = 0.08;
pub const REVEAL_TIME: f32 = 0.35;

impl Transition {
    pub fn running(&self) -> bool {
        self.phase != TransitionPhase::None
    }

    pub fn disc(&self) -> Option<f32> {
        match self.phase {
            TransitionPhase::None => None,
            TransitionPhase::Filling => Some((self.t / FILL_TIME).clamp(0.0, 1.0)),
            TransitionPhase::Holding => Some(1.0),
            TransitionPhase::Revealing => Some(1.0 - (self.t / REVEAL_TIME).clamp(0.0, 1.0)),
        }
    }
}

pub fn begin(world: &mut World, action: PendingAction) {
    {
        let mut transition = world.resource_mut::<Transition>();
        transition.phase = TransitionPhase::Filling;
        transition.t = 0.0;
        transition.pending = Some(action);
    }
    world.resource_mut::<RunState>().transition = true;
}

pub fn transition_system(world: &mut World) {
    let phase = world.resource::<Transition>().phase;
    if phase == TransitionPhase::None {
        return;
    }
    let dt = world.resource::<repame_sim::SimTime>().delta_secs;
    let action = {
        let mut transition = world.resource_mut::<Transition>();
        transition.t += dt;
        let mut covered = false;
        match phase {
            TransitionPhase::Filling if transition.t >= FILL_TIME => {
                transition.phase = TransitionPhase::Holding;
                transition.t = 0.0;
                covered = true;
            }
            TransitionPhase::Holding if transition.t >= HOLD_TIME => {
                transition.phase = TransitionPhase::Revealing;
                transition.t = 0.0;
            }
            TransitionPhase::Revealing if transition.t >= REVEAL_TIME => {
                transition.phase = TransitionPhase::None;
                transition.t = 0.0;
            }
            _ => {}
        }
        if covered {
            transition.pending.take()
        } else {
            None
        }
    };
    world.resource_mut::<RunState>().transition = world.resource::<Transition>().running();
    match action {
        Some(PendingAction::Start) => start_run(world),
        Some(PendingAction::Menu) => to_menu(world),
        None => {}
    }
}

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct Intents {
    pub play: bool,
    pub retry: bool,
    pub revive: bool,
    pub menu: bool,
    pub toggle_pause: bool,
    pub toggle_mute: bool,
}

#[derive(Resource, Clone, Debug, Default)]
pub struct Pops {
    pub list: Vec<(Vec2, f32)>,
    pub bursts: Vec<(Vec2, f32)>,
}

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
    world
        .resource_mut::<Platform>()
        .commercial_break(PLACEMENT_GAME_OVER);
    let mut run = world.resource_mut::<RunState>();
    run.phase = Phase::Break(BreakThen::GameOver);
    run.break_t = super::platform::BREAK_TIMEOUT;
}

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

pub fn player_entity(world: &mut World) -> Option<Entity> {
    let mut q = world.query_filtered::<Entity, With<PlayerBody>>();
    q.iter(world).next()
}

pub fn spawn_world(world: &mut World) {
    let field = *world.resource::<Field>();
    let mut rng = crate::sim::rng::Rng::new(0x5EED_1234);
    if let Some(entity) = player_entity(world) {
        world.entity_mut(entity).despawn();
    }
    for entity in orb_entities(world) {
        world.entity_mut(entity).despawn();
    }
    world.spawn(PlayerBody {
        pos: Vec2::new(field.width * 0.5, field.height * 0.5),
        look: Vec2::ZERO,
    });
    for i in 0..ORB_COUNT {
        let pos = free_spot(&mut rng, world);
        world.spawn(Orb {
            pos,
            bob: i as f32 * 0.7,
            age: 0.0,
        });
    }
}

fn free_spot(rng: &mut crate::sim::rng::Rng, world: &mut World) -> Vec2 {
    let field = world.resource::<Field>();
    let (fw, fh) = (field.width, field.height);
    let mut existing = world
        .query::<&Orb>()
        .iter(world)
        .map(|orb| orb.pos)
        .collect::<Vec<_>>();
    for _ in 0..32 {
        let candidate = Vec2::new(
            rng.range(SPAWN_MARGIN, fw - SPAWN_MARGIN),
            rng.range(SPAWN_MARGIN, fh - SPAWN_MARGIN),
        );
        if existing
            .iter()
            .all(|pos| pos.distance(candidate) > ORB_R * 6.0)
        {
            return candidate;
        }
        existing.push(candidate);
    }
    Vec2::new(fw * 0.5, fh * 0.5)
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
            age: 0.0,
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

pub fn break_system(world: &mut World) {
    let wall = world.resource::<repame_sim::SimTime>().delta_secs;
    let (phase, break_t) = {
        let run = world.resource::<RunState>();
        (run.phase, run.break_t)
    };
    let Phase::Break(then) = phase else {
        return;
    };
    let still_pending = world
        .get_resource::<Platform>()
        .and_then(Platform::pending)
        .is_some();
    if !still_pending {
        resolve_break(world, then);
        return;
    }
    if break_t - wall <= 0.0 {
        world.resource_mut::<Platform>().break_finished();
        resolve_break(world, then);
        return;
    }
    world.resource_mut::<RunState>().break_t = break_t - wall;
}

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

#[cfg(test)]
mod wipe_tests {
    use super::*;

    fn test_world() -> World {
        let mut world = World::new();
        world.init_resource::<Transition>();
        world.init_resource::<RunState>();
        world.init_resource::<Pops>();
        world.init_resource::<Platform>();
        world.init_resource::<SaveGame>();
        world.init_resource::<SfxQueue>();
        world.init_resource::<repame_sim::SimTime>();
        world.init_resource::<Intents>();
        world
    }

    fn advance(world: &mut World, secs: f32) {
        world.resource_mut::<repame_sim::SimTime>().delta_secs = secs;
        transition_system(world);
    }

    #[test]
    fn the_scene_swaps_on_the_tick_the_disc_closes() {
        let mut world = test_world();
        begin(&mut world, PendingAction::Start);
        assert!(world.resource::<RunState>().transitional());

        advance(&mut world, FILL_TIME * 0.5);
        assert_eq!(world.resource::<RunState>().phase, Phase::Menu);
        assert!(world.resource::<Transition>().running());

        advance(&mut world, FILL_TIME * 0.5);
        let transition = world.resource::<Transition>();
        assert_eq!(transition.phase, TransitionPhase::Holding);
        assert_eq!(transition.disc(), Some(1.0));

        advance(&mut world, HOLD_TIME);
        assert_eq!(world.resource::<RunState>().phase, Phase::Playing);
        assert!(world.resource::<Transition>().pending.is_none());

        advance(&mut world, REVEAL_TIME * 0.5);
        let mid = world.resource::<Transition>().disc().unwrap();
        assert!(
            mid > 0.0 && mid < 1.0,
            "disc should be half open, got {mid}"
        );

        advance(&mut world, REVEAL_TIME * 0.5);
        assert_eq!(world.resource::<Transition>().disc(), None);
        assert!(!world.resource::<RunState>().transitional());
    }

    #[test]
    fn the_swap_is_never_visible() {
        let mut world = test_world();
        world.resource_mut::<RunState>().phase = Phase::Playing;
        begin(&mut world, PendingAction::Menu);

        advance(&mut world, FILL_TIME * 0.9);
        assert_eq!(
            world.resource::<Transition>().phase,
            TransitionPhase::Filling
        );
        assert_eq!(world.resource::<RunState>().phase, Phase::Playing);

        advance(&mut world, FILL_TIME * 0.1);
        assert_eq!(
            world.resource::<Transition>().phase,
            TransitionPhase::Holding
        );
        assert_eq!(world.resource::<Transition>().disc(), Some(1.0));
        assert_eq!(world.resource::<RunState>().phase, Phase::Menu);
        assert_eq!(world.resource::<RunState>().panel, Panel::Main);
    }

    #[test]
    fn intents_wait_for_the_wipe_instead_of_restarting_it() {
        let mut world = test_world();
        begin(&mut world, PendingAction::Start);
        world.resource_mut::<Intents>().retry = true;
        apply_intents(&mut world);
        advance(&mut world, FILL_TIME);
        assert_eq!(world.resource::<RunState>().phase, Phase::Playing);
        assert_eq!(
            world.resource::<Transition>().phase,
            TransitionPhase::Holding
        );
    }
}
