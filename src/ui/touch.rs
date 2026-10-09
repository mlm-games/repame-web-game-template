//! Virtual stick: the left of the screen, floating where the pointer lands.
//! Mouse drags drive it too, so desktop is playable without a keyboard.

use glam::Vec2;
use repose_core::input::{PointerEvent, PointerKind};
use repose_core::prelude::{Modifier, View};
use repose_core::{Vec2 as CoreVec2, locals};
use repose_ui::{Box as RBox, Spacer, ViewExt as _, ZStack};

use super::{SharedUi, palette};
use palette::{TOUCH_BG, TOUCH_CENTER, TOUCH_KNOB, TOUCH_KNOB_DARK, TOUCH_RING, TOUCH_SHADOW};

/// How far the knob travels from the origin, in design units.
const MAX_R: f32 = 46.0;
/// The stick only claims the left of the screen.
const ZONE_X: f32 = 0.45;

#[derive(Default)]
pub struct TouchState {
    /// Stick deflection in `-1..=1`, the same analog value the keyboard reads
    /// as a unit vector.
    pub value: Vec2,
    /// Where the ring sits, in design units.
    pub origin: Vec2,
    pub active: bool,
    /// The pointer currently holding the stick, so a second finger cannot
    /// steal the origin out from under the first. Touch ids start at zero and
    /// the mouse is always zero, so the kind is part of the identity.
    owner: Option<Owner>,
    /// dp per design unit, republished every frame: the gesture outlives the
    /// closure that started it, and a resized window moves the grid.
    scale: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Owner {
    kind: PointerKind,
    id: u64,
}

impl TouchState {
    /// Let go of the stick: `value` clears when the touch ends.
    pub fn release(&mut self) {
        self.value = Vec2::ZERO;
        self.active = false;
        self.owner = None;
    }
}

fn owner_of(event: &PointerEvent) -> Owner {
    Owner {
        kind: event.kind,
        id: event.id.0,
    }
}

/// Pointer position in design units: the widget tree lays out in dp.
fn design_pos(px: CoreVec2, scale: f32) -> Vec2 {
    let density = locals::effective_density_scale().max(1e-6);
    let scale = density * scale.max(1e-6);
    Vec2::new(px.x / scale, px.y / scale)
}

pub fn compose(ui: &SharedUi, s: f32, vp: [f32; 2]) -> View {
    if !matches!(ui.phase, crate::sim::state::Phase::Playing) || ui.paused || ui.modal {
        ui.touch.borrow_mut().release();
        return Spacer();
    }
    let handle = ui.touch.clone();
    handle.borrow_mut().scale = s;
    let zone = palette::rect(vp, s, [0.0, 0.0, ZONE_X, 1.0], [0.0; 4])
        .on_pointer_down({
            let handle = handle.clone();
            move |e: PointerEvent| {
                let mut touch = handle.borrow_mut();
                if touch.owner.is_some() {
                    return;
                }
                touch.owner = Some(owner_of(&e));
                touch.active = true;
                touch.value = Vec2::ZERO;
                touch.origin = design_pos(e.window_position, touch.scale);
                e.consume();
            }
        })
        .on_pointer_move({
            let handle = handle.clone();
            move |e: PointerEvent| {
                let mut touch = handle.borrow_mut();
                if touch.owner != Some(owner_of(&e)) {
                    return;
                }
                let here = design_pos(e.window_position, touch.scale);
                let mut delta = here - touch.origin;
                if delta.length() > MAX_R {
                    // The ring slides along with the finger rather than
                    // pinning to where the drag began.
                    touch.origin = here - delta.normalize_or_zero() * MAX_R;
                    delta = delta.normalize_or_zero() * MAX_R;
                }
                touch.value = delta / MAX_R;
                e.consume();
            }
        })
        .on_pointer_up(release_for(&handle))
        .on_pointer_cancel(release_for(&handle));
    ZStack(Modifier::new().fill_max_size()).child(vec![RBox(zone).child(Spacer()), ring(ui, s, vp)])
}

fn release_for(handle: &super::TouchHandle) -> impl Fn(PointerEvent) + 'static {
    let handle = handle.clone();
    move |e: PointerEvent| {
        let mut touch = handle.borrow_mut();
        if touch.owner != Some(owner_of(&e)) {
            return;
        }
        touch.release();
    }
}

fn ring(ui: &SharedUi, s: f32, vp: [f32; 2]) -> View {
    let touch = ui.touch.borrow();
    if !touch.active {
        return Spacer();
    }
    let at = touch.origin.to_array();
    let knob = (touch.origin + touch.value * MAX_R).to_array();
    ZStack(Modifier::new().fill_max_size()).child(vec![
        palette::disc(vp, s, at, MAX_R + 13.0, TOUCH_SHADOW),
        palette::disc(vp, s, at, MAX_R + 3.0, TOUCH_BG),
        palette::disc(vp, s, at, MAX_R + 3.0, TOUCH_RING),
        palette::disc(vp, s, at, 4.0, TOUCH_CENTER),
        palette::disc(vp, s, knob, 16.0, TOUCH_KNOB_DARK),
        palette::disc(vp, s, knob, 13.0, TOUCH_KNOB),
    ])
}
