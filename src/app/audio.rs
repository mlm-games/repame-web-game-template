//! Audio: the shipped engine beds plus race cues.
//!
//! The reference's engine sound is a stack of rpm-banded loops
//! (`SkrilStudio.RealisticEngineSound`): idle, low/low-med/med/med-high/high/very-high
//! and maxRPM, each with a throttle-on and a throttle-off variant, plus a wind bed.
//! Each car's beds live in `StreamingAssets/CarEngines/<engine>/` and are loaded here
//! (Vorbis), crossfaded by rpm and throttle. Cues are synthesised so the game is
//! audible without the reference export.

use std::collections::HashMap;
use std::path::PathBuf;

use repame_audio::{
    AmplitudeEnvelope, Audio, AudioChannel, CueDef, CueSpec, NoiseShape, Sweep, Variation,
    Waveform, db_to_linear,
};
use repame_sim::rng::{Rng as _, SimRng};

use crate::sim::race::Cue;

const ENGINE_DB: f32 = -10.0;
const WIND_DB: f32 = -22.0;
const SFX_DB: f32 = -7.0;
const RATE: u32 = 22050;

/// The tuning table's `decay` is a rate on the cue's own timeline: the envelope
/// falls to `1/e` after `1 / (DECAY_RATE * decay)` of the cue.
const DECAY_RATE: f32 = 30.0;

/// The eight rpm tiers the reference stacks, in rising order.
const TIERS: [&str; 8] = [
    "idle",
    "low",
    "low_med",
    "med",
    "med_high",
    "high",
    "very_high",
    "maxRPM",
];

pub struct AudioDirector {
    audio: Audio,
    muted: bool,
    /// Voices for the currently loaded engine, one per (tier, throttle state).
    engine_voices: Vec<(String, u64)>,
    wind_voice: Option<u64>,
    loaded_dir: Option<String>,
    last_gains: HashMap<String, f32>,
}

fn streaming_engine_dir(engine: &str) -> Option<PathBuf> {
    let root = crate::art::art_root()?;
    let path = root
        .parent()?
        .join("StreamingAssets")
        .join("CarEngines")
        .join(engine);
    path.is_dir().then_some(path)
}

impl AudioDirector {
    pub fn new(muted: bool) -> Self {
        let mut audio = Audio::try_init().unwrap_or_else(|_| Audio::noop());
        install_cues(&mut audio);
        let mut director = Self {
            audio,
            muted: false,
            engine_voices: Vec::new(),
            wind_voice: None,
            loaded_dir: None,
            last_gains: HashMap::new(),
        };
        director.set_muted(muted);
        director
    }

    /// Load (once) the engine beds for a car's engine directory.
    pub fn set_engine(&mut self, engine: Option<&str>) {
        let engine = engine.unwrap_or("");
        if self.loaded_dir.as_deref() == Some(engine) {
            return;
        }
        if engine.is_empty() {
            self.loaded_dir = Some(String::new());
            return;
        }
        let Some(dir) = streaming_engine_dir(engine) else {
            // No export: keep whatever is playing.
            self.loaded_dir = Some(engine.to_string());
            return;
        };
        // Drop the previous engine's loops.
        for (_, voice) in self.engine_voices.drain(..) {
            self.audio.fade_voice(voice, 0.0, 0.12);
        }
        if let Some(voice) = self.wind_voice.take() {
            self.audio.fade_voice(voice, 0.0, 0.12);
        }
        for tier in TIERS {
            for (suffix, name) in [
                ("on", format!("{tier}_on.ogg")),
                ("off", format!("{tier}_off.ogg")),
            ] {
                let Some(path) = dir_variant(&dir, tier, suffix) else {
                    continue;
                };
                let cue = format!("eng:{engine}:{tier}_{suffix}");
                if let Ok(bytes) = std::fs::read(&path) {
                    let def = CueDef {
                        bus: AudioChannel::Sfx,
                        gain: db_to_linear(ENGINE_DB),
                        cooldown_ms: 0,
                        max_voices: 1,
                        pitch_wobble: 0.0,
                        variation: Variation::RoundRobin,
                    };
                    if self.audio.load_cue(&cue, def, &[bytes.as_slice()]).is_ok()
                        && let Some(voice) = self.audio.play_loop(&cue, 0.0, 1.0)
                    {
                        self.engine_voices.push((format!("{tier}:{name}"), voice));
                    }
                }
            }
        }
        // wind bed sits on top of the tiers
        let wind = dir.join("wind_1.ogg");
        if let Ok(bytes) = std::fs::read(&wind) {
            let def = CueDef {
                bus: AudioChannel::Sfx,
                gain: db_to_linear(WIND_DB),
                cooldown_ms: 0,
                max_voices: 1,
                pitch_wobble: 0.0,
                variation: Variation::RoundRobin,
            };
            let cue = format!("eng:{engine}:wind");
            if self.audio.load_cue(&cue, def, &[bytes.as_slice()]).is_ok() {
                self.wind_voice = self.audio.play_loop(&cue, 0.0, 1.0);
            }
        }
        self.loaded_dir = Some(engine.to_string());
    }

    /// Crossfade the rpm tiers by rpm fraction and throttle, and the wind by speed.
    pub fn engine(&mut self, rpm_frac: f32, throttle: f32, speed_kmh: f32) {
        let rpm = rpm_frac.clamp(0.0, 1.0);
        let on = throttle.clamp(0.0, 1.0);
        // Triangular window per tier: the tier's own band plus the two neighbours.
        let bands = TIERS.len() as f32;
        let centre = rpm * (bands - 1.0);
        for (key, voice) in &self.engine_voices {
            let Some((tier, state)) = key.split_once(':') else {
                continue;
            };
            let index = TIERS.iter().position(|t| *t == tier).unwrap_or(0) as f32;
            let distance = (centre - index).abs();
            let window = (1.0 - distance).clamp(0.0, 1.0);
            let throttle_mix = if state == "on" {
                on.max(0.15)
            } else {
                1.0 - on * 0.85
            };
            let gain = window * throttle_mix;
            let last = self.last_gains.entry(key.clone()).or_insert(-1.0);
            if (gain - *last).abs() > 0.01 {
                *last = gain;
                self.audio.fade_voice(*voice, gain, 0.08);
                self.audio.set_voice_rate(*voice, 0.85 + rpm * 0.5);
            }
        }
        if let Some(voice) = self.wind_voice {
            let gain = ((speed_kmh - 40.0) / 200.0).clamp(0.0, 1.0);
            let last = self.last_gains.entry("wind".to_string()).or_insert(-1.0);
            if (gain - *last).abs() > 0.02 {
                *last = gain;
                self.audio.fade_voice(voice, gain, 0.15);
                self.audio.set_voice_rate(voice, 0.9 + gain * 0.4);
            }
        }
    }

    pub fn play(&mut self, cue: Cue) {
        self.audio.play(cue.name());
        if cue == Cue::Crash {
            self.audio.play("break");
        }
    }

    pub fn update(&mut self, dt: f32) {
        self.audio.update(dt);
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
        self.audio
            .set_channel(AudioChannel::Master, if muted { 0.0 } else { 1.0 });
    }

    pub fn muted(&self) -> bool {
        self.muted
    }
}

/// Some tiers ship without an explicit `_on` variant; fall back to the base name.
fn dir_variant(dir: &std::path::Path, tier: &str, suffix: &str) -> Option<PathBuf> {
    let named = dir.join(format!("{tier}_{suffix}.ogg"));
    if named.is_file() {
        return Some(named);
    }
    None
}

impl Cue {
    pub fn name(self) -> &'static str {
        match self {
            Cue::Countdown => "countdown",
            Cue::Go => "go",
            Cue::Crash => "crash",
            Cue::Break => "break",
            Cue::Checkpoint => "checkpoint",
            Cue::Finish => "finish",
            Cue::Fail => "fail",
            Cue::Coin => "coin",
            Cue::Click => "click",
        }
    }

    const ALL: [Cue; 9] = [
        Cue::Countdown,
        Cue::Go,
        Cue::Crash,
        Cue::Break,
        Cue::Checkpoint,
        Cue::Finish,
        Cue::Fail,
        Cue::Coin,
        Cue::Click,
    ];

    /// One exponential-decaying sweep with an optional noise layer.
    fn spec(self, jitter: f32, noise_seed: u64) -> CueSpec {
        let (frames, from_hz, to_hz, decay, noise) = match self {
            Self::Countdown => (2600.0, 440.0, 440.0, 0.10, 0.0),
            Self::Go => (4200.0, 880.0, 1320.0, 0.22, 0.0),
            Self::Crash => (9000.0, 220.0, 60.0, 0.75, 0.55),
            Self::Break => (5200.0, 620.0, 180.0, 0.5, 0.3),
            Self::Checkpoint => (3000.0, 660.0, 990.0, 0.2, 0.0),
            Self::Finish => (12000.0, 520.0, 1560.0, 0.75, 0.05),
            Self::Fail => (11000.0, 400.0, 60.0, 0.8, 0.25),
            Self::Coin => (2400.0, 1180.0, 1760.0, 0.18, 0.0),
            Self::Click => (1600.0, 560.0, 620.0, 0.1, 0.0),
        };
        let secs = frames / RATE as f32;
        // `noise` is the grit-to-tone amplitude ratio at the onset; a blend of
        // `mix` leaves those amplitudes at `(1 - mix) * gain` and `mix * gain`,
        // so the ratio is split across the blend and the spec's gain.
        let level = 1.0 + noise;
        let spec = CueSpec::new(Waveform::Sine, from_hz * jitter, secs)
            .with_sweep(to_hz * jitter, Sweep::Linear)
            .with_amplitude(AmplitudeEnvelope::Adsr {
                attack_s: 0.0,
                decay_s: secs / (DECAY_RATE * decay),
                sustain: 0.0,
                release_s: 0.0,
            })
            .with_noise(noise / level, noise_seed)
            .with_gain(level)
            .with_sample_rate(RATE);
        if noise > 0.0 {
            spec.with_noise_shape(NoiseShape::Squared)
        } else {
            spec
        }
    }
}

fn install_cues(audio: &mut Audio) {
    for cue in Cue::ALL {
        let specs: Vec<CueSpec> = (0..2)
            .map(|v| {
                let seed = 0xD00D + cue as u64 * 977 + v as u64 * 7919;
                let mut rng = SimRng::seeded(seed);
                cue.spec(0.98 + 0.04 * rng.unit_f32(), seed)
            })
            .collect();
        let def = CueDef {
            bus: AudioChannel::Sfx,
            gain: db_to_linear(SFX_DB),
            cooldown_ms: 0,
            max_voices: 6,
            pitch_wobble: 0.0,
            variation: Variation::RoundRobin,
        };
        if let Err(error) = audio.load_synth(cue.name(), def, &specs) {
            log::warn!("deadly-descent: cue {}: {error}", cue.name());
        }
    }
}
