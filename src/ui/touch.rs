//! Touch controls, after the shipped `PG.UI.SteerWheelControls` scheme: a
//! draggable steering wheel that returns to centre (`SteerWheelUI`:
//! `MaxSteerWheelAngle`, `SteerWheelToDefaultSpeed`, `WheelIsPressed`,
//! `PrevTouchPos`) plus acceleration, brake, handbrake and nitro buttons
//! (`SteerWheelControls`' own button set).
//!
//! Pointer events arrive from the UI layer, so a mouse drives exactly the same
//! code as a finger. They land in a shared [`TouchState`] that the app merges with
//! the keyboard each frame.

use std::cell::RefCell;
use std::rc::Rc;

use glam::Vec2;
use repose_core::Dp;
use repose_core::Vec2 as CoreVec2;
use repose_core::input::{PointerEvent, PointerKind};
use repose_core::locals;
use repose_core::prelude::ImageFit;
use repose_core::prelude::{AlignItems, JustifyContent, Modifier, View};
use repose_ui::{Box as RBox, Image, ImageExt, Spacer, ViewExt as _};

use super::SharedUi;
use crate::sim::race::Phase;

/// Horizontal drag, in design units, that means full lock.
pub const MAX_STEER_PX: f32 = 130.0;
/// Below this the wheel reads as centred.
pub const STEER_DEADZONE: f32 = 0.05;
/// Return-to-centre rate when released, per second.
pub const WHEEL_RETURN_SPEED: f32 = 3.0;
/// Wheel art rotation at full lock, degrees.
pub const MAX_WHEEL_ANGLE_DEG: f32 = 110.0;

const WHEEL_BOX: f32 = 190.0;
const WHEEL_MARGIN: f32 = 22.0;
const BUTTON_BOX: f32 = 82.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TouchButton {
    Accel,
    Brake,
    Handbrake,
    Nitro,
}

impl TouchButton {
    fn index(self) -> usize {
        match self {
            TouchButton::Accel => 0,
            TouchButton::Brake => 1,
            TouchButton::Handbrake => 2,
            TouchButton::Nitro => 3,
        }
    }

    fn label(self) -> &'static str {
        match self {
            TouchButton::Accel => "GAS",
            TouchButton::Brake => "BRAKE",
            TouchButton::Handbrake => "HAND",
            TouchButton::Nitro => "NITRO",
        }
    }

    /// Shipped button sprite for this control, when the export has it.
    fn sprite(self) -> &'static str {
        match self {
            TouchButton::Accel => "MobileButtons_1",
            TouchButton::Brake => "MobileButtons_2",
            TouchButton::Handbrake => "MobileButtons_5",
            TouchButton::Nitro => "Nitro_Button",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Owner {
    kind: PointerKind,
    id: u64,
}

#[derive(Debug)]
pub struct TouchState {
    /// −1..1, left negative.
    pub steer: f32,
    pub throttle: f32,
    pub brake: f32,
    pub handbrake: bool,
    pub nitro: bool,
    /// The HUD is on by default; the app toggles it.
    pub visible: bool,
    wheel_origin: Vec2,
    wheel_owner: Option<Owner>,
    pressed: [Option<Owner>; 4],
    scale: f32,
}

impl Default for TouchState {
    fn default() -> Self {
        Self {
            steer: 0.0,
            throttle: 0.0,
            brake: 0.0,
            handbrake: false,
            nitro: false,
            visible: true,
            wheel_origin: Vec2::ZERO,
            wheel_owner: None,
            pressed: [None; 4],
            scale: 1.0,
        }
    }
}

impl TouchState {
    pub fn release_all(&mut self) {
        self.release_buttons();
        self.steer = 0.0;
        self.wheel_owner = None;
    }

    /// Let go of the buttons only; the wheel keeps its angle and decays on its
    /// own, as the shipped wheel does.
    pub fn release_buttons(&mut self) {
        self.pressed = [None; 4];
        self.throttle = 0.0;
        self.brake = 0.0;
        self.handbrake = false;
        self.nitro = false;
    }

    /// Decay the wheel toward centre while nothing holds it.
    pub fn step(&mut self, dt: f32) {
        if self.wheel_owner.is_none() && self.steer != 0.0 {
            let decay = (WHEEL_RETURN_SPEED * dt).min(self.steer.abs());
            self.steer -= self.steer.signum() * decay;
            if self.steer.abs() < STEER_DEADZONE {
                self.steer = 0.0;
            }
        }
    }

    /// Steer value the app consumes: the wheel ignores its own deadzone so the
    /// car does not twitch at centre.
    pub fn steer_input(&self) -> f32 {
        if self.steer.abs() < STEER_DEADZONE {
            0.0
        } else {
            self.steer
        }
    }

    pub fn any_input(&self) -> bool {
        self.steer_input() != 0.0
            || self.throttle > 0.0
            || self.brake > 0.0
            || self.handbrake
            || self.nitro
    }

    /// Test hook: press a named button as a fresh pointer would.
    pub fn press_for_test(&mut self, button: &str) {
        let button = match button {
            "accel" => TouchButton::Accel,
            "brake" => TouchButton::Brake,
            "handbrake" => TouchButton::Handbrake,
            _ => TouchButton::Nitro,
        };
        self.press(
            button,
            Owner {
                kind: PointerKind::Touch,
                id: 1,
            },
        );
    }

    fn press(&mut self, button: TouchButton, owner: Owner) {
        self.pressed[button.index()] = Some(owner);
        match button {
            TouchButton::Accel => self.throttle = 1.0,
            TouchButton::Brake => self.brake = 1.0,
            TouchButton::Handbrake => self.handbrake = true,
            TouchButton::Nitro => self.nitro = true,
        }
    }

    fn release(&mut self, button: TouchButton, owner: Owner) {
        if self.pressed[button.index()] != Some(owner) {
            return;
        }
        self.pressed[button.index()] = None;
        match button {
            TouchButton::Accel => self.throttle = 0.0,
            TouchButton::Brake => self.brake = 0.0,
            TouchButton::Handbrake => self.handbrake = false,
            TouchButton::Nitro => self.nitro = false,
        }
    }
}

pub type TouchHandle = Rc<RefCell<TouchState>>;

fn owner_of(event: &PointerEvent) -> Owner {
    Owner {
        kind: event.kind,
        id: event.id.0,
    }
}

fn design_pos(px: CoreVec2, scale: f32) -> Vec2 {
    let density = locals::effective_density_scale().max(1e-6);
    let scale = density * scale.max(1e-6);
    Vec2::new(px.x / scale, px.y / scale)
}

/// The touch HUD: steering wheel bottom-left, pedals and nitro bottom-right.
/// Inactive outside a race, and it never eats clicks meant for menus.
pub fn compose(ui: &SharedUi, s: f32, vp: [f32; 2]) -> View {
    let handle = ui.touch.clone();
    handle.borrow_mut().scale = s;
    let driving = matches!(ui.phase, Phase::Countdown | Phase::Racing) && !ui.paused;
    if !handle.borrow().visible || !driving {
        handle.borrow_mut().release_all();
        return Spacer();
    }
    let steer = handle.borrow().steer;
    let layers: Vec<View> = vec![
        wheel_zone(&handle, ui, s, vp, steer),
        pedal_row(&handle, ui, s, vp),
    ];
    let mut stack = repose_ui::ZStack(Modifier::new().fill_max_size());
    stack = stack.child(layers);
    stack
}

fn wheel_zone(handle: &TouchHandle, ui: &SharedUi, s: f32, vp: [f32; 2], steer: f32) -> View {
    let art = match ui.images.sprite("teerWheel") {
        Some(handle) => Image(
            Modifier::new()
                .size(Dp(WHEEL_BOX * s), Dp(WHEEL_BOX * s))
                .rotate(-steer * MAX_WHEEL_ANGLE_DEG.to_radians()),
            handle,
        )
        .image_fit(ImageFit::Contain),
        None => RBox(
            Modifier::new()
                .size(Dp(WHEEL_BOX * s), Dp(WHEEL_BOX * s))
                .clip_rounded(Dp(WHEEL_BOX * s * 0.5))
                .background(super::palette::STEEL),
        )
        .child(Spacer()),
    };
    let centre = RBox(
        Modifier::new()
            .size(Dp(WHEEL_BOX * s), Dp(WHEEL_BOX * s))
            .align_items(AlignItems::CENTER)
            .justify_content(JustifyContent::CENTER),
    )
    .child(super::palette::label_plain(
        Modifier::new(),
        "STEER",
        14.0,
        super::palette::CREAM,
        s,
    ));

    let down = handle.clone();
    let moved = handle.clone();
    let up = handle.clone();
    let cancel = handle.clone();
    RBox(
        super::palette::rect(
            vp,
            s,
            [0.0, 1.0, 0.0, 1.0],
            [WHEEL_MARGIN, 0.0, 0.0, -WHEEL_MARGIN],
        )
        .size(Dp(WHEEL_BOX * s), Dp(WHEEL_BOX * s))
        .align_items(AlignItems::CENTER)
        .justify_content(JustifyContent::CENTER)
        .on_pointer_down(move |e: PointerEvent| {
            let mut touch = down.borrow_mut();
            if touch.wheel_owner.is_some() {
                return;
            }
            touch.wheel_owner = Some(owner_of(&e));
            touch.wheel_origin = design_pos(e.window_position, touch.scale);
            e.consume();
        })
        .on_pointer_move(move |e: PointerEvent| {
            let mut touch = moved.borrow_mut();
            if touch.wheel_owner != Some(owner_of(&e)) {
                return;
            }
            let here = design_pos(e.window_position, touch.scale);
            touch.steer = ((here.x - touch.wheel_origin.x) / MAX_STEER_PX).clamp(-1.0, 1.0);
            e.consume();
        })
        .on_pointer_up(move |e: PointerEvent| {
            let mut touch = up.borrow_mut();
            if touch.wheel_owner == Some(owner_of(&e)) {
                touch.wheel_owner = None;
            }
        })
        .on_pointer_cancel(move |e: PointerEvent| {
            let mut touch = cancel.borrow_mut();
            if touch.wheel_owner == Some(owner_of(&e)) {
                touch.wheel_owner = None;
            }
        }),
    )
    .child(art)
    .child(centre)
}

fn pedal_row(handle: &TouchHandle, ui: &SharedUi, s: f32, vp: [f32; 2]) -> View {
    let column = |button: TouchButton, x: f32, y: f32| {
        RBox(
            super::palette::rect(vp, s, [1.0, 1.0, 1.0, 1.0], [x, 0.0, 0.0, -y])
                .size(Dp(BUTTON_BOX * s), Dp(BUTTON_BOX * s))
                .align_items(AlignItems::CENTER)
                .justify_content(JustifyContent::CENTER)
                .on_pointer_down({
                    let handle = handle.clone();
                    move |e: PointerEvent| {
                        handle.borrow_mut().press(button, owner_of(&e));
                        e.consume();
                    }
                })
                .on_pointer_up({
                    let handle = handle.clone();
                    move |e: PointerEvent| {
                        handle.borrow_mut().release(button, owner_of(&e));
                    }
                })
                .on_pointer_cancel({
                    let handle = handle.clone();
                    move |e: PointerEvent| {
                        handle.borrow_mut().release(button, owner_of(&e));
                    }
                }),
        )
        .child(pedal_art(ui, button, s))
    };
    let mut stack = repose_ui::ZStack(Modifier::new().fill_max_size());
    // Brake sits left of the accelerator, nitro above, handbrake inboard.
    stack = stack.child((
        column(
            TouchButton::Brake,
            -(BUTTON_BOX * 2.0 + 46.0),
            BUTTON_BOX + 24.0,
        ),
        column(TouchButton::Accel, -30.0, BUTTON_BOX + 24.0),
        column(
            TouchButton::Nitro,
            -(BUTTON_BOX * 2.0 + 46.0),
            BUTTON_BOX * 2.0 + 46.0,
        ),
        column(TouchButton::Handbrake, -30.0, BUTTON_BOX * 2.0 + 46.0),
    ));
    stack
}

fn pedal_art(ui: &SharedUi, button: TouchButton, s: f32) -> View {
    let label = button.label();
    match ui.images.sprite(button.sprite()) {
        Some(handle) => RBox(
            Modifier::new()
                .size(Dp(BUTTON_BOX * s), Dp(BUTTON_BOX * s))
                .align_items(AlignItems::CENTER)
                .justify_content(JustifyContent::CENTER)
                .hit_passthrough(),
        )
        .child((
            Image(Modifier::new().fill_max_size(), handle).image_fit(ImageFit::Contain),
            super::palette::label_plain(Modifier::new(), label, 12.0, super::palette::INK, s),
        )),
        None => RBox(
            Modifier::new()
                .size(Dp(BUTTON_BOX * s), Dp(BUTTON_BOX * s))
                .clip_rounded(Dp(BUTTON_BOX * s * 0.5))
                .background(super::palette::SUN)
                .align_items(AlignItems::CENTER)
                .justify_content(JustifyContent::CENTER)
                .hit_passthrough(),
        )
        .child(super::palette::label_plain(
            Modifier::new(),
            label,
            13.0,
            super::palette::INK,
            s,
        )),
    }
}
