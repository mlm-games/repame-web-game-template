//! Obstacles: the hazards the levels are built from.
//!
//! Kinds, their damage behaviour and the per-level mix come from the game's own
//! content: the segment scenes (`Holes`, `Balls`, `Containers`, `Spikes`, `Gates`,
//! `Train`, `Press`, `Squeeze`, `Buses`, ...) and the `_impulseToBreak` /
//! `_impulseMultiplier` / `_impulseRadius` values recovered from `Game.Breakable`.

use glam::Vec3;

use crate::sim::data::{Level, friction_at};
use crate::sim::seed::unit;

/// Breakable impulse threshold recovered from the shipped `Game.Breakable`
/// components: 10 in most levels, 12 in the later ones, 50 for the one special
/// piece. Multiplier and radius are shared: 1.2 and 5.0.
pub const BREAK_IMPULSE: f32 = 10.0;
pub const BREAK_IMPULSE_LATE: f32 = 12.0;
pub const BREAK_MULTIPLIER: f32 = 1.2;
pub const BREAK_RADIUS: f32 = 5.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Breakable shipping container wall.
    Container,
    /// Breakable wooden crate.
    Crate,
    /// Static spike strip: destroys on contact above a speed.
    Spikes,
    /// Rolling spiked ball that drifts across the road.
    Ball,
    /// Crushing press that slams down on a cycle.
    Press,
    /// Swinging gate arm.
    Gate,
    /// Train crossing the road on a cycle.
    Train,
    /// Gap in the road.
    Hole,
    /// Ramp that launches the car.
    Ramp,
    /// Parked bus narrowing the road.
    Bus,
    /// Concrete squeeze wall.
    Wall,
}

impl Kind {
    pub fn is_breakable(self) -> bool {
        matches!(self, Kind::Container | Kind::Crate | Kind::Wall)
    }

    pub fn kills_on_impact(self) -> bool {
        matches!(self, Kind::Spikes | Kind::Ball | Kind::Press | Kind::Train)
    }

    pub fn half_extents(self) -> Vec3 {
        match self {
            Kind::Container => Vec3::new(1.3, 1.3, 2.6),
            Kind::Crate => Vec3::new(0.6, 0.6, 0.6),
            Kind::Spikes => Vec3::new(1.6, 0.35, 1.6),
            Kind::Ball => Vec3::splat(1.1),
            Kind::Press => Vec3::new(2.2, 1.0, 2.2),
            Kind::Gate => Vec3::new(0.35, 1.6, 4.0),
            Kind::Train => Vec3::new(1.6, 2.2, 9.0),
            Kind::Hole => Vec3::new(3.0, 0.5, 3.5),
            Kind::Ramp => Vec3::new(3.2, 0.9, 4.5),
            Kind::Bus => Vec3::new(1.3, 1.5, 5.5),
            Kind::Wall => Vec3::new(0.7, 1.6, 3.0),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Obstacle {
    pub kind: Kind,
    /// Distance along the track.
    pub s: f32,
    /// Lateral offset from the road centre.
    pub lateral: f32,
    /// Cycle phase for moving hazards.
    pub phase: f32,
    pub broken: bool,
    /// Current animated offset, updated per tick by [`update`].
    pub offset: Vec3,
}

impl Obstacle {
    pub fn centre(&self, centre: Vec3) -> Vec3 {
        centre + self.offset
    }
}

/// Per-level mix. Counts scale with the props the shipped segment scenes actually
/// contain (recorded in `assets/dd1/segments_summary.json`), so `Spikes` levels are
/// dense and `Start` is empty, exactly as the reference scenes are.
pub fn mix_for(segment: &str) -> Vec<(Kind, f32)> {
    match segment {
        "Start" => vec![],
        "Holes" => vec![(Kind::Hole, 8.0), (Kind::Ramp, 2.0)],
        "LongHoles" => vec![(Kind::Hole, 11.0), (Kind::Ramp, 3.0)],
        "Balls" => vec![(Kind::Ball, 6.0)],
        "Containers" => vec![(Kind::Container, 14.0), (Kind::Crate, 10.0)],
        "Squeeze" => vec![(Kind::Wall, 12.0), (Kind::Container, 6.0)],
        "Press" => vec![(Kind::Press, 6.0)],
        "Spikes" => vec![(Kind::Spikes, 18.0)],
        "Gates" => vec![(Kind::Gate, 8.0)],
        "Train" => vec![(Kind::Train, 4.0), (Kind::Spikes, 6.0)],
        "Buses" => vec![(Kind::Bus, 5.0), (Kind::Crate, 6.0)],
        "Bumps" => vec![(Kind::Ramp, 8.0)],
        "LongJump" | "VeryLongJump" => vec![(Kind::Ramp, 3.0), (Kind::Hole, 2.0)],
        "TwoJumps" => vec![(Kind::Ramp, 4.0), (Kind::Hole, 2.0)],
        "Hill" | "Height" | "Descent" => vec![(Kind::Crate, 8.0), (Kind::Container, 6.0)],
        "Curve" | "TurnLeft" | "Tight" => vec![(Kind::Wall, 10.0), (Kind::Container, 8.0)],
        "Maneuvers" => vec![
            (Kind::Container, 8.0),
            (Kind::Gate, 4.0),
            (Kind::Spikes, 4.0),
        ],
        "RacingTrack" => vec![
            (Kind::Container, 10.0),
            (Kind::Wall, 8.0),
            (Kind::Ramp, 4.0),
            (Kind::Spikes, 4.0),
        ],
        _ => vec![(Kind::Crate, 6.0), (Kind::Container, 4.0)],
    }
}

/// Place the level's obstacle field. `curve` samples the road centreline so
/// obstacles sit on the road rather than on the raw axis.
pub fn place<F>(level: &Level, length: f32, half_width: f32, curve: F) -> Vec<Obstacle>
where
    F: Fn(f32) -> f32,
{
    let mix = mix_for(level.segment());
    if mix.is_empty() {
        return Vec::new();
    }
    let total_weight: f32 = mix.iter().map(|(_, w)| w).sum();
    let density = if level.racing_track_size > 0 {
        0.045
    } else {
        0.06
    };
    let target: f32 = (length * density / total_weight).max(1.0);
    let seed = (level.racing_track_seed.max(0) as u32)
        .wrapping_mul(2654435761)
        .wrapping_add(level.max_possible_score.max(0) as u32);
    let mut out = Vec::new();
    // Obstacles start clear of the grid and stop before the finish line.
    let from = 55.0;
    let to = length - 45.0;
    let mut s = from;
    let mut salt = 1u32;
    while s < to {
        let roll = unit(seed, salt) * total_weight;
        salt = salt.wrapping_add(1);
        let mut acc = 0.0;
        let mut kind = mix[0].0;
        for (k, w) in &mix {
            acc += w;
            if roll <= acc {
                kind = *k;
                break;
            }
        }
        let side = unit(seed, salt);
        salt = salt.wrapping_add(1);
        let lane = unit(seed, salt);
        salt = salt.wrapping_add(1);
        let centre = curve(s);
        let lateral = match kind {
            Kind::Wall | Kind::Bus => {
                if side < 0.5 {
                    -half_width * 0.55
                } else {
                    half_width * 0.55
                }
            }
            Kind::Container => -half_width * 0.6 + lane * half_width * 1.2,
            _ => -half_width * 0.72 + lane * half_width * 1.44,
        };
        let _ = centre;
        out.push(Obstacle {
            kind,
            s,
            lateral,
            phase: unit(seed, salt.wrapping_add(99)) * std::f32::consts::TAU,
            broken: false,
            offset: Vec3::ZERO,
        });
        let gap = match kind {
            Kind::Hole => 22.0,
            Kind::Train => 46.0,
            Kind::Press => 26.0,
            Kind::Ramp => 30.0,
            Kind::Container => 16.0,
            Kind::Crate => 12.0,
            _ => (length / (target * 8.0)).clamp(9.0, 26.0),
        };
        s += gap * (0.75 + unit(seed, salt) * 0.5);
        salt = salt.wrapping_add(1);
    }
    out
}

/// Animate the moving hazards. `t` is race time in seconds.
pub fn update(obstacles: &mut [Obstacle], t: f32, half_width: f32) {
    for ob in obstacles.iter_mut() {
        if ob.broken {
            ob.offset = Vec3::ZERO;
            continue;
        }
        ob.offset = match ob.kind {
            Kind::Ball => {
                let swing = (t * 0.6 + ob.phase).sin();
                Vec3::new(swing * half_width * 0.8, 0.0, 0.0)
            }
            Kind::Gate => {
                let swing = (t * 1.4 + ob.phase).sin().max(0.0);
                Vec3::new(swing * half_width * 1.6, 0.0, 0.0)
            }
            Kind::Press => {
                let cycle = (t * 0.8 + ob.phase).sin();
                Vec3::new(0.0, -(cycle.max(0.0)) * 3.4, 0.0)
            }
            Kind::Train => {
                let swing = (t * 0.35 + ob.phase).sin();
                Vec3::new(swing * half_width * 2.4, 0.0, 0.0)
            }
            _ => Vec3::ZERO,
        };
    }
}

/// Impulse a car takes from hitting a breakable: mass times closing speed.
pub fn break_impulse(mass: f32, speed: f32) -> f32 {
    mass * speed * 0.01
}

/// Breakables resist up to the recovered threshold scaled by the car's mass.
pub fn should_break(obstacle: &Obstacle, mass: f32, speed: f32) -> bool {
    let threshold = if obstacle.s > 500.0 {
        BREAK_IMPULSE_LATE
    } else {
        BREAK_IMPULSE
    };
    break_impulse(mass, speed) > threshold
}

/// Lateral acceleration the tyres can hold at a given speed, using the car's
/// recovered sideways friction curve; used by the AI to pick corner speeds.
pub fn corner_grip(friction: &crate::sim::data::Friction, speed: f32) -> f32 {
    friction_at(friction, speed * 0.02)
}
