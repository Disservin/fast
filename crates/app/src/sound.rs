//! Move sounds, synthesised so no audio files need bundling: a wooden tap for a move,
//! a sharper double knock for a capture, two taps for castling, a small ping for
//! check and a soft chime when the game ends.
//!
//! The audio device is opened on first use, on a thread of its own (its stream cannot
//! move between threads on every platform). Without a device the sounds are skipped.

use std::{
    num::NonZero,
    sync::{OnceLock, mpsc},
    thread,
};

use chess_core::{Chess, GameEnd, Move, Position};

const RATE: u32 = 44_100;
/// Loudness at full volume; the sounds are short, so they stay clear of clipping.
const LEVEL: f32 = 0.8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    Move,
    Capture,
    Castle,
    Check,
    GameEnd,
}

/// The sounds for a move, in order: the move itself, then check or the game's end.
pub fn sounds_for(mv: &Move, after: &Chess, end: Option<GameEnd>) -> Vec<Sound> {
    let mut sounds = vec![if mv.is_castle() {
        Sound::Castle
    } else if mv.is_capture() {
        Sound::Capture
    } else {
        Sound::Move
    }];
    if end.is_some() {
        sounds.push(Sound::GameEnd);
    } else if after.is_check() {
        sounds.push(Sound::Check);
    }
    sounds
}

/// Plays a move's sounds at `volume` (0–1; nothing at 0).
pub fn play_move(volume: f32, mv: &Move, after: &Chess, end: Option<GameEnd>) {
    play(&sounds_for(mv, after, end), volume);
}

/// Mixes the sounds into one clip at `volume` (0–1), each starting a little after
/// the previous one.
pub fn play(sounds: &[Sound], volume: f32) {
    if sounds.is_empty() || volume <= 0.0 {
        return;
    }
    let volume = volume.min(1.0);
    let mut clip: Vec<f32> = Vec::new();
    let mut offset = 0usize;
    for sound in sounds {
        let samples = synthesise(*sound);
        let end = offset + samples.len();
        if clip.len() < end {
            clip.resize(end, 0.0);
        }
        for (index, sample) in samples.iter().enumerate() {
            clip[offset + index] += sample;
        }
        offset += ms(70);
    }
    let peak = clip.iter().fold(0.0f32, |max, s| max.max(s.abs()));
    if peak > 0.95 {
        clip.iter_mut().for_each(|s| *s *= 0.95 / peak);
    }
    // Perceived loudness is roughly logarithmic: squaring makes the slider feel even.
    let gain = volume * volume;
    clip.iter_mut().for_each(|s| *s *= gain);
    if let Some(sender) = player() {
        let _ = sender.send(clip);
    }
}

/// The playback thread's queue, started the first time a sound plays.
fn player() -> Option<&'static mpsc::Sender<Vec<f32>>> {
    static PLAYER: OnceLock<Option<mpsc::Sender<Vec<f32>>>> = OnceLock::new();
    PLAYER
        .get_or_init(|| {
            let (sender, receiver) = mpsc::channel::<Vec<f32>>();
            let spawned = thread::Builder::new()
                .name("sound".to_owned())
                .spawn(move || {
                    let mut sink = match rodio::DeviceSinkBuilder::open_default_sink() {
                        Ok(sink) => sink,
                        Err(err) => {
                            log::info!("no audio output, move sounds are off: {err}");
                            return;
                        }
                    };
                    sink.log_on_drop(false);
                    let (Some(channels), Some(rate)) = (NonZero::new(1u16), NonZero::new(RATE))
                    else {
                        return;
                    };
                    while let Ok(clip) = receiver.recv() {
                        sink.mixer()
                            .add(rodio::buffer::SamplesBuffer::new(channels, rate, clip));
                    }
                });
            match spawned {
                Ok(_) => Some(sender),
                Err(err) => {
                    log::warn!("could not start the sound thread: {err}");
                    None
                }
            }
        })
        .as_ref()
}

fn ms(millis: u32) -> usize {
    (RATE * millis / 1000) as usize
}

/// A small deterministic noise source, so every tap sounds the same.
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// Resonances of a struck wooden piece on a board, as (frequency ratio, loudness,
/// decay relative to the fundamental's). Wood rings at inharmonic ratios and its
/// higher modes die fastest, which is what makes a knock sound like a crisp "tok"
/// rather than a note.
const WOOD_MODES: [(f32, f32, f32); 4] = [
    (1.0, 1.0, 1.0),
    (2.57, 0.5, 0.55),
    (4.68, 0.28, 0.35),
    (7.2, 0.12, 0.22),
];

/// One knock: a very short, bright strike that sets the wood's modes ringing.
/// `pitch` is the fundamental in Hz and `ring` its decay time in seconds.
fn knock(out: &mut [f32], at: usize, loudness: f32, pitch: f32, ring: f32, seed: u32) {
    let mut noise = Noise(seed);
    let mut previous = 0.0f32;
    let start = at.min(out.len());
    for (index, sample) in out[start..].iter_mut().enumerate() {
        let t = index as f32 / RATE as f32;
        // The strike: high-passed noise lasting about a millisecond.
        let raw = noise.next();
        let strike = (raw - previous) * 0.5 * (-t / 0.0007).exp();
        previous = raw;
        // A 0.3 ms rise keeps the start from popping.
        let attack = (t / 0.0003).min(1.0);
        let mut body = 0.0;
        for (ratio, level, decay) in WOOD_MODES {
            let freq = pitch * ratio;
            body += level * (std::f32::consts::TAU * freq * t).sin() * (-t / (ring * decay)).exp();
        }
        *sample += loudness * attack * (0.55 * strike + 0.6 * body);
    }
}

/// A plain decaying sine, for the check ping and the end chime.
fn tone(out: &mut [f32], at: usize, freq: f32, loudness: f32, decay: f32) {
    let start = at.min(out.len());
    for (index, sample) in out[start..].iter_mut().enumerate() {
        let t = index as f32 / RATE as f32;
        let attack = (t / 0.004).min(1.0);
        *sample +=
            loudness * attack * (std::f32::consts::TAU * freq * t).sin() * (-t / decay).exp();
    }
}

/// The samples of one sound, mono at [`RATE`].
pub fn synthesise(sound: Sound) -> Vec<f32> {
    let length = match sound {
        Sound::Move => ms(80),
        Sound::Capture => ms(120),
        Sound::Castle => ms(170),
        Sound::Check => ms(160),
        Sound::GameEnd => ms(900),
    };
    let mut out = vec![0.0f32; length];
    match sound {
        // A light, crisp tok.
        Sound::Move => knock(&mut out, 0, 1.0, 1150.0, 0.016, 0x9E37_79B9),
        // A heavier, lower knock, then the taken piece's lighter click.
        Sound::Capture => {
            knock(&mut out, 0, 1.15, 780.0, 0.022, 0x85EB_CA6B);
            knock(&mut out, ms(30), 0.5, 1500.0, 0.010, 0xC2B2_AE35);
        }
        // King, then rook.
        Sound::Castle => {
            knock(&mut out, 0, 0.9, 1100.0, 0.015, 0x27D4_EB2F);
            knock(&mut out, ms(80), 0.85, 960.0, 0.016, 0x1656_67B1);
        }
        Sound::Check => {
            tone(&mut out, 0, 1318.5, 0.22, 0.05);
            tone(&mut out, 0, 1975.5, 0.08, 0.03);
        }
        Sound::GameEnd => {
            tone(&mut out, 0, 783.99, 0.3, 0.25);
            tone(&mut out, ms(160), 1046.5, 0.3, 0.35);
        }
    }
    out.iter_mut().for_each(|s| *s *= LEVEL);
    // A brief fade-out, so no clip ends with a click.
    let fade = ms(8).min(out.len());
    let start = out.len() - fade;
    for (index, sample) in out[start..].iter_mut().enumerate() {
        *sample *= 1.0 - index as f32 / fade as f32;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chess_core::{GameTree, shakmaty::san::San};

    fn after(tree: &GameTree, node: chess_core::NodeId) -> (Move, Chess) {
        let node = tree.node(node);
        (node.mv.expect("a move"), node.pos.clone())
    }

    #[test]
    fn sounds_follow_the_kind_of_move() {
        let mut tree = GameTree::new();
        let mut node = GameTree::ROOT;
        for san in ["e4", "d5"] {
            node = tree.add_san(node, san).unwrap();
        }
        let capture = tree.add_san(node, "exd5").unwrap();
        let (mv, pos) = after(&tree, capture);
        assert_eq!(sounds_for(&mv, &pos, None), vec![Sound::Capture]);
        let quiet = tree.add_san(node, "Nf3").unwrap();
        let (mv, pos) = after(&tree, quiet);
        assert_eq!(sounds_for(&mv, &pos, None), vec![Sound::Move]);

        let mut mate = GameTree::new();
        let mut node = GameTree::ROOT;
        for san in ["f3", "e5", "g4", "Qh4#"] {
            node = mate.add_san(node, san).unwrap();
        }
        let (mv, pos) = after(&mate, node);
        assert_eq!(
            sounds_for(&mv, &pos, mate.game_end(node)),
            vec![Sound::Move, Sound::GameEnd]
        );
        let castle_pos = GameTree::from_fen("4k3/8/8/8/8/8/8/4K2R w K - 0 1").unwrap();
        let castle = "O-O"
            .parse::<San>()
            .unwrap()
            .to_move(&castle_pos.node(GameTree::ROOT).pos)
            .unwrap();
        assert_eq!(
            sounds_for(&castle, &Chess::default(), None)[0],
            Sound::Castle
        );
    }

    /// The frequency below which half the sound's energy lies, from a plain DFT.
    fn median_frequency(samples: &[f32]) -> f32 {
        let n = samples.len().min(2048);
        let bins: Vec<f32> = (1..n / 2)
            .map(|k| {
                let (mut re, mut im) = (0.0f32, 0.0f32);
                for (i, s) in samples[..n].iter().enumerate() {
                    let angle = std::f32::consts::TAU * (k * i) as f32 / n as f32;
                    re += s * angle.cos();
                    im -= s * angle.sin();
                }
                re * re + im * im
            })
            .collect();
        let total: f32 = bins.iter().sum();
        let mut running = 0.0;
        for (k, energy) in bins.iter().enumerate() {
            running += energy;
            if running >= total / 2.0 {
                return (k + 1) as f32 * RATE as f32 / n as f32;
            }
        }
        0.0
    }

    #[test]
    fn a_move_is_a_crisp_click_not_a_thud() {
        let samples = synthesise(Sound::Move);
        // Bright: most energy well above a low thump.
        assert!(
            median_frequency(&samples) > 900.0,
            "{}",
            median_frequency(&samples)
        );
        // Short: nearly all its energy within the first 30 ms.
        let energy = |part: &[f32]| part.iter().map(|s| s * s).sum::<f32>();
        assert!(energy(&samples[..ms(30)]) > 0.95 * energy(&samples));
        // A capture sits lower than a move.
        assert!(median_frequency(&synthesise(Sound::Capture)) < median_frequency(&samples));
    }

    /// Writes every sound to WAV files, to listen to without starting the app:
    /// `FAST_SOUNDS=/some/dir cargo test -p fast write_sounds -- --ignored`
    #[test]
    #[ignore]
    fn write_sounds() {
        let dir =
            std::path::PathBuf::from(std::env::var("FAST_SOUNDS").unwrap_or_else(|_| ".".into()));
        std::fs::create_dir_all(&dir).unwrap();
        for (sound, name) in [
            (Sound::Move, "move"),
            (Sound::Capture, "capture"),
            (Sound::Castle, "castle"),
            (Sound::Check, "check"),
            (Sound::GameEnd, "game-end"),
        ] {
            let samples: Vec<i16> = synthesise(sound)
                .iter()
                .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                .collect();
            let data_len = (samples.len() * 2) as u32;
            let mut wav = Vec::with_capacity(44 + data_len as usize);
            wav.extend_from_slice(b"RIFF");
            wav.extend_from_slice(&(36 + data_len).to_le_bytes());
            wav.extend_from_slice(b"WAVEfmt ");
            wav.extend_from_slice(&16u32.to_le_bytes());
            wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
            wav.extend_from_slice(&1u16.to_le_bytes()); // mono
            wav.extend_from_slice(&RATE.to_le_bytes());
            wav.extend_from_slice(&(RATE * 2).to_le_bytes());
            wav.extend_from_slice(&2u16.to_le_bytes());
            wav.extend_from_slice(&16u16.to_le_bytes());
            wav.extend_from_slice(b"data");
            wav.extend_from_slice(&data_len.to_le_bytes());
            for sample in samples {
                wav.extend_from_slice(&sample.to_le_bytes());
            }
            std::fs::write(dir.join(format!("{name}.wav")), wav).unwrap();
        }
    }

    #[test]
    fn every_sound_is_audible_short_and_unclipped() {
        for sound in [
            Sound::Move,
            Sound::Capture,
            Sound::Castle,
            Sound::Check,
            Sound::GameEnd,
        ] {
            let samples = synthesise(sound);
            let peak = samples.iter().fold(0.0f32, |max, s| max.max(s.abs()));
            assert!(peak > 0.05, "{sound:?} is silent");
            assert!(peak <= 1.0, "{sound:?} clips at {peak}");
            assert!(samples.len() <= ms(1000), "{sound:?} is too long");
            assert!(
                samples.last().unwrap().abs() < 1e-3,
                "{sound:?} ends abruptly"
            );
        }
        // Volume scales loudness; zero and sounds off play nothing.
        let mut board = crate::config::BoardConfig::default();
        assert!(board.sound_level() > 0.0);
        board.move_sounds = false;
        assert_eq!(board.sound_level(), 0.0);
        board.move_sounds = true;
        board.sound_volume = 250;
        assert_eq!(board.sound_level(), 1.0);
        // The same sound is the same every time.
        assert_eq!(synthesise(Sound::Move), synthesise(Sound::Move));
    }
}
