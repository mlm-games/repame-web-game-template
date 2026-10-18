//! Progression: wallet, unlocks, upgrades, results history and the save file.

use bevy_ecs::prelude::Resource;
use repame_save::{AppId, SaveFile};
use serde::{Deserialize, Serialize};

use crate::sim::car::Upgrades;
use crate::sim::data;
use crate::sim::race::Results;

const APP: AppId = AppId::new("com.mlm-games.deadly-descent");
const SAVE_FILE: &str = "save.ron";
const SCHEMA: u32 = 1;

pub const CLOUD_KEY: &str = "deadly_descent_save";

const MAX_UPGRADE: u8 = 10;

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct CarState {
    pub id: String,
    pub engine: u8,
    pub steer: u8,
    pub durability: u8,
}

/// The shipped `LogInRewardTracker` pays for consecutive days of play; the amount
/// curve is in code, so these are DESIGNED against the level rewards.
pub const LOGIN_REWARDS: [i32; 7] = [200, 350, 500, 800, 1200, 2000, 3500];

/// Coin packs from the bridge config (`coinPack0_2` … `coinPack3_2`), with the
/// coin amounts DESIGNED against the reward curve. `no_ads` is an entitlement.
pub const COIN_PACKS: [(&str, i32); 4] = [
    ("coinPack0_2", 5_000),
    ("coinPack1_2", 10_000),
    ("coinPack2_2", 25_000),
    ("coinPack3_2", 60_000),
];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SaveData {
    #[serde(default)]
    pub coins: i32,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub no_ads: bool,
    #[serde(default)]
    pub selected_car: i32,
    #[serde(default)]
    pub owned_cars: Vec<String>,
    #[serde(default)]
    pub completed: Vec<usize>,
    #[serde(default)]
    pub best_time: Vec<f32>,
    #[serde(default)]
    pub best_score: Vec<i32>,
    /// Best speed in km/h per level, the readout the HUD shows beside the target.
    #[serde(default)]
    pub best_speed: Vec<f32>,
    #[serde(default)]
    pub cars: Vec<CarState>,
    /// Day index (days since the Unix epoch, local) of the last claimed login
    /// reward, and the streak it was part of.
    #[serde(default)]
    pub last_login_day: i64,
    #[serde(default)]
    pub login_streak: u32,
    /// Tutorial progress, matching the shipped steps: 0 = highlight RACE,
    /// 1 = highlight a map, 2 = prompt nitro in the first race, 3 = done.
    #[serde(default)]
    pub tutorial_stage: u8,
}

impl Default for SaveData {
    fn default() -> Self {
        let mut owned = Vec::new();
        for c in data::cars() {
            if c.purchase_type == 0 {
                owned.push(c.id.clone());
            }
        }
        if owned.is_empty()
            && let Some(first) = data::cars().first()
        {
            owned.push(first.id.clone());
        }
        Self {
            coins: 0,
            muted: false,
            no_ads: false,
            selected_car: 0,
            owned_cars: owned,
            completed: Vec::new(),
            best_time: Vec::new(),
            best_score: Vec::new(),
            best_speed: Vec::new(),
            cars: Vec::new(),
            last_login_day: 0,
            login_streak: 0,
            tutorial_stage: 0,
        }
    }
}

impl SaveData {
    pub fn car_state(&self, id: &str) -> CarState {
        self.cars
            .iter()
            .find(|c| c.id == id)
            .cloned()
            .unwrap_or_else(|| CarState {
                id: id.to_string(),
                engine: 0,
                steer: 0,
                durability: 0,
            })
    }

    pub fn owns(&self, id: &str) -> bool {
        self.owned_cars.iter().any(|c| c == id)
    }

    pub fn completed(&self, level: usize) -> bool {
        self.completed.contains(&level)
    }

    pub fn unlocked(&self, level_index: usize) -> bool {
        let levels = data::levels();
        if level_index == 0 {
            return true;
        }
        let level = &levels[level_index.min(levels.len() - 1)];
        if level.cost_to_unlock <= 0 {
            return true;
        }
        // A level opens when the previous one is completed and the fee is paid;
        // the fee is charged on selection (see `try_open_level`).
        self.completed(level_index.saturating_sub(1)) || self.completed(level_index)
    }

    pub fn best_time(&self, level: usize) -> Option<f32> {
        self.best_time.get(level).copied().filter(|t| *t > 0.0)
    }

    pub fn best_score(&self, level: usize) -> Option<i32> {
        self.best_score.get(level).copied()
    }

    pub fn best_speed(&self, level: usize) -> Option<f32> {
        self.best_speed.get(level).copied().filter(|s| *s > 0.0)
    }
}

/// Upgrade step price. The build does not expose the price curve (`Game.PricesConfig`
/// carries `CarPrices` only and no UpgradePrices table was found), so this is DESIGNED
/// to match the reward magnitudes in `levels.json`.
pub fn upgrade_price(kind: UpgradeKind, level: u8) -> i32 {
    let base = match kind {
        UpgradeKind::Engine => 2500,
        UpgradeKind::Steer => 1800,
        UpgradeKind::Durability => 1500,
    };
    let step = level as i32;
    base * (1 + step) * (1 + step)
}

/// Car price. `CarShopItemConfig.purchaseType` decides how a car is acquired, but the
/// coin price itself was not in the build; these are DESIGNED against the reward curve.
pub fn car_price(car_index: usize) -> i32 {
    match car_index {
        0 => 0,
        1 => 60_000,
        2 => 150_000,
        3 => 300_000,
        4 => 600_000,
        5 => 1_200_000,
        6 => 2_500_000,
        7 => 5_000_000,
        8 => 10_000_000,
        _ => 20_000_000,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpgradeKind {
    Engine,
    Steer,
    Durability,
}

#[derive(Resource, Clone, Debug)]
pub struct Progress {
    pub data: SaveData,
    pub cloud_dirty: bool,
    store: SaveFile,
}

impl Default for Progress {
    fn default() -> Self {
        Self::new()
    }
}

impl Progress {
    pub fn new() -> Self {
        let mut save = Self {
            data: SaveData::default(),
            cloud_dirty: false,
            store: SaveFile::new(APP, SAVE_FILE, SCHEMA),
        };
        match save.store.load::<SaveData>() {
            Ok(Some(loaded)) => save.data = loaded,
            Ok(None) => {}
            Err(error) => log::warn!("deadly-descent: save load failed: {error}"),
        }
        if save.data.owned_cars.is_empty() {
            save.data = SaveData::default();
        }
        save
    }

    pub fn persist(&mut self) {
        if let Err(error) = self.store.save(&self.data) {
            log::warn!("deadly-descent: save write failed: {error}");
        }
        self.cloud_dirty = true;
    }

    pub fn coins(&self) -> i32 {
        self.data.coins
    }

    pub fn add_coins(&mut self, amount: i32) {
        self.data.coins = (self.data.coins + amount).max(0);
    }

    pub fn selected_car(&self) -> usize {
        self.data.selected_car.max(0) as usize
    }

    pub fn select_car(&mut self, index: usize) {
        if self.data.owns(data::car_at(index as i32).id.as_str()) {
            self.data.selected_car = index as i32;
        }
    }

    pub fn upgrades_for(&self, car_id: &str) -> Upgrades {
        let state = self.data.car_state(car_id);
        Upgrades {
            engine: Upgrades::engine_level(state.engine.min(MAX_UPGRADE)),
            steer: Upgrades::steer_level(state.steer.min(MAX_UPGRADE)),
            grip: Upgrades::grip_level(state.steer.min(MAX_UPGRADE)),
            durability: Upgrades::durability_level(state.durability.min(MAX_UPGRADE)),
        }
    }

    pub fn upgrade_level(&self, car_id: &str, kind: UpgradeKind) -> u8 {
        let state = self.data.car_state(car_id);
        match kind {
            UpgradeKind::Engine => state.engine,
            UpgradeKind::Steer => state.steer,
            UpgradeKind::Durability => state.durability,
        }
    }

    pub fn buy_upgrade(&mut self, car_id: &str, kind: UpgradeKind) -> bool {
        let level = self.upgrade_level(car_id, kind);
        if level >= MAX_UPGRADE {
            return false;
        }
        let price = upgrade_price(kind, level);
        if self.data.coins < price {
            return false;
        }
        self.data.coins -= price;
        let entry = match self.data.cars.iter_mut().find(|c| c.id == car_id) {
            Some(entry) => entry,
            None => {
                self.data.cars.push(CarState {
                    id: car_id.to_string(),
                    engine: 0,
                    steer: 0,
                    durability: 0,
                });
                self.data.cars.last_mut().unwrap()
            }
        };
        match kind {
            UpgradeKind::Engine => entry.engine += 1,
            UpgradeKind::Steer => entry.steer += 1,
            UpgradeKind::Durability => entry.durability += 1,
        }
        self.persist();
        true
    }

    /// Buy a car with coins when its shop entry is a coin purchase.
    pub fn buy_car(&mut self, index: usize) -> bool {
        let car = data::car_at(index as i32);
        if self.data.owns(&car.id) {
            return true;
        }
        if car.purchase_type != 1 {
            return false;
        }
        let price = car_price(index);
        if self.data.coins < price {
            return false;
        }
        self.data.coins -= price;
        self.data.owned_cars.push(car.id.clone());
        self.persist();
        true
    }

    /// Unlock a level by paying `costToUnlock`, as `Game.Level` describes.
    pub fn try_open_level(&mut self, level_index: usize) -> bool {
        let levels = data::levels();
        let level = &levels[level_index.min(levels.len() - 1)];
        if level.cost_to_unlock <= 0 || self.data.completed(level_index) {
            return true;
        }
        if self.data.coins < level.cost_to_unlock {
            return false;
        }
        self.data.coins -= level.cost_to_unlock;
        self.persist();
        true
    }

    pub fn record_results(&mut self, level_index: usize, results: &Results) {
        let first = !self.data.completed(level_index);
        let scaled = if first {
            results.coins
        } else {
            (results.completion_bonus as f32 / 2.0) as i32
        };
        let mut coins = scaled;
        if let Some(prev) = self.data.best_score(level_index) {
            let delta = results.score - prev;
            if delta > 0 {
                coins += delta / 4;
            }
        }
        self.add_coins(coins);
        if first {
            self.data.completed.push(level_index);
        }
        while self.data.best_score.len() <= level_index {
            self.data.best_score.push(0);
        }
        while self.data.best_time.len() <= level_index {
            self.data.best_time.push(0.0);
        }
        let score = self.data.best_score[level_index];
        if results.score > score {
            self.data.best_score[level_index] = results.score;
        }
        let time = self.data.best_time[level_index];
        if results.finish_time > 0.0 && (time <= 0.0 || results.finish_time < time) {
            self.data.best_time[level_index] = results.finish_time;
        }
        while self.data.best_speed.len() <= level_index {
            self.data.best_speed.push(0.0);
        }
        if results.top_speed_kmh > self.data.best_speed[level_index] {
            self.data.best_speed[level_index] = results.top_speed_kmh;
        }
        self.persist();
    }

    /// Advance the tutorial when the step it highlights is done.
    pub fn advance_tutorial(&mut self, from_stage: u8) {
        if self.data.tutorial_stage == from_stage {
            self.data.tutorial_stage = from_stage + 1;
            self.persist();
        }
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.data.muted = muted;
        self.persist();
    }

    pub fn set_no_ads(&mut self, no_ads: bool) {
        self.data.no_ads = no_ads;
        self.persist();
    }

    /// Days with a claimable login reward: none on the day it was claimed, and
    /// the streak resets after a skipped day.
    pub fn login_reward_available(&self, today: i64) -> Option<(u32, i32)> {
        if today <= self.data.last_login_day {
            return None;
        }
        let continued = today == self.data.last_login_day + 1 && self.data.last_login_day > 0;
        let streak = if continued {
            self.data.login_streak + 1
        } else {
            1
        };
        let index = ((streak - 1) as usize) % LOGIN_REWARDS.len();
        Some((streak, LOGIN_REWARDS[index]))
    }

    /// Claim the day's reward; returns the coins granted.
    pub fn claim_login_reward(&mut self, today: i64) -> Option<i32> {
        let (streak, coins) = self.login_reward_available(today)?;
        self.data.last_login_day = today;
        self.data.login_streak = streak;
        self.add_coins(coins);
        self.persist();
        Some(coins)
    }

    /// Spend `price` coins; false when the wallet is short.
    pub fn spend(&mut self, price: i32) -> bool {
        if self.data.coins < price {
            return false;
        }
        self.data.coins -= price;
        self.persist();
        true
    }

    pub fn grant_coins(&mut self, amount: i32) {
        self.add_coins(amount);
        self.persist();
    }

    /// Coins in a coin pack id, if it is one of the shipped packs.
    pub fn pack_coins(id: &str) -> Option<i32> {
        COIN_PACKS
            .iter()
            .find(|(pack, _)| *pack == id)
            .map(|(_, coins)| *coins)
    }

    pub fn adopt_cloud(&mut self, payload: &serde_json::Value) -> bool {
        match serde_json::from_value::<SaveData>(payload.clone()) {
            Ok(data) => {
                if data.coins >= self.data.coins
                    && data.completed.len() >= self.data.completed.len()
                {
                    self.data = data;
                    self.persist();
                    return true;
                }
                false
            }
            Err(_) => false,
        }
    }
}
