//! Vehicle simulation.
//!
//! Four raycast wheels with the recovered suspension and friction curves, an
//! engine with the recovered torque curve and gear ratios, and impulse-based
//! damage. Integration is ours (Unity's WheelCollider internals are not
//! recoverable from the build); the parameters are the shipped ones.

use glam::{Vec3, vec3};

use crate::sim::data::{Car, friction_at};
use crate::sim::track::Track;

pub const GRAVITY: f32 = -9.81;
pub const MAX_CATCHUP: f32 = 8.0;

/// `SteerConfig.ABS` is 1.0 on the shipped car prefabs (read out of each car's
/// inline steer config, after the five steering curves); `TCS` is 0.0. ABS
/// modulates brake force once a wheel passes its forward-friction peak.
/// Driven wheels per car: the shipped `PG.Wheel.DriveWheel` flags are true on
/// three or four of the four wheels depending on the car.
pub const DRIVEN_WHEELS: usize = 4;

/// Damage per second while a wheel set is off the road, scaled by speed in
/// `step`. DESIGNED: the shipped off-road rule is in code.
pub const OFF_ROAD_DAMAGE_PER_SEC: f32 = 260.0;

pub const ABS_ENABLED: bool = true;
pub const TCS_ENABLED: bool = false;

#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub throttle: f32,
    pub brake: f32,
    pub steer: f32,
    pub handbrake: bool,
    pub nitro: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Upgrades {
    /// Multipliers on the shipped values: engine torque, steering response, grip,
    /// damage resistance. Levels are the port's progression (see README).
    pub engine: f32,
    pub steer: f32,
    pub grip: f32,
    pub durability: f32,
}

impl Upgrades {
    pub fn stock() -> Self {
        Self {
            engine: 1.0,
            steer: 1.0,
            grip: 1.0,
            durability: 1.0,
        }
    }

    pub fn engine_level(level: u8) -> f32 {
        1.0 + 0.08 * level as f32
    }

    pub fn steer_level(level: u8) -> f32 {
        1.0 + 0.05 * level as f32
    }

    pub fn grip_level(level: u8) -> f32 {
        1.0 + 0.06 * level as f32
    }

    pub fn durability_level(level: u8) -> f32 {
        1.0 + 0.10 * level as f32
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct WheelState {
    pub contact: bool,
    pub compression: f32,
    pub angular_vel: f32,
    pub slip: f32,
    pub lateral_slip: f32,
    pub load: f32,
}

#[derive(Clone, Debug)]
pub struct CarSim {
    pub spec: &'static Car,
    pub pos: Vec3,
    pub yaw: f32,
    pub vel: Vec3,
    pub yaw_rate: f32,
    pub pitch: f32,
    pub roll: f32,
    pub wheel_y: [f32; 4],
    pub wheels: [WheelState; 4],
    pub gear: usize,
    pub rpm: f32,
    pub cut_off: f32,
    pub nitro: f32,
    pub nitro_timer: f32,
    pub damage: f32,
    pub destroyed: bool,
    pub off_road: f32,
    pub distance: f32,
    pub top_speed: f32,
    pub upgrades: Upgrades,
}

impl CarSim {
    pub fn new(spec: &'static Car, pos: Vec3, yaw: f32, upgrades: Upgrades) -> Self {
        Self {
            spec,
            pos,
            yaw,
            vel: Vec3::ZERO,
            yaw_rate: 0.0,
            pitch: 0.0,
            roll: 0.0,
            wheel_y: [0.0; 4],
            wheels: [WheelState::default(); 4],
            gear: 0,
            rpm: spec.min_rpm,
            cut_off: 0.0,
            nitro: spec
                .max_nitro
                .max(1.0)
                .min(spec.min_nitro.max(0.0))
                .max(0.0),
            nitro_timer: 0.0,
            damage: 0.0,
            destroyed: false,
            off_road: 0.0,
            distance: 0.0,
            top_speed: 0.0,
            upgrades,
        }
    }

    pub fn max_damage(&self) -> f32 {
        // PG.VehicleDamageController.MaxDamage, scaled by the durability upgrade.
        2000.0 * self.upgrades.durability
    }

    pub fn wheel_world(&self, i: usize) -> Vec3 {
        let w = &self.spec.wheels[i];
        let local = vec3(w.pos[0], w.pos[1], w.pos[2]);
        let (s, c) = self.yaw.sin_cos();
        let x = local.x * c - local.z * s;
        let z = local.x * s + local.z * c;
        self.pos + vec3(x, local.y, z)
    }

    pub fn forward(&self) -> Vec3 {
        let (s, c) = self.yaw.sin_cos();
        vec3(s, 0.0, c)
    }

    pub fn speed(&self) -> f32 {
        self.vel.length()
    }

    /// Direction of travel in degrees of heading, used by AI and camera.
    pub fn heading(&self) -> f32 {
        self.yaw
    }

    pub fn step(&mut self, dt: f32, input: Input, track: &Track) {
        if self.destroyed {
            self.vel.y += GRAVITY * dt;
            self.pos += self.vel * dt;
            return;
        }
        let dt = dt.min(0.05);
        let spec = self.spec;
        let mut grounded = 0;
        let mut surface_y = f32::NEG_INFINITY;
        for i in 0..spec.wheels.len().min(4) {
            let w = &spec.wheels[i];
            let p = self.wheel_world(i);
            match track.surface(p.x, p.z) {
                Some(h) => {
                    let rest = w.suspension_distance;
                    let target = w.target_position.clamp(0.0, 1.0);
                    let centre_y = self.pos.y + w.pos[1];
                    let compression =
                        (h + (1.0 - target) * rest - (centre_y - rest)).clamp(0.0, rest * 1.4);
                    let prev = self.wheels[i].compression;
                    let rate = (compression - prev) / dt;
                    let force = (w.spring * compression - w.damper * rate).max(0.0);
                    self.wheels[i].contact = true;
                    self.wheels[i].compression = compression;
                    self.wheels[i].load = force;
                    grounded += 1;
                    surface_y = surface_y.max(h);
                }
                None => {
                    self.wheels[i].contact = false;
                    self.wheels[i].compression *= 0.9;
                    self.wheels[i].load = 0.0;
                }
            }
        }
        // Attitude follows the wheel contacts.
        if grounded >= 2 {
            let front = self.wheel_avg_y(track, true);
            let rear = self.wheel_avg_y(track, false);
            let target_pitch = ((front - rear) / spec.wheelbase()).atan();
            let left = self.wheel_avg_side_height(track, true);
            let right = self.wheel_avg_side_height(track, false);
            let target_roll = ((left - right) / spec.track_width()).atan();
            let k = 1.0 - (-8.0 * dt).exp();
            self.pitch += (target_pitch - self.pitch) * k;
            self.roll += (target_roll - self.roll) * k;
        }
        if grounded == 0 {
            self.vel.y += GRAVITY * dt;
        }

        let speed = self.speed();
        self.top_speed = self.top_speed.max(speed * 3.6);
        let forward = self.forward();
        let forward_speed = self.vel.dot(forward);

        // ---- engine
        let wheel_radius = spec.wheel_radius();
        let gears = spec.gear_count().max(1);
        let ratio = spec.gear_ratio(self.gear) * spec.final_drive.unwrap_or(3.5);
        let wheel_rpm = (forward_speed.abs() / (wheel_radius * std::f32::consts::TAU)) * 60.0;
        let target_rpm = wheel_rpm * ratio.max(0.5);
        let idle = spec.min_rpm.max(500.0);
        self.rpm += (target_rpm.max(idle) - self.rpm) * (1.0 - (-6.0 * dt).exp());

        if self.cut_off > 0.0 {
            self.cut_off -= dt;
        } else if self.rpm > spec.cut_off_rpm.max(spec.max_rpm * 0.9) {
            self.cut_off = spec.cut_off_time.max(0.05) + 0.15;
            self.rpm = spec.target_cut_off_rpm.min(self.rpm);
        }
        if self.rpm > spec.rpm_to_next_gear && self.gear + 1 < gears {
            self.gear += 1;
        } else if self.rpm < spec.min_rpm.max(900.0) && self.gear > 0 && forward_speed < 6.0 {
            self.gear -= 1;
        }
        if self.gear >= gears {
            self.gear = gears - 1;
        }

        let torque_mult = spec.torque_at(self.rpm);
        let cut = if self.cut_off > 0.0 { 0.15 } else { 1.0 };
        let throttle = input.throttle.clamp(0.0, 1.0) * cut;
        // `EngineConfig.MaxMotorTorque` is the figure Unity hands each driven
        // `WheelCollider`, and the shipped cars drive three or four of their four
        // wheels, so the axle total is that figure times the driven count.
        let drive = spec.max_motor_torque
            * DRIVEN_WHEELS as f32
            * self.upgrades.engine
            * torque_mult
            * ratio
            * throttle;
        let drive_force = drive / wheel_radius.max(0.05);

        // ---- nitro
        if input.nitro && self.nitro > spec.min_nitro.max(0.0) {
            self.nitro_timer = (self.nitro_timer - dt).max(0.0);
            self.nitro = (self.nitro - dt * 0.25).max(0.0);
        } else if self.nitro_timer <= 0.0 {
            self.nitro = (self.nitro + dt * 0.08).min(spec.max_nitro.max(1.0));
        }
        let boost = if self.nitro_timer > 0.0 {
            1.0 + spec.nitro_boost
        } else {
            1.0
        };
        if input.nitro && self.nitro > spec.min_nitro.max(0.0) && self.nitro_timer <= 0.0 {
            self.nitro_timer = spec.nitro_boost_duration.max(0.6);
        }

        // ---- longitudinal forces
        let mass = spec.mass;
        let mut brake_force = input.brake.clamp(0.0, 1.0) * mass * 12.0;
        if ABS_ENABLED && brake_force > 0.0 && !spec.wheels.is_empty() {
            // Antilock: back off while any wheel is past the friction peak.
            let peak = spec.wheels[0].fwd.slip.max(0.05);
            let slipping = self
                .wheels
                .iter()
                .take(4)
                .filter(|w| w.contact && w.slip > peak)
                .count();
            if slipping > 0 {
                let shed = 1.0 - 0.22 * slipping as f32;
                brake_force *= shed.clamp(0.35, 1.0);
            }
        }
        if TCS_ENABLED && input.throttle > 0.0 {
            // Traction control would cap drive by wheelspin; off in this build.
        }
        let handbrake = if input.handbrake { mass * 6.0 } else { 0.0 };
        let rolling = -forward_speed.signum() * 0.02 * mass;
        let slope = {
            let ahead = track.surface(self.pos.x + forward.x * 2.0, self.pos.z + forward.z * 2.0);
            let here = track.surface(self.pos.x, self.pos.z);
            match (ahead, here) {
                (Some(a), Some(b)) => ((a - b) / 2.0).clamp(-1.0, 1.0),
                _ => 0.0,
            }
        };
        let gravity_along = GRAVITY * mass * slope.sin();

        let accel =
            (drive_force * boost - brake_force - handbrake + rolling) / mass + gravity_along / mass;
        let mut new_forward = forward_speed + accel * dt;

        // ---- speed cap: SpeedLimit when set, otherwise the drag curve
        let cap = if spec.speed_limit > 1.0 {
            spec.speed_limit * self.upgrades.engine
        } else {
            // drag ~ 0.4 * v; tops out near 300 km/h scaled by the level's bots
            320.0
        };
        new_forward = new_forward.clamp(-14.0, cap);

        // ---- steering and lateral grip
        let steer_authority = self.steer_authority(speed);
        let max_angle = spec.max_steer_angle.unwrap_or(40.0_f32.to_radians());
        let angle =
            input.steer.clamp(-1.0, 1.0) * max_angle * steer_authority * self.upgrades.steer;
        let turn = if speed.abs() > 0.5 {
            new_forward * angle.tan() / spec.wheelbase()
        } else {
            0.0
        };
        let grip = spec
            .wheels
            .first()
            .map(|w| friction_at(&w.side_f, speed * 0.02))
            .unwrap_or(1.0)
            * self.upgrades.grip;
        let demanded = turn * speed.abs();
        let max_lat = grip * mass * 9.81 * 1.6 / mass;
        let (actual_turn, slide) = if demanded.abs() <= max_lat.abs().max(0.001) {
            (turn, 0.0)
        } else {
            (
                max_lat * demanded.signum() / speed.abs().max(0.001),
                demanded.abs() - max_lat.abs(),
            )
        };
        self.yaw_rate += (actual_turn - self.yaw_rate) * (1.0 - (-9.0 * dt).exp());
        self.yaw += self.yaw_rate * dt;

        // ---- integrate
        let fwd = self.forward();
        let right = vec3(fwd.z, 0.0, -fwd.x);
        let lateral_drift = -slide.min(120.0) * 0.0025;
        self.vel = fwd * new_forward + right * lateral_drift;
        if grounded == 0 {
            self.vel.y += GRAVITY * dt;
        } else {
            self.vel.y *= 0.2;
            if let Some(h) = track.surface(self.pos.x, self.pos.z) {
                self.pos.y = self.pos.y.max(h - 1.4);
            }
        }
        self.pos += self.vel * dt;
        self.distance = self.pos.z.max(self.distance);

        // ---- wheel spin + ground effects
        for i in 0..spec.wheels.len().min(4) {
            let ws = &mut self.wheels[i];
            let target = new_forward / wheel_radius.max(0.05);
            ws.angular_vel += (target - ws.angular_vel) * (1.0 - (-10.0 * dt).exp());
            let spec_w = &spec.wheels[i];
            ws.slip = if ws.contact {
                (ws.angular_vel * spec_w.radius - new_forward).abs() / new_forward.abs().max(1.0)
            } else {
                0.0
            };
            ws.lateral_slip = slide.abs();
        }

        // ---- off-road: the mountain side tears the car up rather than
        // ---- destroying it outright, so a near miss stays survivable.
        if track.surface(self.pos.x, self.pos.z).is_none() && !self.destroyed {
            self.off_road += dt;
            self.damage += OFF_ROAD_DAMAGE_PER_SEC * dt * speed.max(6.0) / 30.0;
            if self.damage >= self.max_damage() {
                self.destroyed = true;
            }
        } else {
            self.off_road = 0.0;
        }
    }

    /// Mean surface height under the wheels on one axle.
    fn wheel_avg_y(&self, track: &Track, front: bool) -> f32 {
        let mut sum = 0.0;
        let mut n = 0.0;
        for (i, w) in self.spec.wheels.iter().enumerate().take(4) {
            if (w.side == "front") == front {
                let p = self.wheel_world(i);
                if let Some(h) = track.surface(p.x, p.z) {
                    sum += h;
                    n += 1.0;
                }
            }
        }
        if n > 0.0 { sum / n } else { self.pos.y }
    }

    /// Mean surface height under the wheels on one side; the roll target is the
    /// difference between the two sides.
    fn wheel_avg_side_height(&self, track: &Track, left: bool) -> f32 {
        let mut sum = 0.0;
        let mut n = 0.0;
        for (i, w) in self.spec.wheels.iter().enumerate().take(4) {
            if (w.pos[0] < 0.0) == left {
                let p = self.wheel_world(i);
                if let Some(h) = track.surface(p.x, p.z) {
                    sum += h;
                    n += 1.0;
                }
            }
        }
        if n > 0.0 { sum / n } else { self.pos.y }
    }

    /// Steer authority from the recovered `SteerLimitCurve` shape: full lock at
    /// low speed, a fifth of it at 40 m/s.
    fn steer_authority(&self, speed: f32) -> f32 {
        let t = (speed / 40.0).clamp(0.0, 1.0);
        1.0 - 0.8 * t
    }
}
