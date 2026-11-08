//! Recorded chess-piece sounds by default, with optional synthesized profiles.
//! Classic uses CC0 wooden-board recordings by el_boss (see assets/sounds/classic).
//! Futuristic, NES, Piano and SFX are the AGPL-licensed Lichess sets by Enigmahack
//! (see assets/sounds/lichess).
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

/// Persisted sound character, shared by all move events and previews.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoundProfile {
    #[default]
    Classic,
    Wood,
    Soft,
    Click,
    Bell,
    DeepWood,
    Thud,
    Futuristic,
    Nes,
    Piano,
    Sfx,
}
impl SoundProfile {
    pub const ALL: [Self; 11] = [
        Self::Classic,
        Self::Wood,
        Self::Soft,
        Self::Click,
        Self::Bell,
        Self::DeepWood,
        Self::Thud,
        Self::Futuristic,
        Self::Nes,
        Self::Piano,
        Self::Sfx,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Classic => "Classic",
            Self::Wood => "Wood",
            Self::Soft => "Soft",
            Self::Click => "Click",
            Self::Bell => "Bell",
            Self::DeepWood => "Deep wood",
            Self::Thud => "Thud",
            Self::Futuristic => "Futuristic",
            Self::Nes => "NES",
            Self::Piano => "Piano",
            Self::Sfx => "SFX",
        }
    }
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
pub fn play_move(
    profile: SoundProfile,
    volume: f32,
    mv: &Move,
    after: &Chess,
    end: Option<GameEnd>,
) {
    play(profile, &sounds_for(mv, after, end), volume);
}

/// Mixes the sounds into one clip at `volume` (0–1), each starting a little after
/// the previous one.
pub fn play(profile: SoundProfile, sounds: &[Sound], volume: f32) {
    if sounds.is_empty() || volume <= 0.0 {
        return;
    }
    if let Some(sender) = player() {
        // Drop excess previews rather than blocking navigation or building a backlog.
        let _ = sender.try_send((profile, sounds.to_vec(), volume));
    }
}

fn mix(profile: SoundProfile, sounds: &[Sound], volume: f32) -> Vec<f32> {
    if sounds.is_empty() || volume <= 0.0 {
        return Vec::new();
    }
    let volume = volume.min(1.0);
    let mut clip: Vec<f32> = Vec::new();
    let mut offset = 0usize;
    for sound in sounds {
        let samples = synthesise_profile(profile, *sound);
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
    clip
}

/// The playback thread's queue, started the first time a sound plays.
type Playback = (SoundProfile, Vec<Sound>, f32);
fn player() -> Option<&'static mpsc::SyncSender<Playback>> {
    static PLAYER: OnceLock<Option<mpsc::SyncSender<Playback>>> = OnceLock::new();
    PLAYER
        .get_or_init(|| {
            let (sender, receiver) = mpsc::sync_channel::<Playback>(4);
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
                    while let Ok((profile, sounds, volume)) = receiver.recv() {
                        let clip = mix(profile, &sounds, volume);
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
    (2.57, 0.35, 0.45),
    (4.68, 0.14, 0.25),
    (7.2, 0.05, 0.15),
];

/// One knock: a short, rounded strike that sets the wood's modes ringing.
/// `pitch` is the fundamental in Hz and `ring` its decay time in seconds.
fn knock(out: &mut [f32], at: usize, loudness: f32, pitch: f32, ring: f32, seed: u32) {
    let mut noise = Noise(seed);
    let mut previous = 0.0f32;
    let start = at.min(out.len());
    for (index, sample) in out[start..].iter_mut().enumerate() {
        let t = index as f32 / RATE as f32;
        // Round off the noisy impact so it adds contact without a sharp hiss.
        previous += 0.18 * (noise.next() - previous);
        let strike = previous * (-t / 0.0015).exp();
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

/// Turns embedded signed 16-bit little-endian mono PCM into samples.
fn decode(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(2)
        .map(|sample| i16::from_le_bytes([sample[0], sample[1]]) as f32 / 32768.0)
        .collect()
}

/// The embedded PCM of one Lichess sound set, as (move, capture, check, game end).
macro_rules! lichess_set {
    ($set:literal) => {
        [
            include_bytes!(concat!(
                "../../../assets/sounds/lichess/",
                $set,
                "/Move.pcm"
            ))
            .as_slice(),
            include_bytes!(concat!(
                "../../../assets/sounds/lichess/",
                $set,
                "/Capture.pcm"
            ))
            .as_slice(),
            include_bytes!(concat!(
                "../../../assets/sounds/lichess/",
                $set,
                "/Check.pcm"
            ))
            .as_slice(),
            include_bytes!(concat!(
                "../../../assets/sounds/lichess/",
                $set,
                "/GenericNotify.pcm"
            ))
            .as_slice(),
        ]
    };
}

/// A Lichess set's recording for a move event, decoded once on the sound thread.
/// Castling uses the move sound, as on Lichess; the game's end uses its notify sound.
fn lichess(profile: SoundProfile, sound: Sound) -> Vec<f32> {
    static SETS: [[&[u8]; 4]; 4] = [
        lichess_set!("futuristic"),
        lichess_set!("nes"),
        lichess_set!("piano"),
        lichess_set!("sfx"),
    ];
    static DECODED: [[OnceLock<Vec<f32>>; 4]; 4] = [const { [const { OnceLock::new() }; 4] }; 4];
    let set = match profile {
        SoundProfile::Futuristic => 0,
        SoundProfile::Nes => 1,
        SoundProfile::Piano => 2,
        SoundProfile::Sfx => 3,
        _ => return Vec::new(),
    };
    let index = match sound {
        Sound::Move | Sound::Castle => 0,
        Sound::Capture => 1,
        Sound::Check => 2,
        Sound::GameEnd => 3,
    };
    DECODED[set][index]
        .get_or_init(|| decode(SETS[set][index]))
        .clone()
}

/// Decode embedded mono PCM once, away from the UI thread. No runtime downloads.
fn classic(sound: Sound) -> Vec<f32> {
    static PLACEMENT: OnceLock<Vec<f32>> = OnceLock::new();
    static CAPTURE: OnceLock<Vec<f32>> = OnceLock::new();
    static RESET: OnceLock<Vec<f32>> = OnceLock::new();
    let placement = || {
        PLACEMENT.get_or_init(|| {
            decode(include_bytes!(
                "../../../assets/sounds/classic/placement.pcm"
            ))
        })
    };
    match sound {
        Sound::Move => placement().clone(),
        Sound::Capture => CAPTURE
            .get_or_init(|| decode(include_bytes!("../../../assets/sounds/classic/capture.pcm")))
            .clone(),
        Sound::Castle => {
            let samples = placement();
            let mut out = vec![0.0; samples.len() + ms(90)];
            for (index, sample) in samples.iter().enumerate() {
                out[index] += sample * 0.85;
                out[index + ms(90)] += sample * 0.85;
            }
            out
        }
        // The piece impact already announces check; avoid adding electronic tones.
        Sound::Check => Vec::new(),
        Sound::GameEnd => RESET
            .get_or_init(|| decode(include_bytes!("../../../assets/sounds/classic/reset.pcm")))
            .clone(),
    }
}

/// Alternate profiles use distinct envelopes and timbres, including event cues.
pub fn synthesise_profile(profile: SoundProfile, sound: Sound) -> Vec<f32> {
    match profile {
        SoundProfile::Classic => return classic(sound),
        SoundProfile::Wood => return synthesise(sound),
        SoundProfile::Futuristic | SoundProfile::Nes | SoundProfile::Piano | SoundProfile::Sfx => {
            return lichess(profile, sound);
        }
        _ => {}
    }
    let length = match sound {
        Sound::Move => 100,
        Sound::Capture => 150,
        Sound::Castle => 200,
        Sound::Check => 200,
        Sound::GameEnd => 650,
    };
    let mut out = vec![0.0; ms(length)];
    let pitch = match sound {
        Sound::Move => 330.0,
        Sound::Capture => 220.0,
        Sound::Castle => 275.0,
        Sound::Check => 440.0,
        Sound::GameEnd => 330.0,
    };
    match profile {
        SoundProfile::Soft => {
            tone(&mut out, 0, pitch * 0.45, 0.38, 0.018);
            tone(&mut out, 0, pitch * 0.9, 0.10, 0.009);
        }
        SoundProfile::Click => knock(&mut out, 0, 0.65, pitch, 0.008, 0x9E37_79B9),
        SoundProfile::Bell => {
            tone(&mut out, 0, pitch, 0.30, 0.045);
            tone(&mut out, 0, pitch * 1.5, 0.07, 0.025);
        }
        SoundProfile::DeepWood => {
            knock(&mut out, 0, 1.0, pitch * 0.38, 0.035, 0x9E37_79B9);
        }
        SoundProfile::Thud => {
            tone(&mut out, 0, pitch * 0.23, 0.65, 0.025);
            tone(&mut out, 0, pitch * 0.36, 0.16, 0.012);
        }
        SoundProfile::Wood
        | SoundProfile::Classic
        | SoundProfile::Futuristic
        | SoundProfile::Nes
        | SoundProfile::Piano
        | SoundProfile::Sfx => unreachable!(),
    }
    if matches!(sound, Sound::Castle | Sound::GameEnd) {
        let multiplier = match profile {
            SoundProfile::DeepWood => 0.45,
            SoundProfile::Thud => 0.28,
            _ => 1.5,
        };
        tone(&mut out, ms(80), pitch * multiplier, 0.20, 0.04);
    }
    // Two low-pass stages remove bright impact harmonics from the darker profiles.
    if matches!(profile, SoundProfile::DeepWood | SoundProfile::Thud) {
        let cutoff = if profile == SoundProfile::Thud {
            300.0
        } else {
            500.0
        };
        let alpha = 1.0 - (-std::f32::consts::TAU * cutoff / RATE as f32).exp();
        let (mut first, mut second) = (0.0, 0.0);
        for sample in &mut out {
            first += alpha * (*sample - first);
            second += alpha * (first - second);
            *sample = second;
        }
    }
    let fade = ms(8).min(out.len());
    let start = out.len() - fade;
    for (index, sample) in out[start..].iter_mut().enumerate() {
        *sample *= 1.0 - index as f32 / fade as f32;
    }
    out
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
        // A low wooden clank with a short, solid body.
        Sound::Move => knock(&mut out, 0, 1.0, 260.0, 0.028, 0x9E37_79B9),
        // A heavier, lower knock, then the taken piece's lighter click.
        Sound::Capture => {
            knock(&mut out, 0, 1.15, 180.0, 0.032, 0x85EB_CA6B);
            knock(&mut out, ms(30), 0.5, 320.0, 0.018, 0xC2B2_AE35);
        }
        // King, then rook.
        Sound::Castle => {
            knock(&mut out, 0, 0.9, 240.0, 0.028, 0x27D4_EB2F);
            knock(&mut out, ms(80), 0.85, 210.0, 0.030, 0x1656_67B1);
        }
        Sound::Check => {
            tone(&mut out, 0, 440.0, 0.22, 0.05);
            tone(&mut out, 0, 660.0, 0.08, 0.03);
        }
        Sound::GameEnd => {
            tone(&mut out, 0, 261.63, 0.3, 0.25);
            tone(&mut out, ms(160), 349.23, 0.3, 0.35);
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
    fn moves_have_a_low_body_and_a_short_decay() {
        let samples = synthesise(Sound::Move);
        // Keep the body in the wooden clank range, below the old bright click.
        assert!(
            (180.0..360.0).contains(&median_frequency(&samples)),
            "{}",
            median_frequency(&samples)
        );
        // Short: nearly all its energy within the first 40 ms.
        let energy = |part: &[f32]| part.iter().map(|s| s * s).sum::<f32>();
        assert!(energy(&samples[..ms(40)]) > 0.95 * energy(&samples));
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
    fn classic_uses_recordings_and_no_electronic_check_overlay() {
        assert_eq!(SoundProfile::default(), SoundProfile::Classic);
        let placement = synthesise_profile(SoundProfile::Classic, Sound::Move);
        let capture = synthesise_profile(SoundProfile::Classic, Sound::Capture);
        assert!((ms(50)..ms(250)).contains(&placement.len()));
        assert!((ms(100)..ms(400)).contains(&capture.len()));
        assert!(placement.iter().any(|sample| sample.abs() > 0.1));
        assert_ne!(placement, capture);
        assert_ne!(placement, synthesise(Sound::Move));
        assert!(synthesise_profile(SoundProfile::Classic, Sound::Check).is_empty());
        assert_eq!(
            mix(SoundProfile::Classic, &[Sound::Move, Sound::Check], 1.0),
            mix(SoundProfile::Classic, &[Sound::Move], 1.0)
        );
    }

    #[test]
    fn lichess_sets_play_their_own_recording_for_every_event() {
        for profile in [
            SoundProfile::Futuristic,
            SoundProfile::Nes,
            SoundProfile::Piano,
            SoundProfile::Sfx,
        ] {
            let clips = [Sound::Move, Sound::Capture, Sound::Check, Sound::GameEnd]
                .map(|sound| synthesise_profile(profile, sound));
            for (index, clip) in clips.iter().enumerate() {
                assert!(clip.iter().any(|s| s.abs() > 0.1), "{profile:?} is silent");
                assert!(clip.len() <= ms(1500), "{profile:?} is too long");
                for other in &clips[index + 1..] {
                    assert_ne!(clip, other, "{profile:?} repeats a sound");
                }
            }
            assert_eq!(
                synthesise_profile(profile, Sound::Castle),
                clips[0],
                "{profile:?} castles with its move sound"
            );
        }
    }

    #[test]
    fn darker_profiles_keep_all_event_cues_in_the_low_register() {
        for profile in [SoundProfile::DeepWood, SoundProfile::Thud] {
            for sound in [
                Sound::Move,
                Sound::Capture,
                Sound::Castle,
                Sound::Check,
                Sound::GameEnd,
            ] {
                let frequency = median_frequency(&synthesise_profile(profile, sound));
                assert!(
                    (40.0..220.0).contains(&frequency),
                    "{profile:?} {sound:?}: {frequency} Hz"
                );
            }
        }
    }

    #[test]
    fn profiles_are_distinct_and_mixed_audio_is_bounded() {
        let profiles = SoundProfile::ALL.map(|profile| synthesise_profile(profile, Sound::Move));
        for (index, samples) in profiles.iter().enumerate() {
            assert!(samples.iter().any(|sample| sample.abs() > 0.05));
            for other in &profiles[index + 1..] {
                assert_ne!(samples, other);
            }
        }
        for profile in SoundProfile::ALL {
            for sound in [
                Sound::Move,
                Sound::Capture,
                Sound::Castle,
                Sound::Check,
                Sound::GameEnd,
            ] {
                let clip = mix(profile, &[sound], 1.0);
                assert!(
                    clip.iter()
                        .all(|sample| sample.is_finite() && sample.abs() <= 0.951)
                );
                if let Some(last) = clip.last() {
                    assert!(last.abs() < 0.001);
                }
            }
            assert!(mix(profile, &[Sound::Move], 0.0).is_empty());
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
