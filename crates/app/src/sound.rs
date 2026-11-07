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

/// One knock of wood on wood: a filtered click over a short, low body resonance.
/// `brightness` (0–1) opens the filter; `body` is the resonance in Hz.
fn tap(out: &mut [f32], at: usize, loudness: f32, brightness: f32, body: f32, seed: u32) {
    let mut noise = Noise(seed);
    let mut low = 0.0f32;
    let alpha = 0.08 + 0.5 * brightness;
    let start = at.min(out.len());
    for (index, sample) in out[start..].iter_mut().enumerate() {
        let t = index as f32 / RATE as f32;
        low += alpha * (noise.next() - low);
        let click = low * (-t / 0.004).exp();
        let resonance = (std::f32::consts::TAU * body * t).sin() * (-t / 0.022).exp();
        let overtone = (std::f32::consts::TAU * body * 2.7 * t).sin() * (-t / 0.009).exp();
        *sample += loudness * (0.9 * click + 0.45 * resonance + 0.18 * overtone);
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
        Sound::Move => ms(110),
        Sound::Capture => ms(170),
        Sound::Castle => ms(200),
        Sound::Check => ms(160),
        Sound::GameEnd => ms(900),
    };
    let mut out = vec![0.0f32; length];
    match sound {
        Sound::Move => tap(&mut out, 0, 1.0, 0.25, 210.0, 0x9E37_79B9),
        Sound::Capture => {
            tap(&mut out, 0, 1.25, 0.6, 170.0, 0x85EB_CA6B);
            tap(&mut out, ms(38), 0.55, 0.35, 240.0, 0xC2B2_AE35);
        }
        Sound::Castle => {
            tap(&mut out, 0, 0.9, 0.25, 220.0, 0x27D4_EB2F);
            tap(&mut out, ms(85), 0.8, 0.3, 190.0, 0x1656_67B1);
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
