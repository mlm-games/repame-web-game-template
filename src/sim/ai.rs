//! AI drivers.
//!
//! Shape from the build: `PG.BaseAIControl` (35 methods) and `PG.RaceAIControl`
//! (27 methods) drive the car through an `AIPath` with a PID steering loop
//! (`P`, `I`, `D`, `PID` fields) and a set of hit-point rays
//! (`MainHits`, `AdditionalHits`, `MainHitPoints`, `AdditionalHitPoints`). The
//! numbers below are the shipped `PG.BaseAIConfig` + `PG.RaceAIConfig` values read
//! out of the two `RaceAIConfigAsset`s; the loop that consumes them is ours.

use crate::sim::car::{CarSim, Input};
use crate::sim::data::{self, AiConfig};
use crate::sim::obstacles::Kind;
use crate::sim::race::TrackWorld;

#[derive(Clone, Debug)]
pub struct AiDriver {
    pub sim: CarSim,
    pub skill: f32,
    /// `AIConfigAsset` profile: aggressive racing or the precise variant.
    pub config: &'static AiConfig,
    /// Lateral offset the driver is currently seeking, slewed at
    /// `change_horizontal_offset_speed`.
    pub lane_offset: f32,
    pub main_hit_timer: f32,
    pub additional_hit_timer: f32,
    pub reverse_timer: f32,
    pub reverse_cooldown: f32,
    pub stuck_timer: f32,
    /// PID steering state, as `RaceAIControl.P/I/D` describe.
    pub steer_integral: f32,
    pub steer_prev: f32,
}

impl AiDriver {
    pub fn new(sim: CarSim, skill: f32, config: &'static AiConfig) -> Self {
        Self {
            sim,
            skill: skill.clamp(0.5, 1.0),
            config,
            lane_offset: 0.0,
            main_hit_timer: 0.0,
            additional_hit_timer: 0.0,
            reverse_timer: 0.0,
            reverse_cooldown: 0.0,
            stuck_timer: 0.0,
            steer_integral: 0.0,
            steer_prev: 0.0,
        }
    }

    /// Distance to look down the road for the target point.
    fn target_offset(&self) -> f32 {
        self.config.offset_to_target_point
            + self.config.speed_factor_to_target_point * self.sim.speed()
    }

    fn turn_prediction_offset(&self) -> f32 {
        self.config.offset_turn_prediction
            + self.config.speed_factor_to_turn_prediction * self.sim.speed()
    }

    /// Speed the driver is willing to carry: the profile cap, the level's bot
    /// cap, and the corner limit from the car's own sideways friction.
    pub fn target_speed(&self, track_world: &TrackWorld, level_max: f32) -> f32 {
        let track = &track_world.track;
        let s = self.sim.pos.z;
        let c1 = track.centre((s + self.target_offset()).min(track.length));
        let c2 = track.centre((s + self.target_offset() * 2.0).min(track.length));
        let curvature = ((c2.x - self.sim.pos.x) - (c1.x - self.sim.pos.x)).abs()
            / self.target_offset().max(1.0);
        let grip =
            data::friction_at(&self.sim.spec.wheels[0].side_f, self.sim.speed() * 0.02).max(0.2);
        let corner = (grip * 9.81 / curvature.max(1e-3)).sqrt();
        let profile_cap = self.config.max_speed / 3.6;
        let hard_cap = profile_cap.min(level_max / 3.6);
        let boldness = 1.0 + (self.config.aggressiveness - 0.4) * 0.6;
        (corner * boldness).min(hard_cap) * self.skill
    }

    /// Steer, throttle, brake and nitro for this tick. Mutates the driver's own
    /// timers, never the world.
    pub fn control(&mut self, track_world: &TrackWorld, level_max: f32, dt: f32) -> Input {
        let cfg = self.config;
        let track = &track_world.track;
        let pos = self.sim.pos;
        let speed = self.sim.speed();

        // --- obstacle rays: main look-ahead plus the wider additional pass
        let mut main_hit = None;
        let mut additional_hit = None;
        let look_from = pos.z - 4.0;
        let look_to = pos.z + cfg.obstacle_hit_distance;
        let from = track_world.obstacles.partition_point(|ob| ob.s < look_from);
        let to = track_world.obstacles.partition_point(|ob| ob.s <= look_to);
        for ob in &track_world.obstacles[from..to] {
            if ob.broken || ob.kind == Kind::Hole {
                continue;
            }
            let dz = ob.s - pos.z;
            if !(1.0..cfg.obstacle_hit_distance).contains(&dz) {
                continue;
            }
            let ob_x = track.centre(ob.s).x + ob.lateral + ob.offset.x;
            let dx = ob_x - pos.x;
            if dx.abs() < self.sim.spec.track_width() * 0.75 {
                if main_hit.is_none() {
                    main_hit = Some(dx);
                }
            } else if dx.abs() < cfg.obstacle_hit_distance * 0.5 && additional_hit.is_none() {
                additional_hit = Some(dx);
            }
        }
        if let Some(dx) = main_hit {
            self.main_hit_timer = cfg.main_hit_dellay_time;
            let want = -dx.signum() * cfg.offset_for_main_hit_points;
            self.lane_offset = want;
        } else if let Some(dx) = additional_hit {
            self.additional_hit_timer = cfg.additional_hit_dellay_time;
            self.lane_offset = -dx.signum() * cfg.offset_for_additional_hit_points;
        } else {
            self.main_hit_timer = (self.main_hit_timer - dt).max(0.0);
            self.additional_hit_timer = (self.additional_hit_timer - dt).max(0.0);
            if self.main_hit_timer <= 0.0 && self.additional_hit_timer <= 0.0 {
                self.lane_offset *= 1.0 - (cfg.change_horizontal_offset_speed * dt).min(1.0);
            }
        }

        // --- target point down the road, offset laterally toward a clear lane,
        // --- and a further turn-prediction point the heading error aims at
        let target_s = (pos.z + self.target_offset()).min(track.length);
        let ahead = track.centre(target_s);
        let jitter = (cfg.offset_for_main_hit_directions * (pos.z * 0.05).sin()) * 0.5;
        let desired_x = ahead.x + self.lane_offset + jitter;
        let prediction_s = (pos.z + self.turn_prediction_offset()).min(track.length);
        let prediction = track.centre(prediction_s);
        let predicted_x = prediction.x + self.lane_offset * 0.5 + jitter;
        let heading_error = (predicted_x - desired_x) * 0.5 / self.target_offset().max(1.0)
            + (desired_x - pos.x) / self.target_offset().max(1.0);

        // --- PID steering
        let p = 1.1;
        let i = 0.25;
        let d = 0.35;
        self.steer_integral = (self.steer_integral + heading_error * dt).clamp(-1.0, 1.0);
        let derivative = (heading_error - self.steer_prev) / dt.max(1e-3);
        self.steer_prev = heading_error;
        let steer = (p * heading_error + i * self.steer_integral + d * derivative).clamp(-1.0, 1.0);

        // --- speed control with the reverse recovery the build describes
        let target = self.target_speed(track_world, level_max);
        let mut reverse = false;
        if speed < cfg.min_speed / 3.6 {
            self.stuck_timer += dt;
        } else {
            self.stuck_timer = 0.0;
        }
        if self.reverse_timer > 0.0 {
            self.reverse_timer -= dt;
            reverse = true;
        } else if self.reverse_cooldown > 0.0 {
            self.reverse_cooldown -= dt;
        } else if self.stuck_timer > cfg.reverse_wait_time {
            self.reverse_timer = cfg.reverse_time;
            self.reverse_cooldown = cfg.between_reverse_time_for_reset;
            self.stuck_timer = 0.0;
        }
        let throttle = if reverse {
            0.0
        } else if speed < target {
            1.0
        } else {
            0.0
        };
        let brake = if reverse {
            1.0
        } else if speed > target * 1.06 {
            0.6
        } else {
            0.0
        };
        Input {
            throttle,
            brake,
            steer: if reverse { -steer } else { steer },
            handbrake: false,
            nitro: self.config.aggressiveness > 0.6 && speed > target * 0.75,
        }
    }
}
