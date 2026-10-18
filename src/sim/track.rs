//! Track geometry: a descending road with the level's obstacles on it.
//!
//! The shipped game assembles each level from hand-authored segment scenes
//! (`Assets/Game/Scenes/TracksInGame/*`) and, for the Racing Track levels, from a
//! seeded generator over road pieces. The obstacle *placement* here comes from the
//! extracted segment layouts in `assets/dd1/segments`; the road itself is
//! regenerated from the level's seed and size with the recalibrated downhill
//! profile (see README: segment internals are only partially recoverable).

use glam::Vec3;

use crate::sim::seed::hash;

use crate::sim::data::Level;

pub const ROAD_HALF_WIDTH: f32 = 9.0;

/// Track length in metres as a function of the level's racing track size
/// (0 = short obstacle course, 3 = long procedural race).
pub fn length_for(level: &Level) -> f32 {
    match level.racing_track_size {
        0 => 420.0,
        1 => 640.0,
        2 => 900.0,
        _ => 1200.0,
    }
}

pub fn width_for(level: &Level) -> f32 {
    if level.is_racing_track() {
        ROAD_HALF_WIDTH
    } else {
        ROAD_HALF_WIDTH * 0.85
    }
}

/// Downhill profile: a steady grade with eased start and a run-out, in metres of
/// drop over the track length. The descent is what makes the game what it is, so
/// the drop scales with the level's size.
pub fn drop_for(level: &Level) -> f32 {
    match level.racing_track_size {
        0 => 90.0,
        1 => 150.0,
        2 => 220.0,
        _ => 300.0,
    }
}

#[derive(Debug)]
pub struct Track {
    pub length: f32,
    pub half_width: f32,
    pub drop: f32,
    /// Lateral offset of the road centre at a distance along the track.
    pub curve: Vec<f32>,
    pub curve_step: f32,
    pub seed: u32,
    pub checkpoints: Vec<f32>,
    pub finish: f32,
    pub start: Vec3,
}

impl Track {
    pub fn build(level: &Level) -> Self {
        let length = length_for(level);
        let half_width = width_for(level);
        let drop = drop_for(level);
        let seed =
            level.racing_track_seed.max(0) as u32 * 7919 + level.max_possible_score.max(0) as u32;
        let step = 5.0;
        let count = (length / step) as usize + 2;
        // Lateral wander: sum of two low-frequency sines seeded from the level so
        // every level has a recognisable shape, plus a slow drift down the hill.
        let a = (hash(seed) % 1000) as f32 / 1000.0;
        let b = (hash(seed ^ 0x51ed) % 1000) as f32 / 1000.0;
        let amp = if level.is_racing_track() {
            half_width * 2.2
        } else {
            half_width * 0.9
        };
        let mut curve = Vec::with_capacity(count);
        for i in 0..count {
            let t = (i as f32 * step) / length;
            let wobble = (t * std::f32::consts::TAU * (1.0 + a)).sin() * 0.6
                + (t * std::f32::consts::TAU * (2.0 + b * 1.5)).sin() * 0.4;
            let ease = (t * 6.0).min(1.0) * ((1.0 - t) * 8.0).min(1.0);
            curve.push(wobble * amp * ease);
        }
        let checkpoint_count = if level.require_checkpoints { 3 } else { 0 };
        let checkpoints = (1..=checkpoint_count)
            .map(|k| length * k as f32 / (checkpoint_count + 1) as f32)
            .collect();
        let start_z = 0.0;
        let start = Vec3::new(0.0, Self::height_at_track(0.0, length, drop), start_z);
        Self {
            length,
            half_width,
            drop,
            curve,
            curve_step: step,
            seed,
            checkpoints,
            finish: length,
            start,
        }
    }

    pub fn curve_lookup(curve: &[f32], step: f32, s: f32) -> f32 {
        if curve.is_empty() {
            return 0.0;
        }
        let f = (s / step).clamp(0.0, (curve.len() - 1) as f32);
        let i = f.floor() as usize;
        let j = (i + 1).min(curve.len() - 1);
        let t = f - i as f32;
        curve[i] + (curve[j] - curve[i]) * t
    }

    /// Downhill height for a distance travelled, shared by the road mesh, the
    /// wheel raycasts and the AI.
    pub fn height_at_track(s: f32, length: f32, drop: f32) -> f32 {
        let t = (s / length).clamp(0.0, 1.0);
        // Steeper in the middle, easing into the finish.
        let eased = t * t * (3.0 - 2.0 * t);
        -drop * (t * 0.35 + eased * 0.65)
    }

    pub fn height(&self, s: f32) -> f32 {
        Self::height_at_track(s, self.length, self.drop)
    }

    /// Road centre in world space at a distance along the track.
    pub fn centre(&self, s: f32) -> Vec3 {
        Vec3::new(
            Self::curve_lookup(&self.curve, self.curve_step, s),
            self.height(s),
            0.0,
        )
    }

    pub fn road_pos(&self, s: f32, lateral: f32) -> Vec3 {
        let c = self.centre(s);
        Vec3::new(c.x + lateral, c.y, c.z)
    }

    /// Surface height at an arbitrary world point, from the road ribbon.
    pub fn surface(&self, x: f32, z: f32) -> Option<f32> {
        // The road runs along +x with z as its length axis in sim space.
        let s = z;
        if !(0.0..=self.length).contains(&s) {
            return None;
        }
        let c = self.centre(s);
        let lateral = x - c.x;
        if lateral.abs() > self.half_width {
            return None;
        }
        Some(c.y)
    }

    pub fn on_road(&self, x: f32, z: f32) -> bool {
        self.surface(x, z).is_some()
    }
}
