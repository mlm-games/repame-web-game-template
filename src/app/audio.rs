use repame_audio::{
    AmplitudeEnvelope, Audio, AudioChannel, CueDef, CueSpec, NoiseShape, Sweep, Variation,
    Waveform, db_to_linear,
};

use crate::sim::state::SfxCue;
use repame_sim::rng::{Rng as _, SimRng};

const RATE: u32 = 22050;
const VARIATIONS: usize = 2;
const SFX_DB: f32 = -6.0;

/// The tuning table's `decay` is a rate on the cue's own timeline: the envelope
/// falls to `1/e` after `1 / (DECAY_RATE * decay)` of the cue.
const DECAY_RATE: f32 = 40.0;

impl SfxCue {
    pub fn name(self) -> &'static str {
        match self {
            Self::Pickup => "pickup",
            Self::Ui => "ui",
            Self::Death => "death",
            Self::Revive => "revive",
        }
    }

    const ALL: [SfxCue; 4] = [SfxCue::Pickup, SfxCue::Ui, SfxCue::Death, SfxCue::Revive];

    fn spec(self, jitter: f32, noise_seed: u64) -> CueSpec {
        let (frames, from_hz, to_hz, decay, noise) = match self {
            Self::Pickup => (2200.0, 880.0, 1320.0, 0.16, 0.0),
            Self::Ui => (1600.0, 440.0, 660.0, 0.10, 0.0),
            Self::Death => (7000.0, 320.0, 90.0, 0.55, 0.35),
            Self::Revive => (6000.0, 330.0, 990.0, 0.45, 0.0),
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

pub struct AudioDirector {
    audio: Audio,
    muted: bool,
}

impl AudioDirector {
    pub fn new(muted: bool) -> Self {
        let mut audio = Audio::try_init().unwrap_or_else(|_| Audio::noop());
        install_audio(&mut audio);
        let mut director = Self {
            audio,
            muted: false,
        };
        director.set_muted(muted);
        director
    }

    pub fn play(&mut self, cue: SfxCue) {
        self.audio.play(cue.name());
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

fn install_audio(audio: &mut Audio) {
    for cue in SfxCue::ALL {
        let specs: Vec<CueSpec> = (0..VARIATIONS)
            .map(|v| {
                let seed = 0xB1EE + cue as u64 * 691 + v as u64 * 7919;
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
            log::warn!("web-game: cue {}: {error}", cue.name());
        }
    }
}
