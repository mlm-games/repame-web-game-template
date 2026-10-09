//! Procedural sfx, synthesised into WAVs at startup and registered as
//! cues, so the game ships no sound files. Held by the app, not the sim.

use repame_audio::{Audio, AudioChannel, CueDef, Variation, db_to_linear};

use crate::sim::rng::Rng;
use crate::sim::state::SfxCue;

const RATE: u32 = 22050;
const VARIATIONS: usize = 2;
const SFX_DB: f32 = -6.0;

impl SfxCue {
    /// The cue name, the key the audio service stores it under.
    pub fn name(self) -> &'static str {
        match self {
            Self::Pickup => "pickup",
            Self::Ui => "ui",
            Self::Death => "death",
            Self::Revive => "revive",
        }
    }

    const ALL: [SfxCue; 4] = [SfxCue::Pickup, SfxCue::Ui, SfxCue::Death, SfxCue::Revive];
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

/// One exponential-decaying blip: frequency glide and decay length per cue.
fn render_blip(cue: SfxCue, rng: &mut Rng) -> Vec<f32> {
    let (frames, f0, f1, decay, noise) = match cue {
        SfxCue::Pickup => (2200, 880.0, 1320.0, 0.16, 0.0),
        SfxCue::Ui => (1600, 440.0, 660.0, 0.10, 0.0),
        SfxCue::Death => (7000, 320.0, 90.0, 0.55, 0.35),
        // A rising arpeggio, not a single glide: the revive should read as a
        // second wind, not a blip.
        SfxCue::Revive => (6000, 330.0, 990.0, 0.45, 0.0),
    };
    let jitter = 0.98 + 0.04 * rng.randf();
    let mut out = Vec::with_capacity(frames);
    let mut phase = 0.0f32;
    for i in 0..frames {
        let t = i as f32 / frames as f32;
        let env = (-decay * 40.0 * t).exp();
        let freq = (f0 + (f1 - f0) * t) * jitter;
        phase += std::f32::consts::TAU * freq / RATE as f32;
        let mut s = phase.sin() * env;
        if noise > 0.0 {
            s += (rng.randf() * 2.0 - 1.0) * noise * env * env;
        }
        out.push(s.clamp(-1.0, 1.0));
    }
    out
}

fn encode_wav(samples: &[f32], rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + samples.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes());
    }
    out
}

fn install_audio(audio: &mut Audio) {
    for cue in SfxCue::ALL {
        let files: Vec<Vec<u8>> = (0..VARIATIONS)
            .map(|v| {
                let mut rng = Rng::new(0xB1EE + cue as u64 * 691 + v as u64 * 7919);
                encode_wav(&render_blip(cue, &mut rng), RATE)
            })
            .collect();
        let refs: Vec<&[u8]> = files.iter().map(|b| b.as_slice()).collect();
        let def = CueDef {
            bus: AudioChannel::Sfx,
            gain: db_to_linear(SFX_DB),
            cooldown_ms: 0,
            max_voices: 6,
            pitch_wobble: 0.0,
            variation: Variation::RoundRobin,
        };
        let _ = audio.load_cue(cue.name(), def, &refs);
    }
}
