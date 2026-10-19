//! The garage must be clickable: a press on RACE asks for the map screen, and
//! the map screen starts the map under the pointer. Both are checked on the
//! *full* frame the app builds, 3D viewport included, because a sibling that
//! covers the screen would swallow the click.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use deadly_descent::sim::car::Input;
use deadly_descent::sim::race::Phase;
use deadly_descent::sim::race::{Screen, Session};
use deadly_descent::sim::{Controls, Intent, Intents, apply_intents, subscribe};
use deadly_descent::ui::{self, SharedUi, Sink, UiAction};
use repame_sim::Sim;
use repame_view3d::GeomHandle;
use repose_core::RenderContext;
use repose_core::prelude::{Modifier, View};
use repose_ui::{Interactions, TextFieldState, ViewExt as _, ZStack, layout_and_paint};

const W: u32 = 1280;
const H: u32 = 720;

fn boot() -> (Sim, RenderContext, GeomHandle, SharedUi) {
    let mut sim = Sim::with_default_step();
    subscribe(&mut sim.world);
    sim.world.insert_resource(Controls(Input::default()));
    // One tick initialises the world's resources (session, track, garage state).
    let mut schedule = deadly_descent::sim::build_schedule();
    sim.step_with(std::time::Duration::from_millis(16), |world| {
        schedule.run(world);
    });
    let ctx = RenderContext::new();
    let mut ui = SharedUi {
        viewport: glam::Vec2::new(W as f32, H as f32),
        ..SharedUi::default()
    };
    ui::sync_shared_ui(&mut sim.world, &mut ui, &ctx);
    (sim, ctx, GeomHandle::new(), ui)
}

/// The frame the app composes: the 3D viewport under the UI.
fn app_frame(
    sim: &mut Sim,
    ctx: &RenderContext,
    geom: &GeomHandle,
    ui: &SharedUi,
    sink: Sink,
) -> View {
    let scene = deadly_descent::scene::frame_for(&mut sim.world, ctx, [W as f32, H as f32], geom);
    let root = ui::compose_root(ui, sink);
    ZStack(Modifier::new().fill_max_size()).child((scene, root))
}

fn layout(view: &View) -> (Vec<repose_core::HitRegion>, Vec<repose_core::SemNode>) {
    let states: HashMap<u64, Rc<RefCell<TextFieldState>>> = HashMap::new();
    let (_scene, hits, sems) =
        layout_and_paint(view, (W, H), &states, &Interactions::default(), None);
    (hits, sems)
}

/// The region a pointer at `point` lands on: deepest first, then topmost.
fn topmost(
    hits: &[repose_core::HitRegion],
    point: repose_core::Vec2,
) -> Option<&repose_core::HitRegion> {
    hits.iter()
        .filter(|hit| !hit.disabled && hit.rect.contains(point))
        .max_by_key(|hit| (hit.depth, hit.z_index as i32))
}

fn centre_of(rect: repose_core::Rect) -> repose_core::Vec2 {
    repose_core::Vec2 {
        x: rect.x + rect.w * 0.5,
        y: rect.y + rect.h * 0.5,
    }
}

#[test]
fn a_click_on_race_asks_for_the_map_screen() {
    let (mut sim, ctx, geom, mut ui) = boot();
    ui.phase = Phase::Garage;
    let sink: Sink = Arc::new(Mutex::new(Vec::new()));
    let view = app_frame(&mut sim, &ctx, &geom, &ui, sink.clone());
    let (hits, sems) = layout(&view);

    let race = sems
        .iter()
        .find(|node| node.label.as_deref() == Some("RACE"))
        .expect("the garage labels its RACE button");
    let hit = topmost(&hits, centre_of(race.rect)).expect("something is under the RACE label");
    let click = hit
        .on_click
        .as_ref()
        .expect("the region under RACE carries a click");
    click();

    let actions = sink.lock().unwrap().clone();
    assert!(
        actions.contains(&UiAction::ToMapSelect),
        "RACE should ask for the map screen, got {actions:?}"
    );
}

#[test]
fn the_map_screen_starts_the_map_under_the_pointer() {
    let (mut sim, ctx, geom, mut ui) = boot();
    ui.phase = Phase::MapSelect;
    let sink: Sink = Arc::new(Mutex::new(Vec::new()));
    let view = app_frame(&mut sim, &ctx, &geom, &ui, sink.clone());
    let (hits, sems) = layout(&view);

    let first = ui.levels.first().expect("the map list is filled").clone();
    let label = first.name.to_uppercase();
    let cell = sems
        .iter()
        .find(|node| node.label.as_deref() == Some(label.as_str()))
        .unwrap_or_else(|| panic!("map select shows a card for {label}"));
    let hit = topmost(&hits, centre_of(cell.rect)).expect("something is under the map card");
    let click = hit
        .on_click
        .as_ref()
        .expect("the region under a map card carries a click");
    click();

    let actions = sink.lock().unwrap().clone();
    assert!(
        actions.iter().any(|a| matches!(a, UiAction::StartLevel(_))),
        "a map card should start its map, got {actions:?}"
    );
}

#[test]
fn race_intent_reaches_the_map_screen_before_any_level_is_loaded() {
    // The garage is the first screen, so there is no `Session` yet: the phase has
    // to live on its own or the RACE click is dropped and nothing happens.
    let (mut sim, _ctx, _geom, _ui) = boot();
    assert!(
        sim.world.get_resource::<Session>().is_none(),
        "the garage runs before any level is loaded"
    );
    sim.world
        .resource_mut::<Intents>()
        .0
        .push(Intent::ToMapSelect);
    apply_intents(&mut sim.world);
    let screen = sim.world.resource::<Screen>().0;
    assert_eq!(screen, Phase::MapSelect, "RACE must leave the garage");
}

#[test]
fn starting_a_level_puts_the_screen_into_its_countdown() {
    let (mut sim, _ctx, _geom, _ui) = boot();
    sim.world
        .resource_mut::<Intents>()
        .0
        .push(Intent::StartLevel(0));
    apply_intents(&mut sim.world);
    let session_phase = sim.world.resource::<Session>().phase;
    let screen = sim.world.resource::<Screen>().0;
    assert_eq!(session_phase, Phase::Countdown);
    assert_eq!(screen, session_phase, "the screen follows the new session");
}

#[test]
fn clicking_race_lands_on_the_map_screen_at_the_next_frame() {
    // The whole journey the player takes: a click on RACE, the action drained into
    // an intent, the sim applying it, and the next frame showing map select.
    let (mut sim, ctx, geom, mut ui) = boot();
    let sink: Sink = Arc::new(Mutex::new(Vec::new()));

    let view = app_frame(&mut sim, &ctx, &geom, &ui, sink.clone());
    let (hits, sems) = layout(&view);
    let race = sems
        .iter()
        .find(|node| node.label.as_deref() == Some("RACE"))
        .expect("the garage labels its RACE button");
    topmost(&hits, centre_of(race.rect))
        .expect("something is under RACE")
        .on_click
        .as_ref()
        .expect("RACE carries a click")();

    for action in sink.lock().unwrap().drain(..) {
        match action {
            UiAction::ToMapSelect => sim
                .world
                .resource_mut::<Intents>()
                .0
                .push(Intent::ToMapSelect),
            other => panic!("RACE only asks for the map screen, got {other:?}"),
        }
    }
    apply_intents(&mut sim.world);
    let mut schedule = deadly_descent::sim::build_schedule();
    sim.step_with(std::time::Duration::from_millis(16), |world| {
        schedule.run(world);
    });
    ui::sync_shared_ui(&mut sim.world, &mut ui, &ctx);
    assert_eq!(ui.phase, Phase::MapSelect, "the click leaves the garage");

    let view = app_frame(&mut sim, &ctx, &geom, &ui, sink.clone());
    let (_hits, sems) = layout(&view);
    assert!(
        sems.iter()
            .any(|node| node.label.as_deref() == Some("SELECT LOCATION")),
        "the map screen is the one on screen now"
    );
}

#[test]
fn the_touch_pad_is_reachable_during_a_race() {
    // The wheel and the four buttons only exist while racing; if the pad is never
    // composed, nothing on screen can be dragged or pressed.
    let (mut sim, ctx, geom, mut ui) = boot();
    sim.world
        .resource_mut::<Intents>()
        .0
        .push(Intent::StartLevel(0));
    apply_intents(&mut sim.world);
    let mut schedule = deadly_descent::sim::build_schedule();
    for _ in 0..4 {
        sim.step_with(std::time::Duration::from_millis(16), |world| {
            schedule.run(world);
        });
    }
    ui::sync_shared_ui(&mut sim.world, &mut ui, &ctx);
    assert!(
        matches!(ui.phase, Phase::Countdown | Phase::Racing),
        "the level is running, not the garage: {:?}",
        ui.phase
    );

    // The pad is opt-in on desktop, where the keyboard drives the car.
    ui.touch.borrow_mut().visible = true;
    let sink: Sink = Arc::new(Mutex::new(Vec::new()));
    let view = app_frame(&mut sim, &ctx, &geom, &ui, sink);
    let (hits, _sems) = layout(&view);
    let pads = hits
        .iter()
        .filter(|hit| hit.on_pointer_down.is_some() && !hit.disabled)
        .count();
    assert!(
        pads >= 5,
        "the wheel and four buttons take pointers during a race, found {pads}"
    );
}
