//! Level and car data recovered from the shipped Unity build.
//!
//! `assets/dd1/levels.json` is the game's own 61-entry level table read out of
//! `Game.MapConfig` in `sharedassets0.assets`; `assets/dd1/cars.json` is the car
//! roster with the WheelCollider/Rigidbody values and the inline engine, steering
//! and gearbox tuning read out of `Game.UpgradableCarController` on each car
//! prefab. See the README for the extraction method and its limits.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Deserialize;

pub const LEVELS_JSON: &str = include_str!("../../assets/dd1/levels.json");
pub const CARS_JSON: &str = include_str!("../../assets/dd1/cars.json");
pub const AI_JSON: &str = include_str!("../../assets/dd1/ai.json");
pub const ENGINE_DIRS_JSON: &str = include_str!("../../assets/dd1/engine_dirs.json");

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BotConfig {
    pub car: i32,
    pub skill: i32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Level {
    pub scene: String,
    pub icon: String,
    pub name: String,
    pub max_possible_score: i32,
    pub cost_to_unlock: i32,
    pub require_checkpoints: bool,
    pub show_container_tutorial: bool,
    pub score_for_completion: i32,
    pub reward_for_completion: i32,
    pub second_completion_reward_multiplier: f32,
    pub speed_to_get_reward: i32,
    pub reward_for_speed: i32,
    pub time_to_get_reward: i32,
    pub reward_for_time: i32,
    pub reward_for_max_damage: i32,
    pub recommended_car: i32,
    pub bot_configs: Vec<BotConfig>,
    pub max_bot_speed: f32,
    pub racing_track_size: i32,
    pub racing_track_seed: i32,
}

impl Level {
    /// Scene file name without the folder and extension, e.g. `Start`, `RacingTrack`.
    pub fn segment(&self) -> &str {
        let tail = self.scene.rsplit('/').next().unwrap_or(&self.scene);
        tail.strip_suffix(".unity").unwrap_or(tail)
    }

    pub fn is_racing_track(&self) -> bool {
        self.segment() == "RacingTrack"
    }

    pub fn icon_stem(&self) -> &str {
        self.icon.strip_suffix(".jpg").unwrap_or(&self.icon)
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Friction {
    pub slip: f32,
    pub value: f32,
    pub asymptote_slip: f32,
    pub asymptote_value: f32,
    pub stiffness: f32,
}

/// Sample WheelCollider's friction curve: rise to `value` at `slip`, then fall
/// toward `asymptote_value` at `asymptote_slip`, held beyond.
pub fn friction_at(c: &Friction, slip: f32) -> f32 {
    let s = slip.abs();
    let v = if s <= c.slip {
        if c.slip <= 1e-6 {
            c.value
        } else {
            c.value * (s / c.slip)
        }
    } else if s >= c.asymptote_slip {
        c.asymptote_value
    } else {
        let t = (s - c.slip) / (c.asymptote_slip - c.slip).max(1e-6);
        c.value + (c.asymptote_value - c.value) * t
    };
    v * c.stiffness
}

#[derive(Clone, Debug, Deserialize)]
pub struct WheelSpec {
    pub name: String,
    pub side: String,
    pub pos: [f32; 3],
    pub radius: f32,
    pub suspension_distance: f32,
    pub spring: f32,
    pub damper: f32,
    pub target_position: f32,
    pub fwd: Friction,
    pub side_f: Friction,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Car {
    pub id: String,
    pub display: String,
    pub icon: String,
    pub car_type: i32,
    pub purchase_type: i32,
    pub ads_required: i32,
    pub position: i32,
    pub mass: f32,
    pub drag: f32,
    pub angular_drag: f32,
    pub wheels: Vec<WheelSpec>,
    pub max_motor_torque: f32,
    pub max_rpm: f32,
    pub min_rpm: f32,
    pub rpm_fast: f32,
    pub rpm_slow: f32,
    pub speed_limit: f32,
    pub cut_off_rpm: f32,
    pub target_cut_off_rpm: f32,
    pub cut_off_time: f32,
    pub nitro_boost: f32,
    pub nitro_boost_duration: f32,
    pub min_nitro: f32,
    pub max_nitro: f32,
    pub rpm_to_next_gear: f32,
    pub rpm_to_prev_gear_diff: f32,
    /// `[rpm, torque multiplier]` pairs; rpm is absolute, multiplier unitless.
    pub torque_curve: Vec<[f32; 2]>,
    pub gears: Option<Vec<f32>>,
    pub final_drive: Option<f32>,
    pub max_steer_angle: Option<f32>,
    pub engines: Option<serde_json::Value>,
}

impl Car {
    pub fn gear_count(&self) -> usize {
        self.gears.as_ref().map(|g| g.len()).unwrap_or(6)
    }

    pub fn gear_ratio(&self, gear: usize) -> f32 {
        let gears = self.gears.as_deref().unwrap_or(&[]);
        let idx = gear.min(gears.len().saturating_sub(1));
        gears.get(idx).copied().unwrap_or(1.0)
    }

    /// Torque multiplier at an absolute rpm, from the recovered curve.
    pub fn torque_at(&self, rpm: f32) -> f32 {
        if self.torque_curve.is_empty() {
            return 1.0;
        }
        let mut prev = self.torque_curve[0];
        if rpm <= prev[0] {
            return prev[1];
        }
        for point in &self.torque_curve[1..] {
            if rpm <= point[0] {
                let span = (point[0] - prev[0]).max(1e-3);
                let t = (rpm - prev[0]) / span;
                return prev[1] + (point[1] - prev[1]) * t;
            }
            prev = *point;
        }
        self.torque_curve.last().unwrap()[1]
    }

    /// Wheel radius from the spec (all four wheels share it on every car).
    pub fn wheel_radius(&self) -> f32 {
        self.wheels.first().map(|w| w.radius).unwrap_or(0.32)
    }

    /// Distance between the front and rear axles, from the wheel positions.
    pub fn wheelbase(&self) -> f32 {
        let z: Vec<f32> = self.wheels.iter().map(|w| w.pos[2]).collect();
        let max = z.iter().cloned().fold(f32::MIN, f32::max);
        let min = z.iter().cloned().fold(f32::MAX, f32::min);
        (max - min).abs().max(1.0)
    }

    pub fn track_width(&self) -> f32 {
        let x: Vec<f32> = self.wheels.iter().map(|w| w.pos[0]).collect();
        let max = x.iter().cloned().fold(f32::MIN, f32::max);
        let min = x.iter().cloned().fold(f32::MAX, f32::min);
        (max - min).abs().max(0.5)
    }
}

/// `PG.BaseAIConfig` + `PG.RaceAIConfig`, inlined in the two
/// `PG.RaceAIConfigAsset`s the shipped scenes reference. Field names are the
/// build's own; the values are identical across cars.
#[derive(Clone, Debug, Deserialize)]
pub struct AiConfig {
    pub name: String,
    pub max_speed: f32,
    pub min_speed: f32,
    pub set_steer_angle_multiplayer: f32,
    pub offset_to_target_point: f32,
    pub speed_factor_to_target_point: f32,
    pub offset_turn_prediction: f32,
    pub speed_factor_to_turn_prediction: f32,
    pub look_angle_speed_factor: f32,
    pub reverse_wait_time: f32,
    pub reverse_time: f32,
    pub between_reverse_time_for_reset: f32,
    pub aggressiveness: f32,
    pub obstacle_hit_distance: f32,
    pub hit_point_height: f32,
    pub change_horizontal_offset_speed: f32,
    pub offset_for_main_hit_points: f32,
    pub offset_for_main_hit_directions: f32,
    pub offset_for_additional_hit_points: f32,
    pub main_hit_dellay_time: f32,
    pub additional_hit_dellay_time: f32,
}

/// The aggressive profile the racing levels reference; `precise` is the
/// conservative one used where the build points at it.
pub fn ai_config(precise: bool) -> &'static AiConfig {
    static CONFIG: OnceLock<(AiConfig, AiConfig)> = OnceLock::new();
    let both = CONFIG.get_or_init(|| {
        #[derive(Deserialize)]
        struct Both {
            #[serde(default)]
            default: Option<AiConfig>,
            #[serde(default)]
            precise: Option<AiConfig>,
        }
        match serde_json::from_str::<Both>(AI_JSON) {
            Ok(both) => (
                both.default.unwrap_or_else(fallback_ai_default),
                both.precise.unwrap_or_else(fallback_ai_precise),
            ),
            Err(error) => {
                eprintln!("deadly-descent: ai data failed to parse: {error}");
                (fallback_ai_default(), fallback_ai_precise())
            }
        }
    });
    if precise { &both.1 } else { &both.0 }
}

fn ai_config_fields(
    max_speed: f32,
    offset: f32,
    look: f32,
    aggressiveness: f32,
    name: &str,
) -> AiConfig {
    AiConfig {
        name: name.to_string(),
        max_speed,
        min_speed: 6.0,
        set_steer_angle_multiplayer: 2.0,
        offset_to_target_point: offset,
        speed_factor_to_target_point: 0.0,
        offset_turn_prediction: offset,
        speed_factor_to_turn_prediction: 2.0,
        look_angle_speed_factor: look,
        reverse_wait_time: 2.0,
        reverse_time: 2.0,
        between_reverse_time_for_reset: 6.0,
        aggressiveness,
        obstacle_hit_distance: 15.0,
        hit_point_height: 0.5,
        change_horizontal_offset_speed: 3.0,
        offset_for_main_hit_points: 0.7,
        offset_for_main_hit_directions: 0.8,
        offset_for_additional_hit_points: 1.2,
        main_hit_dellay_time: 0.4,
        additional_hit_dellay_time: 0.6,
    }
}

fn fallback_ai_default() -> AiConfig {
    ai_config_fields(1000.0, 20.0, 90.0, 0.7, "RaceAIConfig")
}

fn fallback_ai_precise() -> AiConfig {
    ai_config_fields(300.0, 10.0, 30.0, 0.4, "Precise RaceAIConfig")
}

fn parse<T: serde::de::DeserializeOwned>(json: &str, what: &str) -> Vec<T> {
    match serde_json::from_str::<Vec<T>>(json) {
        Ok(v) => v,
        Err(error) => {
            eprintln!("deadly-descent: {what} data failed to parse: {error}");
            log::error!("deadly-descent: {what} data failed to parse: {error}");
            Vec::new()
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct EngineDirs {
    #[serde(default)]
    pub exterior: Vec<String>,
    #[serde(default)]
    pub interior: Vec<String>,
}

/// The engine-sound directory a car's loader points at, read off the car prefab.
pub fn engine_dir(car_id: &str) -> Option<&'static str> {
    static TABLE: OnceLock<HashMap<String, EngineDirs>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        serde_json::from_str::<HashMap<String, EngineDirs>>(ENGINE_DIRS_JSON).unwrap_or_default()
    });
    table.get(car_id)?.exterior.first().map(|s| s.as_str())
}

pub fn levels() -> &'static [Level] {
    static LEVELS: OnceLock<Vec<Level>> = OnceLock::new();
    LEVELS.get_or_init(|| parse(LEVELS_JSON, "level"))
}

pub fn cars() -> &'static [Car] {
    static CARS: OnceLock<Vec<Car>> = OnceLock::new();
    CARS.get_or_init(|| parse(CARS_JSON, "car"))
}

pub fn car_at(index: i32) -> &'static Car {
    static FALLBACK: std::sync::OnceLock<Car> = std::sync::OnceLock::new();
    let all = cars();
    if all.is_empty() {
        return FALLBACK.get_or_init(default_car);
    }
    let i = (index.max(0) as usize).min(all.len() - 1);
    &all[i]
}

fn default_wheels() -> Vec<WheelSpec> {
    let spec = |name: &str, x: f32, z: f32| WheelSpec {
        name: name.to_string(),
        side: if z > 0.0 {
            "front".into()
        } else {
            "rear".into()
        },
        pos: [x, 0.33, z],
        radius: 0.32,
        suspension_distance: 0.25,
        spring: 36000.0,
        damper: 6400.0,
        target_position: 0.5,
        fwd: Friction {
            slip: 0.4,
            value: 1.5,
            asymptote_slip: 1.0,
            asymptote_value: 0.9,
            stiffness: 1.0,
        },
        side_f: Friction {
            slip: 0.4,
            value: 1.5,
            asymptote_slip: 0.8,
            asymptote_value: 0.9,
            stiffness: 1.0,
        },
    };
    vec![
        spec("FL", -0.8, 1.3),
        spec("FR", 0.8, 1.3),
        spec("RL", -0.8, -1.3),
        spec("RR", 0.8, -1.3),
    ]
}

/// Used only if `cars.json` fails to parse, so the sim never panics on data.
fn default_car() -> Car {
    Car {
        id: "fallback".into(),
        display: "Fallback".into(),
        icon: "FALLBACK".into(),
        car_type: 0,
        purchase_type: 0,
        ads_required: 0,
        position: 0,
        mass: 1500.0,
        drag: 0.02,
        angular_drag: 0.0,
        max_motor_torque: 200.0,
        max_rpm: 7000.0,
        min_rpm: 700.0,
        rpm_fast: 8.0,
        rpm_slow: 3.0,
        speed_limit: 0.0,
        cut_off_rpm: 6600.0,
        target_cut_off_rpm: 6000.0,
        cut_off_time: 0.05,
        nitro_boost: 1.0,
        nitro_boost_duration: 1.0,
        min_nitro: 0.0,
        max_nitro: 1.0,
        rpm_to_next_gear: 6500.0,
        rpm_to_prev_gear_diff: 1500.0,
        torque_curve: vec![[0.0, 0.81], [3360.0, 0.92], [6500.0, 0.99], [7000.0, 0.0]],
        wheels: default_wheels(),
        gears: Some(vec![3.9, 2.3, 1.6, 1.2, 0.94, 0.79]),
        final_drive: Some(3.5),
        max_steer_angle: None,
        engines: None,
    }
}

/// Level at `index`, clamped, or a safe default when the table is empty.
pub fn level_at(index: usize) -> Level {
    let all = levels();
    if all.is_empty() {
        return Level {
            scene: "RacingTrack".into(),
            icon: "RacingTrack.jpg".into(),
            name: "Fallback".into(),
            max_possible_score: 1000,
            cost_to_unlock: 0,
            require_checkpoints: false,
            show_container_tutorial: false,
            score_for_completion: 1000,
            reward_for_completion: 100,
            second_completion_reward_multiplier: 1.0,
            speed_to_get_reward: 150,
            reward_for_speed: 10,
            time_to_get_reward: 60,
            reward_for_time: 10,
            reward_for_max_damage: 10,
            recommended_car: 0,
            bot_configs: Vec::new(),
            max_bot_speed: 200.0,
            racing_track_size: 1,
            racing_track_seed: 1,
        };
    }
    all[index.min(all.len() - 1)].clone()
}
