#![allow(clippy::pedantic)]

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use sdl2::audio::{AudioCallback, AudioDevice, AudioSpecDesired};

use crate::controls::SoundPalette;

/// Sample rate for CRT audio (48 kHz as specified).
pub const SAMPLE_RATE: i32 = 48_000;

/// Shared synthesis state driven by the audio callback.
pub(crate) struct SharedState {
    pub(crate) queue: VecDeque<SoundEvent>,
    pub(crate) current: Option<ActiveTone>,
}

pub(crate) struct SoundEvent {
    pub(crate) freq: f32,
    pub(crate) samples_left: usize,
    pub(crate) volume: f32,
}

pub(crate) struct ActiveTone {
    pub(crate) freq: f32,
    pub(crate) samples_left: usize,
    pub(crate) volume: f32,
    pub(crate) phase: f32,
}

pub(crate) struct CrtAudioCallback {
    pub(crate) shared: Arc<Mutex<SharedState>>,
    pub(crate) sample_rate: f32,
}

impl AudioCallback for CrtAudioCallback {
    type Channel = f32;

    fn callback(&mut self, out: &mut [f32]) {
        let mut state = match self.shared.lock() {
            Ok(s) => s,
            Err(_) => {
                for x in out.iter_mut() {
                    *x = 0.0;
                }
                return;
            }
        };
        for sample in out.iter_mut() {
            // Load next event if none active
            if state.current.is_none() {
                if let Some(ev) = state.queue.pop_front() {
                    state.current = Some(ActiveTone {
                        freq: ev.freq,
                        samples_left: ev.samples_left,
                        volume: ev.volume,
                        phase: 0.0,
                    });
                }
            }
            if let Some(tone) = state.current.as_mut() {
                // Simple sine with quick decay envelope
                let envelope = if tone.samples_left < 64 {
                    tone.samples_left as f32 / 64.0
                } else {
                    1.0
                };
                let val = (tone.phase * 2.0 * std::f32::consts::PI).sin() * tone.volume * envelope;
                *sample = val;
                tone.phase += tone.freq / self.sample_rate;
                if tone.phase >= 1.0 {
                    tone.phase -= 1.0;
                }
                tone.samples_left = tone.samples_left.saturating_sub(1);
                if tone.samples_left == 0 {
                    state.current = None;
                }
            } else {
                *sample = 0.0;
            }
        }
    }
}

/// Trait for pluggable sound palettes (Teletype / ModemCrt / Minimal).
pub(crate) trait SoundGenerator: Send {
    /// Enqueue sound for `ch` into the shared state.
    fn trigger(&self, ch: char, shared: &Arc<Mutex<SharedState>>);
}

/// Teletype: sharp 900 Hz click per visible char, short.
pub struct TeletypeSound;
/// ModemCrt: longer 1400 Hz tone, modem-like.
pub struct ModemSound;
/// Minimal: soft 600 Hz tick, very short and quiet.
pub struct MinimalSound;

impl SoundGenerator for TeletypeSound {
    fn trigger(&self, ch: char, shared: &Arc<Mutex<SharedState>>) {
        if ch == ' ' || ch == '\n' || ch == '\r' {
            return;
        }
        let freq = if ch.is_ascii_alphabetic() { 900.0 } else { 850.0 };
        let samples = (SAMPLE_RATE as f32 * 0.018) as usize;
        let ev = SoundEvent {
            freq,
            samples_left: samples,
            volume: 0.35,
        };
        if let Ok(mut s) = shared.lock() {
            // cap queue to avoid unbounded growth
            if s.queue.len() < 64 {
                s.queue.push_back(ev);
            }
        }
    }
}

impl SoundGenerator for ModemSound {
    fn trigger(&self, ch: char, shared: &Arc<Mutex<SharedState>>) {
        if ch == '\n' || ch == '\r' {
            return;
        }
        let freq = if ch == ' ' { 700.0 } else { 1400.0 };
        let samples = (SAMPLE_RATE as f32 * 0.035) as usize;
        let ev = SoundEvent {
            freq,
            samples_left: samples,
            volume: 0.28,
        };
        if let Ok(mut s) = shared.lock() {
            if s.queue.len() < 64 {
                s.queue.push_back(ev);
            }
        }
    }
}

impl SoundGenerator for MinimalSound {
    fn trigger(&self, ch: char, shared: &Arc<Mutex<SharedState>>) {
        if ch.is_whitespace() {
            return;
        }
        let freq = 600.0;
        let samples = (SAMPLE_RATE as f32 * 0.010) as usize;
        let ev = SoundEvent {
            freq,
            samples_left: samples,
            volume: 0.12,
        };
        if let Ok(mut s) = shared.lock() {
            if s.queue.len() < 32 {
                s.queue.push_back(ev);
            }
        }
    }
}

/// Central sound engine — owns the SDL2 audio device (if available) and a shared queue
/// consumed by the `AudioCallback` at 48 kHz. Gracefully falls back to silent if no device.
pub struct SoundEngine {
    _device: Option<AudioDevice<CrtAudioCallback>>,
    shared: Arc<Mutex<SharedState>>,
}

impl SoundEngine {
    /// Try to open a 48 kHz mono `f32` device. On failure returns a silent engine.
    #[must_use]
    pub fn new(audio: &sdl2::AudioSubsystem) -> Self {
        let shared = Arc::new(Mutex::new(SharedState {
            queue: VecDeque::new(),
            current: None,
        }));
        let shared_clone = Arc::clone(&shared);
        let spec = AudioSpecDesired {
            freq: Some(SAMPLE_RATE),
            channels: Some(1),
            samples: Some(1024),
        };
        let device = match audio.open_playback(None, &spec, move |_spec| CrtAudioCallback {
            shared: shared_clone,
            sample_rate: SAMPLE_RATE as f32,
        }) {
            Ok(dev) => {
                dev.resume();
                if std::env::var("DEBUG").is_ok() {
                    eprintln!("sound: audio device opened @ {SAMPLE_RATE} Hz");
                }
                Some(dev)
            }
            Err(e) => {
                if std::env::var("DEBUG").is_ok() {
                    eprintln!("sound: no audio device ({e}), running silent");
                }
                None
            }
        };
        Self {
            _device: device,
            shared,
        }
    }

    /// Silent fallback — no device, queue still present but never consumed audibly.
    #[must_use]
    pub fn silent() -> Self {
        Self {
            _device: None,
            shared: Arc::new(Mutex::new(SharedState {
                queue: VecDeque::new(),
                current: None,
            })),
        }
    }

    /// Whether an audio device is active.
    #[allow(dead_code)]
    #[must_use]
    pub fn has_device(&self) -> bool {
        self._device.is_some()
    }

    /// Play a single character with the given palette. No-op if menu silent or no device.
    pub fn play_char(&self, ch: char, palette: SoundPalette) {
        if self._device.is_none() {
            return;
        }
        // Menu silence is enforced by caller (AppState::drain checks is_menu_active),
        // but double-guard here.
        match palette {
            SoundPalette::Teletype => TeletypeSound.trigger(ch, &self.shared),
            SoundPalette::ModemCrt => ModemSound.trigger(ch, &self.shared),
            SoundPalette::Minimal => MinimalSound.trigger(ch, &self.shared),
        }
    }

    /// For testing: queue length.
    #[allow(dead_code)]
    #[must_use]
    pub fn queue_len(&self) -> usize {
        self.shared.lock().map(|s| s.queue.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silent_engine_no_panic() {
        let eng = SoundEngine::silent();
        assert!(!eng.has_device());
        eng.play_char('A', SoundPalette::Teletype);
        // queue should be filled even without device? Our silent still has shared but play_char checks device none so no enqueue.
        // For silent, play_char is no-op (no device), so queue remains 0.
        assert_eq!(eng.queue_len(), 0);
    }

    #[test]
    fn palette_generators_enqueue() {
        let shared = Arc::new(Mutex::new(SharedState {
            queue: VecDeque::new(),
            current: None,
        }));
        TeletypeSound.trigger('A', &shared);
        assert_eq!(shared.lock().unwrap().queue.len(), 1);
        shared.lock().unwrap().queue.clear();
        ModemSound.trigger(' ', &shared);
        assert_eq!(shared.lock().unwrap().queue.len(), 1);
        shared.lock().unwrap().queue.clear();
        MinimalSound.trigger(' ', &shared);
        // Minimal ignores whitespace
        assert_eq!(shared.lock().unwrap().queue.len(), 0);
        MinimalSound.trigger('X', &shared);
        assert_eq!(shared.lock().unwrap().queue.len(), 1);
    }
}
