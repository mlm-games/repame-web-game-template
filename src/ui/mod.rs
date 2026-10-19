//! Screens built from the shipped UI skin and the layout the reference scenes use.
//!
//! Strings, stat rows and panel names follow the build's own UI text
//! ("Select Location", "Earned <coin> N", "Total reward", "Try again!", "NITRO",
//! "BestSpeed", "Continue", "Back", the `cur/max km/h` speed readout). The UI pushes
//! [`UiAction`]s; the app translates them into simulation intents.

pub mod images;
pub mod palette;
pub mod touch;

use std::sync::{Arc, Mutex};

use bevy_ecs::prelude::*;
use glam::Vec2;
use repose_core::Dp;
use repose_core::prelude::{AlignItems, Color, ImageFit, JustifyContent, Modifier, View};
use repose_core::render_context::RenderContext;
use repose_ui::{Box as RBox, Column, Image, ImageExt, Row, Spacer, ViewExt as _, ZStack};

use crate::sim::data;
use crate::sim::meta::{Progress, UpgradeKind};
use crate::sim::race::{Phase, Results, Session};
use images::UiImages;
use palette::*;

pub const DESIGN: [f32; 2] = [1280.0, 720.0];

pub type Sink = Arc<Mutex<Vec<UiAction>>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiAction {
    StartLevel(usize),
    SelectCar(usize),
    BuyCar(usize),
    UpgradeEngine,
    UpgradeSteer,
    UpgradeDurability,
    ToGarage,
    ToMapSelect,
    MapPage(i32),
    Restart,
    NextLevel,
    ToggleMute,
    TogglePause,
    Resume,
    RewardedCoins,
    ClaimLogin,
    BuyNoAds,
    BuyPack(usize),
    ShowLeaderboard,
    ToggleTouchPad,
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

#[derive(Clone, Debug, Default)]
pub struct LevelView {
    pub index: usize,
    pub name: String,
    pub icon: String,
    pub segment: String,
    pub cost: i32,
    pub unlocked: bool,
    pub completed: bool,
    pub best_score: i32,
    pub best_time: Option<f32>,
    pub racing: bool,
    /// Ordinal among racing levels, indexing the shipped map previews.
    pub racing_ordinal: Option<usize>,
    pub speed_target: f32,
    /// Shipped world/mode grouping (`Map.<mode>.<n>`), 0 when unknown.
    pub mode: i32,
}

#[derive(Clone, Debug, Default)]
pub struct CarView {
    pub index: usize,
    pub display: String,
    pub icon: String,
    pub owned: bool,
    pub selected: bool,
    pub price: i32,
    pub coin_purchase: bool,
    pub ads_required: i32,
    pub engine: u8,
    pub steer: u8,
    pub durability: u8,
    pub engine_price: i32,
    pub steer_price: i32,
    pub durability_price: i32,
    pub top_speed_kmh: f32,
    pub durability_hp: f32,
}

#[derive(Clone, Debug)]
pub struct SharedUi {
    pub phase: Phase,
    pub level: usize,
    pub car: usize,
    pub coins: i32,
    pub levels: Vec<LevelView>,
    /// Map screen page, 15 levels each; the grid has room for three rows.
    pub map_page: usize,
    pub cars: Vec<CarView>,
    pub time: f32,
    pub countdown: f32,
    pub speed_kmh: f32,
    pub speed_target: f32,
    pub best_speed_kmh: f32,
    pub damage: f32,
    pub max_damage: f32,
    pub place: usize,
    pub racers: usize,
    pub nitro: f32,
    pub nitro_max: f32,
    pub checkpoints_done: usize,
    pub checkpoints_needed: usize,
    pub level_name: String,
    pub results: Option<Results>,
    pub muted: bool,
    pub paused: bool,
    pub viewport: Vec2,
    pub images: UiImages,
    pub reward_pending: bool,
    /// (streak day, coins) when a login reward is claimable.
    pub login_reward: Option<(u32, i32)>,
    pub no_ads: bool,
    /// Leaderboard id the results screen posts to and opens.
    pub leaderboard_id: String,
    pub leaderboard_posted: bool,
    /// Tutorial step being highlighted (see `meta::SaveData::tutorial_stage`).
    pub tutorial_stage: u8,
    /// Seconds since the app started, for the hint pulse.
    pub elapsed: f32,
    pub touch: touch::TouchHandle,
    pub touch_hint_seen: bool,
}

impl Default for SharedUi {
    fn default() -> Self {
        Self {
            phase: Phase::Garage,
            level: 0,
            car: 0,
            coins: 0,
            levels: Vec::new(),
            map_page: 0,
            cars: Vec::new(),
            time: 0.0,
            countdown: 0.0,
            speed_kmh: 0.0,
            speed_target: 0.0,
            best_speed_kmh: 0.0,
            damage: 0.0,
            max_damage: 1.0,
            place: 1,
            racers: 1,
            nitro: 0.0,
            nitro_max: 1.0,
            checkpoints_done: 0,
            checkpoints_needed: 0,
            level_name: String::new(),
            results: None,
            muted: false,
            paused: false,
            viewport: Vec2::new(DESIGN[0], DESIGN[1]),
            images: UiImages::default(),
            reward_pending: false,
            login_reward: None,
            no_ads: false,
            leaderboard_id: String::new(),
            leaderboard_posted: false,
            tutorial_stage: 3,
            elapsed: 0.0,
            touch: std::rc::Rc::new(std::cell::RefCell::new(touch::TouchState::default())),
            touch_hint_seen: false,
        }
    }
}

impl SharedUi {
    pub fn ui_scale(&self) -> f32 {
        (self.viewport.x / DESIGN[0])
            .min(self.viewport.y / DESIGN[1])
            .max(1e-6)
    }
}

pub fn sync_shared_ui(world: &mut World, ui: &mut SharedUi, ctx: &RenderContext) {
    ui.images.ensure(ctx);
    // Borrowed, not cloned: the save is read every frame and is large enough that
    // a per-frame deep copy showed up in profiles.
    let progress_default = Progress::default();
    let progress = world
        .get_resource::<Progress>()
        .unwrap_or(&progress_default);
    ui.coins = progress.coins();
    ui.login_reward = progress.login_reward_available(today_index());
    ui.muted = progress.data.muted;
    if ui.levels.is_empty() {
        let mut racing_ordinal = 0usize;
        ui.levels = data::levels()
            .iter()
            .enumerate()
            .map(|(i, level)| {
                let racing = level.is_racing_track();
                let ordinal = if racing {
                    let current = racing_ordinal;
                    racing_ordinal += 1;
                    Some(current)
                } else {
                    None
                };
                LevelView {
                    index: i,
                    name: level.name.clone(),
                    icon: level.icon_stem().to_string(),
                    segment: level.segment().to_string(),
                    cost: level.cost_to_unlock,
                    unlocked: progress.data.unlocked(i),
                    completed: progress.data.completed(i),
                    best_score: progress.data.best_score(i).unwrap_or(0),
                    best_time: progress.data.best_time(i),
                    racing,
                    racing_ordinal: ordinal,
                    speed_target: level.speed_to_get_reward as f32,
                    mode: 0,
                }
            })
            .collect();
        ui.cars = data::cars()
            .iter()
            .enumerate()
            .map(|(i, car)| {
                let state = progress.data.car_state(car.id.as_str());
                CarView {
                    index: i,
                    display: car.display.clone(),
                    icon: car.icon.clone(),
                    owned: progress.data.owns(car.id.as_str()),
                    selected: progress.selected_car() == i,
                    price: crate::sim::meta::car_price(i),
                    coin_purchase: car.purchase_type == 1,
                    ads_required: car.ads_required,
                    engine: state.engine,
                    steer: state.steer,
                    durability: state.durability,
                    engine_price: crate::sim::meta::upgrade_price(
                        UpgradeKind::Engine,
                        state.engine,
                    ),
                    steer_price: crate::sim::meta::upgrade_price(UpgradeKind::Steer, state.steer),
                    durability_price: crate::sim::meta::upgrade_price(
                        UpgradeKind::Durability,
                        state.durability,
                    ),
                    top_speed_kmh: top_speed_estimate(car),
                    durability_hp: 2000.0
                        * crate::sim::car::Upgrades::durability_level(state.durability),
                }
            })
            .collect();
    } else {
        for (i, view) in ui.levels.iter_mut().enumerate() {
            view.unlocked = progress.data.unlocked(i);
            view.completed = progress.data.completed(i);
            view.best_score = progress.data.best_score(i).unwrap_or(0);
            view.best_time = progress.data.best_time(i);
        }
        let selected = progress.selected_car();
        for (i, view) in ui.cars.iter_mut().enumerate() {
            let car = data::car_at(i as i32);
            let state = progress.data.car_state(car.id.as_str());
            view.owned = progress.data.owns(car.id.as_str());
            view.selected = selected == i;
            view.engine = state.engine;
            view.steer = state.steer;
            view.durability = state.durability;
            view.engine_price = crate::sim::meta::upgrade_price(UpgradeKind::Engine, state.engine);
            view.steer_price = crate::sim::meta::upgrade_price(UpgradeKind::Steer, state.steer);
            view.durability_price =
                crate::sim::meta::upgrade_price(UpgradeKind::Durability, state.durability);
            view.durability_hp =
                2000.0 * crate::sim::car::Upgrades::durability_level(state.durability);
        }
    }
    // The screen is read outside the session block: the garage and the map screen
    // are shown before any level is loaded, so there is no session to read from.
    ui.phase = world
        .get_resource::<crate::sim::race::Screen>()
        .map(|screen| screen.0)
        .or_else(|| world.get_resource::<Session>().map(|session| session.phase))
        .unwrap_or(Phase::Garage);
    if world.get_resource::<Session>().is_none() {
        // Outside a race the player still picks cars, and this panel is the only
        // place that names the pick: reading the index from a session left it on
        // the first car while the tiles and the 3D car followed the selection.
        ui.car = progress.selected_car();
    }
    if let Some(session) = world.get_resource::<Session>() {
        ui.level = session.level;
        ui.car = session.car;
        ui.time = session.time;
        ui.countdown = session.countdown;
        // The readout is the speed the car is doing now: `top_speed` is the
        // run's best, which only ever climbs and left the dial frozen at the
        // fastest moment of the race.
        ui.speed_kmh = session.player.speed() * 3.6;
        ui.damage = session.player.damage;
        ui.max_damage = session.player.max_damage();
        ui.nitro = session.player.nitro;
        ui.nitro_max = session.player.spec.max_nitro.max(1.0);
        ui.checkpoints_needed = session.checkpoints.len();
        ui.checkpoints_done = session.checkpoints.iter().filter(|c| **c).count();
        let level = session.level();
        ui.speed_target = level.speed_to_get_reward as f32;
        ui.best_speed_kmh = progress
            .data
            .best_speed
            .get(session.level)
            .copied()
            .unwrap_or(0.0);
        ui.level_name = level.name.clone();
        ui.place = session.results.map(|r| r.place).unwrap_or_else(|| {
            1 + session
                .ai
                .iter()
                .filter(|ai| ai.sim.distance > session.player.distance)
                .count()
        });
        ui.racers = session.ai.len() + 1;
        ui.results = session.results;
    }
}

/// Days since the Unix epoch in local time, the unit the login streak counts in.
pub fn today_index() -> i64 {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64 / 86_400)
        .unwrap_or(0)
}

/// Top speed an ideal run through the gearbox reaches: redline in top gear.
fn top_speed_estimate(car: &data::Car) -> f32 {
    let gears = car.gear_count();
    let top = car.gear_ratio(gears - 1) * car.final_drive.unwrap_or(3.5);
    let wheel_rps = (car.cut_off_rpm / top.max(0.1)) / 60.0;
    let mps = wheel_rps * car.wheel_radius() * std::f32::consts::TAU;
    mps * 3.6
}

// ------------------------------------------------------------------ helpers

fn tutorial_hint(ui: &SharedUi, s: f32) -> View {
    if ui.tutorial_stage == 2 && ui.phase == Phase::Racing {
        hint_ring(ui, "HOLD SHIFT FOR NITRO", s)
    } else {
        Spacer()
    }
}

/// A pulsing hint ring, as the shipped `PulsationAnimation` step uses.
fn hint_ring(ui: &SharedUi, text: &str, s: f32) -> View {
    let pulse = 0.55 + 0.45 * (ui.elapsed * 2.4).sin().abs();
    Row(Modifier::new()
        .gap(Dp(8.0 * s))
        .align_items(AlignItems::CENTER)
        .padding_values(repose_core::PaddingValues {
            left: Dp(12.0 * s),
            right: Dp(12.0 * s),
            top: Dp(5.0 * s),
            bottom: Dp(5.0 * s),
        })
        .background(Color((0xFF as f32 * pulse) as u8, 0xC9, 0x3C, 0xF0))
        .clip_rounded(Dp(14.0 * s)))
    .child(label_plain(Modifier::new(), text, 18.0, INK, s))
}

fn sprite(ui: &SharedUi, stem: &str, w: f32, h: f32, s: f32, fit: ImageFit) -> View {
    match ui.images.sprite(stem) {
        Some(handle) => {
            Image(Modifier::new().width(Dp(w * s)).height(Dp(h * s)), handle).image_fit(fit)
        }
        None => RBox(
            Modifier::new()
                .width(Dp(w * s))
                .height(Dp(h * s))
                .background(STEEL),
        )
        .child(Spacer()),
    }
}

fn icon_view(ui: &SharedUi, name: &str, w: f32, h: f32, s: f32) -> View {
    match ui.images.sprite(name) {
        Some(handle) => Image(Modifier::new().width(Dp(w * s)).height(Dp(h * s)), handle)
            .image_fit(ImageFit::Contain),
        None => RBox(
            Modifier::new()
                .width(Dp(w * s))
                .height(Dp(h * s))
                .background(SUN),
        )
        .child(Spacer()),
    }
}

fn coin_row(ui: &SharedUi, amount: i32, size: f32, s: f32) -> View {
    Row(Modifier::new()
        .gap(Dp(6.0 * s))
        .align_items(AlignItems::CENTER))
    .child((
        icon_view(ui, "iconCoin", size, size, s),
        label_plain(Modifier::new(), &format!("{amount}"), size * 0.9, CREAM, s),
    ))
}

fn stat_row(_ui: &SharedUi, title: &str, value: &str, frac: f32, s: f32) -> View {
    Row(Modifier::new()
        .gap(Dp(10.0 * s))
        .align_items(AlignItems::CENTER))
    .child((
        label_plain(Modifier::new().width(Dp(120.0 * s)), title, 16.0, CREAM, s),
        bar(frac, SUN, s, 240.0, 14.0),
        label_plain(Modifier::new(), value, 15.0, SUN, s),
    ))
}

fn upgrade_row(
    ui: &SharedUi,
    title: &str,
    level: u8,
    price: i32,
    action: UiAction,
    sink: &Sink,
    s: f32,
) -> View {
    let affordable = ui.coins >= price && level < 10;
    Row(Modifier::new()
        .gap(Dp(8.0 * s))
        .align_items(AlignItems::CENTER))
    .child((
        label_plain(Modifier::new().width(Dp(110.0 * s)), title, 15.0, CREAM, s),
        label_plain(
            Modifier::new().width(Dp(46.0 * s)),
            &format!("{level}/10"),
            15.0,
            CYAN,
            s,
        ),
        bar(level as f32 / 10.0, CYAN, s, 150.0, 12.0),
        coin_row(ui, price, 20.0, s),
        small_button(ui, "BUY", action, sink, s, !affordable),
    ))
}

fn small_button(
    ui: &SharedUi,
    text: &str,
    action: UiAction,
    sink: &Sink,
    s: f32,
    disabled: bool,
) -> View {
    let mut m = Modifier::new()
        .background(if disabled { LOCKED } else { SUN })
        .border(Dp(2.0 * s), INK, Dp(10.0 * s))
        .padding_values(repose_core::PaddingValues {
            left: Dp(12.0 * s),
            right: Dp(12.0 * s),
            top: Dp(3.0 * s),
            bottom: Dp(3.0 * s),
        })
        .align_items(AlignItems::CENTER)
        .justify_content(JustifyContent::CENTER)
        .focusable(true);
    if !disabled {
        m = m.on_click({
            let sink = sink.clone();
            move || sink.lock().unwrap().push(action)
        });
    }
    let _ = ui;
    RBox(m).child(label_plain(Modifier::new(), text, 15.0, INK, s))
}

// ------------------------------------------------------------------ screens

pub fn compose_root(ui: &SharedUi, sink: Sink) -> View {
    let s = ui.ui_scale();
    let vp = [ui.viewport.x, ui.viewport.y];
    // The garage draws the shipped room and the car in 3D under this layer, so a
    // backdrop here would cover it; the map screen is UI-only and keeps the
    // blurred race shot behind its panels.
    let background = if ui.phase == Phase::MapSelect {
        sprite(ui, "RaceBackgroundBlur", vp[0], vp[1], 1.0, ImageFit::Cover)
    } else {
        Spacer()
    };
    let base = match ui.phase {
        Phase::Garage => garage(ui, s, vp, &sink),
        Phase::MapSelect => map_select(ui, s, vp, &sink),
        Phase::Countdown | Phase::Racing => hud(ui, s, vp, &sink),
        Phase::Finished | Phase::Failed => results(ui, s, vp, &sink),
    };
    let overlay = if ui.paused && matches!(ui.phase, Phase::Countdown | Phase::Racing) {
        pause(ui, s, vp, &sink)
    } else {
        Spacer()
    };
    let daily = if ui.phase == Phase::Garage && ui.login_reward.is_some() {
        daily(ui, s, vp, &sink)
    } else {
        Spacer()
    };
    // The on-screen pad only draws in a race; outside one it is a `Spacer`.
    let pads = touch::compose(ui, s, vp);
    // No opaque sheet and no global scrim here: the 3D viewport is drawn under
    // this layer and has to stay visible. Readability comes from the panels'
    // own translucent backgrounds.
    ZStack(Modifier::new().fill_max_size()).child((background, base, daily, pads, overlay))
}

fn garage(ui: &SharedUi, s: f32, vp: [f32; 2], sink: &Sink) -> View {
    let car = ui.cars.get(ui.car).cloned().unwrap_or_default();
    // top bar
    let top = Row(rect(vp, s, [0.0, 0.0, 0.0, 0.0], [18.0, 12.0, 0.0, 0.0])
        .gap(Dp(14.0 * s))
        .align_items(AlignItems::CENTER))
    .child((
        coin_row(ui, ui.coins, 30.0, s),
        label(Modifier::new(), "GARAGE", 30.0, SUN, s),
        label(
            Modifier::new(),
            &ui.cars
                .get(ui.car)
                .map(|car| car.display.to_uppercase())
                .unwrap_or_default(),
            30.0,
            CREAM,
            s,
        ),
    ));
    // right side buttons
    let right = Row(rect(vp, s, [1.0, 0.0, 1.0, 0.0], [-18.0, 12.0, 0.0, 0.0])
        .gap(Dp(8.0 * s))
        .align_items(AlignItems::CENTER))
    .child((
        icon_view(ui, "no_ads", 40.0, 40.0, s),
        small_button(
            ui,
            if ui.muted { "SOUND OFF" } else { "SOUND ON" },
            UiAction::ToggleMute,
            sink,
            s,
            false,
        ),
    ));
    // car panel
    let mut rows: Vec<View> = Vec::new();
    rows.push(stat_row(
        ui,
        "Speed",
        &format!("{:.0} km/h", car.top_speed_kmh),
        (car.top_speed_kmh / 400.0).clamp(0.0, 1.0),
        s,
    ));
    rows.push(stat_row(
        ui,
        "Durability",
        &format!("{:.0}", car.durability_hp),
        (car.durability_hp / 4000.0).clamp(0.0, 1.0),
        s,
    ));
    if car.owned {
        rows.push(upgrade_row(
            ui,
            "Engine",
            car.engine,
            car.engine_price,
            UiAction::UpgradeEngine,
            sink,
            s,
        ));
        rows.push(upgrade_row(
            ui,
            "Steer",
            car.steer,
            car.steer_price,
            UiAction::UpgradeSteer,
            sink,
            s,
        ));
        rows.push(upgrade_row(
            ui,
            "Durability",
            car.durability,
            car.durability_price,
            UiAction::UpgradeDurability,
            sink,
            s,
        ));
    } else if car.coin_purchase {
        rows.push(
            Row(Modifier::new()
                .gap(Dp(10.0 * s))
                .align_items(AlignItems::CENTER))
            .child((
                label(Modifier::new(), "PRICE", 18.0, CREAM, s),
                coin_row(ui, car.price, 26.0, s),
                small_button(
                    ui,
                    "BUY",
                    UiAction::BuyCar(car.index),
                    sink,
                    s,
                    ui.coins < car.price,
                ),
            )),
        );
    } else {
        rows.push(icon_view(ui, "AD", 60.0, 40.0, s));
        rows.push(label(
            Modifier::new(),
            &format!("{} ADS", car.ads_required),
            18.0,
            CREAM,
            s,
        ));
    }
    // carousel of the shipped car cards
    let mut cards: Vec<View> = Vec::new();
    for c in ui.cars.iter() {
        let selected = c.selected;
        let owned = c.owned;
        let mut card_rows: Vec<View> = Vec::new();
        card_rows.push(match ui.images.car_icon(&c.icon) {
            Some(handle) => {
                let mut image = Image(
                    Modifier::new().width(Dp(80.0 * s)).height(Dp(42.0 * s)),
                    handle,
                )
                .image_fit(ImageFit::Contain);
                if !owned {
                    image = image.image_tint(Color(0x60, 0x60, 0x60, 0xFF));
                }
                image
            }
            None => RBox(
                Modifier::new()
                    .width(Dp(96.0 * s))
                    .height(Dp(52.0 * s))
                    .background(STEEL),
            )
            .child(label_plain(Modifier::new(), &c.display, 12.0, CREAM, s)),
        });
        if !owned {
            card_rows.push(icon_view(ui, "PictoIcon_Lock_2", 18.0, 18.0, s));
        }
        let m = Modifier::new()
            .padding_values(repose_core::PaddingValues {
                left: Dp(6.0 * s),
                right: Dp(6.0 * s),
                top: Dp(4.0 * s),
                bottom: Dp(4.0 * s),
            })
            .align_items(AlignItems::CENTER)
            .justify_content(JustifyContent::CENTER)
            .focusable(true)
            .on_click({
                let sink = sink.clone();
                let action = UiAction::SelectCar(c.index);
                move || sink.lock().unwrap().push(action)
            });
        // Frame sprite behind the card, flat colour when the skin is absent.
        // `Car_Button_Selected` is the tick badge, so the selected card keeps the
        // same backing card and draws the badge over it: using the badge as the
        // card left the pick with no card at all.
        let frame_view =
            match ui.images.sprite("Car_Button_Background") {
                Some(handle) => {
                    Image(Modifier::new().fill_max_size(), handle).image_fit(ImageFit::FitHeight)
                }
                None => RBox(Modifier::new().fill_max_size().background(if selected {
                    SUN
                } else {
                    STEEL
                }))
                .child(Spacer()),
            };
        let mut card_layers: Vec<View> = vec![
            frame_view,
            Column(
                Modifier::new()
                    .gap(Dp(2.0 * s))
                    .align_items(AlignItems::CENTER),
            )
            .child(card_rows),
        ];
        if selected {
            card_layers.push(
                RBox(
                    Modifier::new()
                        .absolute()
                        .offset_left(Dp(64.0 * s))
                        .offset_top(Dp(2.0 * s)),
                )
                .child(icon_view(ui, "Car_Button_Selected", 24.0, 21.0, s)),
            );
        }
        cards
            .push(RBox(m).child(
                ZStack(Modifier::new().size(Dp(92.0 * s), Dp(58.0 * s))).child(card_layers),
            ));
    }
    let carousel = Row(Modifier::new()
        .gap(Dp(6.0 * s))
        .align_items(AlignItems::CENTER))
    .child(cards);
    let panel_content = Column(
        Modifier::new()
            .gap(Dp(5.0 * s))
            .align_items(AlignItems::CENTER),
    )
    .child(rows);
    let panel = RBox(
        Modifier::new()
            .background(PANEL)
            .clip_rounded(Dp(16.0 * s))
            .padding_values(repose_core::PaddingValues {
                left: Dp(20.0 * s),
                right: Dp(20.0 * s),
                top: Dp(10.0 * s),
                bottom: Dp(10.0 * s),
            })
            .max_width(Dp(vp[0] * 0.94)),
    )
    .child(panel_content);
    let placed = RBox(
        Modifier::new()
            .absolute()
            .offset_top(Dp(vp[1] * 0.09))
            .fill_max_width()
            .justify_content(JustifyContent::CENTER)
            .align_items(AlignItems::CENTER),
    )
    .child(panel);
    // The shipped garage keeps the rows up top, the car in the middle of the
    // frame and the start button under it, so the car is never under a panel.
    let start = RBox(
        Modifier::new()
            .absolute()
            .offset_top(Dp(vp[1] * 0.755))
            .fill_max_width()
            .justify_content(JustifyContent::CENTER)
            .align_items(AlignItems::CENTER),
    )
    .child(
        Column(
            Modifier::new()
                .gap(Dp(8.0 * s))
                .align_items(AlignItems::CENTER),
        )
        .child(if car.owned {
            let mut start_rows: Vec<View> = Vec::new();
            start_rows.push(button(
                "RACE",
                SUN,
                26.0,
                s,
                UiAction::ToMapSelect,
                sink,
                false,
            ));
            if ui.tutorial_stage == 0 {
                start_rows.push(hint_ring(ui, "TAP RACE TO PICK A MAP", s));
            }
            Column(
                Modifier::new()
                    .gap(Dp(6.0 * s))
                    .align_items(AlignItems::CENTER),
            )
            .child(start_rows)
        } else {
            Spacer()
        }),
    );
    let shelf = RBox(
        Modifier::new()
            .absolute()
            .offset_top(Dp(vp[1] * (if car.owned { 0.895 } else { 0.80 })))
            .fill_max_width()
            .justify_content(JustifyContent::CENTER)
            .align_items(AlignItems::CENTER),
    )
    .child(carousel);
    ZStack(Modifier::new().fill_max_size()).child((top, right, placed, start, shelf))
}

fn map_select(ui: &SharedUi, s: f32, vp: [f32; 2], sink: &Sink) -> View {
    let mut header: Vec<View> = Vec::new();
    header.push(label(Modifier::new(), "SELECT LOCATION", 40.0, SUN, s));
    header.push(coin_row(ui, ui.coins, 28.0, s));
    // The shipped map selector groups levels into worlds; the grouping comes from
    // the preview names, so it is only known once the images are bound.
    if let Some(ordinal) = ui.levels.get(ui.level).and_then(|l| l.racing_ordinal)
        && let Some(mode) = ui.images.racing_mode(ordinal)
    {
        header.push(label_plain(
            Modifier::new(),
            &format!("WORLD {}", mode),
            20.0,
            CREAM,
            s,
        ));
    }
    let mut grid: Vec<View> = Vec::new();
    for chunk in ui.levels.chunks(5) {
        let mut cells: Vec<View> = Vec::new();
        for cell in chunk {
            let preview = if cell.racing {
                cell.racing_ordinal
                    .and_then(|o| ui.images.racing_preview(o))
            } else {
                ui.images.map_icon(&cell.icon)
            };
            let mut inner: Vec<View> = Vec::new();
            inner.push(match preview {
                Some(handle) => Image(
                    Modifier::new().width(Dp(150.0 * s)).height(Dp(84.0 * s)),
                    handle,
                )
                .image_fit(ImageFit::Cover)
                .image_tint(if cell.unlocked {
                    Color(0xFF, 0xFF, 0xFF, 0xFF)
                } else {
                    Color(0x55, 0x55, 0x60, 0xFF)
                }),
                None => RBox(
                    Modifier::new()
                        .width(Dp(150.0 * s))
                        .height(Dp(84.0 * s))
                        .background(STEEL)
                        .align_items(AlignItems::CENTER)
                        .justify_content(JustifyContent::CENTER),
                )
                .child(label_plain(Modifier::new(), &cell.name, 16.0, CREAM, s)),
            });
            inner.push(label_plain(
                Modifier::new(),
                &cell.name.to_uppercase(),
                14.0,
                CREAM,
                s,
            ));
            if cell.completed {
                inner.push(
                    Row(Modifier::new()
                        .gap(Dp(4.0 * s))
                        .align_items(AlignItems::CENTER))
                    .child((
                        icon_view(ui, "star", 18.0, 18.0, s),
                        label_plain(
                            Modifier::new(),
                            &format!("{}", cell.best_score),
                            13.0,
                            SUN,
                            s,
                        ),
                    )),
                );
            } else if !cell.unlocked {
                inner.push(
                    Row(Modifier::new()
                        .gap(Dp(4.0 * s))
                        .align_items(AlignItems::CENTER))
                    .child((
                        icon_view(ui, "PictoIcon_Lock_2", 16.0, 16.0, s),
                        coin_row(ui, cell.cost, 18.0, s),
                    )),
                );
            } else {
                inner.push(label_plain(Modifier::new(), "READY", 13.0, OK, s));
            }
            let m = Modifier::new()
                .padding_values(repose_core::PaddingValues {
                    left: Dp(5.0 * s),
                    right: Dp(5.0 * s),
                    top: Dp(4.0 * s),
                    bottom: Dp(4.0 * s),
                })
                .clip_rounded(Dp(12.0 * s))
                .align_items(AlignItems::CENTER)
                .justify_content(JustifyContent::CENTER)
                .focusable(true)
                .on_click({
                    let sink = sink.clone();
                    let index = cell.index;
                    move || sink.lock().unwrap().push(UiAction::StartLevel(index))
                });
            let frame_view = match ui.images.sprite("Car_Button_Background") {
                Some(handle) => {
                    Image(Modifier::new().fill_max_size(), handle).image_fit(ImageFit::FillBounds)
                }
                None => RBox(
                    Modifier::new()
                        .fill_max_size()
                        .background(if cell.unlocked { STEEL } else { LOCKED }),
                )
                .child(Spacer()),
            };
            let card = RBox(m).child(
                ZStack(Modifier::new().size(Dp(166.0 * s), Dp(132.0 * s))).child((
                    frame_view,
                    Column(
                        Modifier::new()
                            .gap(Dp(3.0 * s))
                            .align_items(AlignItems::CENTER),
                    )
                    .child(inner),
                )),
            );
            cells.push(card);
        }
        grid.push(
            Row(Modifier::new()
                .gap(Dp(6.0 * s))
                .align_items(AlignItems::CENTER))
            .child(cells),
        );
    }
    let mut col: Vec<View> = header;
    col.push(Spacer());
    // Three rows fit under the header and above the page buttons.
    let per_page = 15;
    let pages = ui.levels.len().div_ceil(per_page).max(1);
    let page = ui.map_page.min(pages - 1);
    let start = page * per_page;
    for row in grid.into_iter().skip(start / 5).take(3) {
        col.push(row);
    }
    if ui.tutorial_stage <= 1 {
        col.push(hint_ring(ui, "PICK ANY MAP TO START", s));
    }
    col.push(Spacer());
    let mut nav: Vec<View> = vec![button(
        "BACK",
        STEEL,
        20.0,
        s,
        UiAction::ToGarage,
        sink,
        false,
    )];
    if pages > 1 {
        nav.push(small_button(
            ui,
            "PREV",
            UiAction::MapPage(-1),
            sink,
            s,
            page == 0,
        ));
        nav.push(label_plain(
            Modifier::new(),
            &format!("{} / {}", page + 1, pages),
            18.0,
            CREAM,
            s,
        ));
        nav.push(small_button(
            ui,
            "NEXT",
            UiAction::MapPage(1),
            sink,
            s,
            page + 1 >= pages,
        ));
    }
    col.push(
        Row(Modifier::new()
            .gap(Dp(10.0 * s))
            .align_items(AlignItems::CENTER))
        .child(nav),
    );
    let content = Column(
        Modifier::new()
            .gap(Dp(6.0 * s))
            .align_items(AlignItems::CENTER),
    )
    .child(col);
    padded_panel(vp, s, content, 10.0)
}

fn hud(ui: &SharedUi, s: f32, vp: [f32; 2], sink: &Sink) -> View {
    let damage_frac = (ui.damage / ui.max_damage.max(1.0)).clamp(0.0, 1.0);
    // The steering wheel and pedals take the bottom corners, where the speed and
    // nitro readouts sit, so the pad replaces them rather than drawing over them.
    let pad = ui.touch.borrow().visible;
    let speed_frac = (ui.speed_kmh / 300.0).clamp(0.0, 1.0);
    // top-left: level + checkpoints
    let mut top_left_children: Vec<View> = Vec::new();
    top_left_children.push(label_plain(
        Modifier::new(),
        &ui.level_name.to_uppercase(),
        20.0,
        CREAM,
        s,
    ));
    if ui.checkpoints_needed > 0 {
        top_left_children.push(
            Row(Modifier::new()
                .gap(Dp(6.0 * s))
                .align_items(AlignItems::CENTER))
            .child((
                icon_view(ui, "CheckpointAquired", 24.0, 24.0, s),
                label_plain(
                    Modifier::new(),
                    &format!("{}/{}", ui.checkpoints_done, ui.checkpoints_needed),
                    18.0,
                    SUN,
                    s,
                ),
            )),
        );
    }
    let top_left = Column(
        rect(vp, s, [0.0, 0.0, 0.0, 0.0], [16.0, 12.0, 0.0, 0.0])
            .gap(Dp(4.0 * s))
            .align_items(AlignItems::START),
    )
    .child(top_left_children);
    // top-centre: the shipped readout shape, current / level target
    let top_centre = Column(
        rect(vp, s, [0.5, 0.0, 0.5, 0.0], [0.0, 8.0, 0.0, 0.0])
            .gap(Dp(2.0 * s))
            .align_items(AlignItems::CENTER),
    )
    .child((
        label_plain(
            Modifier::new(),
            &format!("{:.0}/{:.0} km/h", ui.speed_kmh, ui.speed_target),
            26.0,
            CREAM,
            s,
        ),
        label_plain(Modifier::new(), &format!("{:.1}s", ui.time), 18.0, SUN, s),
    ));
    // top-right: coins, place, pause
    // A right-anchored row needs the width the content has: anchoring its left
    // edge 16 dp from the right edge left the coins and pause button off screen.
    let top_right = Row(rect(vp, s, [0.5, 0.0, 1.0, 0.0], [0.0, 12.0, -16.0, 0.0])
        .gap(Dp(10.0 * s))
        .justify_content(JustifyContent::END)
        .align_items(AlignItems::CENTER))
    .child((
        coin_row(ui, ui.coins, 26.0, s),
        label_plain(
            Modifier::new(),
            &format!("P {}/{}", ui.place, ui.racers),
            22.0,
            CREAM,
            s,
        ),
        small_button(ui, "II", UiAction::TogglePause, sink, s, false),
    ));
    // bottom-left: the shipped speed bar. `speed-dial` is a 512x128 trapezoid,
    // not a round face, so it fills left to right under its own track.
    let bar_width = 260.0;
    let bar_height = 42.0;
    let fill = (bar_width * speed_frac).max(6.0);
    let speed_sprite = ui.images.sprite("speed-dial");
    let speed_bar = ZStack(
        Modifier::new()
            .width(Dp(bar_width * s))
            .height(Dp(bar_height * s)),
    )
    .child((
        match speed_sprite {
            Some(handle) => Image(Modifier::new().fill_max_size(), handle)
                .image_fit(ImageFit::FillBounds)
                .image_tint(Color(0x26, 0x26, 0x2C, 0xFF)),
            None => RBox(Modifier::new().fill_max_size().background(STEEL)),
        },
        RBox(
            Modifier::new()
                .absolute()
                .offset_left(Dp(3.0 * s))
                .offset_top(Dp(4.0 * s))
                .width(Dp(fill * s))
                .height(Dp((bar_height - 8.0) * s))
                .background(SUN)
                .clip_rounded(Dp(4.0 * s)),
        )
        .child(Spacer()),
        RBox(
            Modifier::new()
                .fill_max_size()
                .align_items(AlignItems::CENTER)
                .justify_content(JustifyContent::CENTER),
        )
        .child(label(
            Modifier::new(),
            &format!("{:.0} km/h", ui.speed_kmh),
            24.0,
            CREAM,
            s,
        )),
    ));
    let bottom_left = if pad {
        Spacer()
    } else {
        RBox(
            rect(
                vp,
                s,
                [0.0, 1.0, 0.0, 1.0],
                [14.0, -14.0 - bar_height, 14.0 + bar_width, -14.0],
            )
            .align_items(AlignItems::START)
            .justify_content(JustifyContent::END),
        )
        .child(speed_bar)
    };
    // bottom-right: nitro dial + damage
    let nitro = ZStack(Modifier::new().size(Dp(140.0 * s), Dp(140.0 * s))).child((
        match ui.images.sprite("nitro-dial") {
            Some(handle) => Image(Modifier::new().size(Dp(140.0 * s), Dp(140.0 * s)), handle)
                .image_fit(ImageFit::Contain),
            None => Spacer(),
        },
        match ui.images.sprite("nitro-fill-dial") {
            Some(handle) => Image(
                Modifier::new().size(Dp(140.0 * s), Dp(140.0 * s)).rotate(
                    (ui.nitro / ui.nitro_max.max(1e-3)).clamp(0.0, 1.0) * std::f32::consts::TAU,
                ),
                handle,
            )
            .image_fit(ImageFit::Contain),
            None => Spacer(),
        },
        RBox(
            Modifier::new()
                .size(Dp(140.0 * s), Dp(140.0 * s))
                .align_items(AlignItems::CENTER)
                .justify_content(JustifyContent::CENTER),
        )
        .child(
            Column(
                Modifier::new()
                    .gap(Dp(2.0 * s))
                    .align_items(AlignItems::CENTER),
            )
            .child((
                icon_view(ui, "S_N2O_Icon", 30.0, 30.0, s),
                label_plain(Modifier::new(), "NITRO", 15.0, CYAN, s),
            )),
        ),
    ));
    let bottom_right = if pad {
        Spacer()
    } else {
        Column(
            rect(vp, s, [1.0, 1.0, 1.0, 1.0], [-240.0, -184.0, -14.0, -14.0])
                .gap(Dp(6.0 * s))
                .align_items(AlignItems::END),
        )
        .child((
            nitro,
            Row(Modifier::new()
                .gap(Dp(8.0 * s))
                .align_items(AlignItems::CENTER))
            .child((
                label_plain(
                    Modifier::new(),
                    &format!("{:.0}%", damage_frac * 100.0),
                    16.0,
                    if damage_frac > 0.66 { DANGER } else { CREAM },
                    s,
                ),
                bar(
                    damage_frac,
                    if damage_frac > 0.66 { DANGER } else { SUN },
                    s,
                    160.0,
                    12.0,
                ),
            )),
        ))
    };
    let centre = if ui.phase == Phase::Countdown {
        let n = ui.countdown.ceil().max(1.0);
        RBox(
            Modifier::new()
                .fill_max_size()
                .align_items(AlignItems::CENTER)
                .justify_content(JustifyContent::CENTER),
        )
        .child(label(
            Modifier::new(),
            &format!("{}", n as i32),
            96.0,
            SUN,
            s,
        ))
    } else {
        Spacer()
    };
    let hint_layer = RBox(
        Modifier::new()
            .absolute()
            .offset_top(Dp(vp[1] * 0.72))
            .fill_max_width()
            .justify_content(JustifyContent::CENTER)
            .align_items(AlignItems::CENTER),
    )
    .child(tutorial_hint(ui, s));
    ZStack(Modifier::new().fill_max_size()).child((
        top_left,
        top_centre,
        top_right,
        bottom_left,
        bottom_right,
        centre,
        hint_layer,
    ))
}

fn results(ui: &SharedUi, s: f32, vp: [f32; 2], sink: &Sink) -> View {
    let Some(r) = ui.results else {
        return Spacer();
    };
    let won = ui.phase == Phase::Finished;
    let mut rows: Vec<View> = Vec::new();
    if won {
        rows.push(label(Modifier::new(), "FINISHED", 54.0, OK, s));
        rows.push(
            Row(Modifier::new()
                .gap(Dp(10.0 * s))
                .align_items(AlignItems::CENTER))
            .child((
                icon_view(ui, "star", 30.0, 30.0, s),
                label_plain(
                    Modifier::new(),
                    &format!("PLACE {} / {}", r.place, r.racers),
                    26.0,
                    CREAM,
                    s,
                ),
            )),
        );
    } else {
        rows.push(
            Row(Modifier::new()
                .gap(Dp(10.0 * s))
                .align_items(AlignItems::CENTER))
            .child((
                icon_view(ui, "skull_in_flames", 44.0, 44.0, s),
                label_plain(Modifier::new(), "YOUR CAR SMASHED", 34.0, DANGER, s),
            )),
        );
        rows.push(label_plain(Modifier::new(), "Try again!", 22.0, CREAM, s));
    }
    rows.push(
        Row(Modifier::new()
            .gap(Dp(18.0 * s))
            .align_items(AlignItems::CENTER))
        .child((
            label_plain(
                Modifier::new(),
                &format!("TIME {:.1}s", r.finish_time),
                20.0,
                CREAM,
                s,
            ),
            label_plain(
                Modifier::new(),
                &format!("BEST SPEED {:.0} km/h", r.top_speed_kmh),
                20.0,
                CYAN,
                s,
            ),
            label_plain(
                Modifier::new(),
                &format!("DAMAGE {:.0}/{:.0}", r.damage, r.max_damage),
                20.0,
                DANGER,
                s,
            ),
        )),
    );
    if won {
        rows.push(
            Row(Modifier::new()
                .gap(Dp(8.0 * s))
                .align_items(AlignItems::CENTER))
            .child((
                label_plain(Modifier::new(), "Earned", 24.0, CREAM, s),
                coin_row(ui, r.coins, 26.0, s),
            )),
        );
        rows.push(
            Row(Modifier::new()
                .gap(Dp(8.0 * s))
                .align_items(AlignItems::CENTER))
            .child((
                label_plain(Modifier::new(), "Total reward", 22.0, SUN, s),
                coin_row(ui, ui.coins, 24.0, s),
            )),
        );
        rows.push(
            Row(Modifier::new()
                .gap(Dp(14.0 * s))
                .align_items(AlignItems::CENTER))
            .child((
                label_plain(
                    Modifier::new(),
                    &format!("FINISH +{}", r.completion_bonus),
                    15.0,
                    CREAM,
                    s,
                ),
                label_plain(
                    Modifier::new(),
                    &format!("SPEED +{}", r.speed_bonus),
                    15.0,
                    CREAM,
                    s,
                ),
                label_plain(
                    Modifier::new(),
                    &format!("TIME +{}", r.time_bonus),
                    15.0,
                    CREAM,
                    s,
                ),
                label_plain(
                    Modifier::new(),
                    &format!("DAMAGE +{}", r.damage_bonus),
                    15.0,
                    CREAM,
                    s,
                ),
            )),
        );
        rows.push(small_button(
            ui,
            "FREE COINS (AD)",
            UiAction::RewardedCoins,
            sink,
            s,
            ui.reward_pending,
        ));
    }
    rows.push(
        Row(Modifier::new()
            .gap(Dp(10.0 * s))
            .align_items(AlignItems::CENTER))
        .child((
            button("CONTINUE", SUN, 22.0, s, UiAction::NextLevel, sink, false),
            button("RESTART", CYAN, 20.0, s, UiAction::Restart, sink, false),
            button("GARAGE", STEEL, 20.0, s, UiAction::ToGarage, sink, false),
        )),
    );
    let content = Column(
        Modifier::new()
            .gap(Dp(8.0 * s))
            .align_items(AlignItems::CENTER),
    )
    .child(rows);
    if won {
        return palette::panel(vp, s, content);
    }
    // The losing screen's card is the shipped `YouLoseBack` panel, not a banner:
    // it is 512x321, and stretching it to a row is what squashed it.
    let width = 700.0;
    let height = width * 321.0 / 512.0;
    let backdrop = sprite(ui, "YouLoseBack", width, height, s, ImageFit::FillBounds);
    let overlay = RBox(
        Modifier::new()
            .absolute()
            .fill_max_size()
            .justify_content(JustifyContent::CENTER)
            .align_items(AlignItems::CENTER),
    )
    .child(content);
    RBox(
        Modifier::new()
            .fill_max_size()
            .justify_content(JustifyContent::CENTER)
            .align_items(AlignItems::CENTER),
    )
    .child(
        ZStack(Modifier::new().width(Dp(width * s)).height(Dp(height * s)))
            .child((backdrop, overlay)),
    )
}

fn daily(ui: &SharedUi, s: f32, vp: [f32; 2], sink: &Sink) -> View {
    let Some((day, coins)) = ui.login_reward else {
        return Spacer();
    };
    let content = Column(
        Modifier::new()
            .gap(Dp(10.0 * s))
            .align_items(AlignItems::CENTER),
    )
    .child((
        label(Modifier::new(), "DAILY REWARD", 40.0, SUN, s),
        label_plain(Modifier::new(), &format!("DAY {day}"), 22.0, CREAM, s),
        coin_row(ui, coins, 40.0, s),
        button("CLAIM", SUN, 24.0, s, UiAction::ClaimLogin, sink, false),
    ));
    palette::panel(vp, s, content)
}

fn pause(ui: &SharedUi, s: f32, vp: [f32; 2], sink: &Sink) -> View {
    let content = Column(
        Modifier::new()
            .gap(Dp(10.0 * s))
            .align_items(AlignItems::CENTER),
    )
    .child((
        label(Modifier::new(), "PAUSED", 46.0, SUN, s),
        button("RESUME", OK, 22.0, s, UiAction::Resume, sink, false),
        button("RESTART", CYAN, 20.0, s, UiAction::Restart, sink, false),
        button("GARAGE", STEEL, 18.0, s, UiAction::ToGarage, sink, false),
        small_button(
            ui,
            if ui.muted { "SOUND OFF" } else { "SOUND ON" },
            UiAction::ToggleMute,
            sink,
            s,
            false,
        ),
        small_button(
            ui,
            if ui.touch.borrow().visible {
                "TOUCH PAD ON"
            } else {
                "TOUCH PAD OFF"
            },
            UiAction::ToggleTouchPad,
            sink,
            s,
            false,
        ),
    ));
    palette::panel(vp, s, content)
}
