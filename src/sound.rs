// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! Playing a track's clips: an effect that follows the score (interface
//! 1.8). OxiGuitar prepares it with the track's marked beats as they are
//! played, repeats unrolled, each with its start in the score's seconds;
//! the clips among them become segments of one virtual sound along the
//! song, at the sound's rate. On the audio thread each call says where the
//! score is and how fast it moves, and the effect adds that stretch of the
//! virtual sound to the track's buffers, after its instrument and its other
//! effects, before its mixer row: so the track's volume, pan, mute and solo
//! are the clip's too.
//!
//! At 100 % the samples are copied as they are. At another practice speed
//! they go through a time stretch that keeps their pitch (timestretch's
//! streaming session, prepared here and run on the audio thread, which
//! allocates nothing and takes no lock). Starting, stopping and jumping
//! fade over 2.5 ms, so nothing clicks.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::SystemTime;

use timestretch::{
    Parameters, PreparedSession, ProcessStatus, SessionConfig, SourceAccess, StreamSession,
    TempoRate,
};

use crate::decode;

/// A file as it is on disk now: a changed file is read again.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    path: PathBuf,
    len: u64,
    modified: Option<SystemTime>,
}

/// Every clip's sound at a rate, while a prepared track holds it: shared,
/// so a track prepared again (a mark moved, a setting changed) reads no
/// file and resamples nothing, and freed once no track plays it.
static SOUNDS: Mutex<Option<Sounds>> = Mutex::new(None);

/// Clips' sounds by file and rate.
type Sounds = HashMap<(Key, u32), Weak<Vec<f32>>>;

/// The sound of `path` at `rate`, stereo and interleaved, or why it can't
/// be had: a sentence for the user.
pub fn at_rate(path: &Path, rate: u32) -> Result<Arc<Vec<f32>>, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("can't be read: {e}"))?;
    let key = (
        Key {
            path: path.to_path_buf(),
            len: meta.len(),
            modified: meta.modified().ok(),
        },
        rate,
    );
    let mut sounds = SOUNDS.lock().unwrap_or_else(PoisonError::into_inner);
    let sounds = sounds.get_or_insert_with(HashMap::new);
    sounds.retain(|_, w| w.strong_count() > 0);
    if let Some(s) = sounds.get(&key).and_then(Weak::upgrade) {
        return Ok(s);
    }
    let audio = decode::file(path)?;
    let samples = Arc::new(resample(audio, rate)?);
    sounds.insert(key, Arc::downgrade(&samples));
    Ok(samples)
}

/// `audio` at `rate`, through Rubato's FFT resampler (its whole-clip path,
/// which trims the resampler's delay so the result lines up).
fn resample(audio: decode::Audio, rate: u32) -> Result<Vec<f32>, String> {
    use rubato::audioadapter_buffers::direct::InterleavedSlice;
    use rubato::{Fft, FixedSync, Resampler};
    if audio.rate == rate {
        return Ok(audio.samples);
    }
    let frames = audio.frames();
    let failed = |e: &dyn std::fmt::Display| format!("can't be converted to {rate} Hz: {e}");
    let mut r = Fft::<f32>::new(audio.rate as usize, rate as usize, 1024, 2, FixedSync::Both)
        .map_err(|e| failed(&e))?;
    let input = InterleavedSlice::new(&audio.samples, 2, frames).map_err(|e| failed(&e))?;
    let out = r
        .process_all(&input, frames, None)
        .map_err(|e| failed(&e))?;
    Ok(out.take_data())
}

/// A clip as the track plays it once: from `start`, a frame of the
/// score's time at the sound's rate (at 100 % speed).
#[derive(Clone, Debug)]
pub struct Segment {
    /// Where it starts.
    pub start: i64,
    /// Its sound at the sound's rate, stereo and interleaved.
    pub sound: Arc<Vec<f32>>,
}

impl Segment {
    fn end(&self) -> i64 {
        self.start + (self.sound.len() / 2) as i64
    }
}

/// Frames a start, stop or jump fades over: 2.5 ms at 48 kHz.
const FADE_SECONDS: f64 = 0.0025;

/// The most frames the time stretch asks for at once (its source chunk).
const CHUNK: usize = 2048;

/// One track's clips, on the audio thread.
pub struct Clips {
    rate: f64,
    /// By start.
    segments: Vec<Segment>,
    /// Where the last one ends.
    end: i64,
    /// What one call adds, interleaved.
    out: Vec<f32>,
    /// What the stretch is fed, interleaved.
    feed: Vec<f32>,
    stretch: Option<Stretch>,
    /// The score's time the next call should start at, while playing.
    next: Option<f64>,
    /// The speed the last call played at.
    speed: f64,
    fade: usize,
}

/// The time stretch and where it is.
struct Stretch {
    session: StreamSession,
    /// The tempo it was last told.
    tempo: f64,
    /// Commands sent, for their ids.
    commands: u64,
    /// It must seek before it plays: it was idle, or the score jumped.
    lost: bool,
    /// It failed; the clips stay silent away from 100 %.
    broken: bool,
}

impl std::fmt::Debug for Clips {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Clips")
            .field("rate", &self.rate)
            .field("segments", &self.segments.len())
            .finish_non_exhaustive()
    }
}

impl Clips {
    /// The clips `segments` for a sound at `rate`, in calls of at most
    /// `frames` frames. Allocates all it will need.
    pub fn new(mut segments: Vec<Segment>, rate: u32, frames: u32) -> Clips {
        segments.sort_by_key(|s| s.start);
        let end = segments.iter().map(Segment::end).max().unwrap_or(0);
        let frames = frames.max(1) as usize;
        let stretch = (end > 0)
            .then(|| {
                PreparedSession::new(SessionConfig {
                    channels: 2,
                    source_sample_rate: rate,
                    output_sample_rate: rate,
                    source_access: SourceAccess::Seekable,
                    source_frames: Some(end as u64),
                    ..SessionConfig::default()
                })
                .ok()
            })
            .flatten()
            .map(|p| Stretch {
                session: p.activate(),
                tempo: 1.0,
                commands: 0,
                lost: true,
                broken: false,
            });
        Clips {
            rate: f64::from(rate),
            segments,
            end,
            out: vec![0.0; frames * 2],
            feed: vec![0.0; CHUNK.max(frames) * 2],
            stretch,
            next: None,
            speed: 1.0,
            fade: ((f64::from(rate) * FADE_SECONDS) as usize).max(1),
        }
    }

    /// Whether there is anything to play.
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// The virtual sound from frame `from`, `into.len() / 2` frames of it.
    fn mix(segments: &[Segment], from: i64, into: &mut [f32]) {
        into.fill(0.0);
        let to = from + (into.len() / 2) as i64;
        for s in segments {
            if s.start >= to {
                break;
            }
            let (a, b) = (from.max(s.start), to.min(s.end()));
            if a >= b {
                continue;
            }
            let src = &s.sound[((a - s.start) * 2) as usize..((b - s.start) * 2) as usize];
            let dst = &mut into[((a - from) * 2) as usize..((b - from) * 2) as usize];
            for (d, x) in dst.iter_mut().zip(src) {
                *d += x;
            }
        }
    }

    /// Whether any clip sounds between frames `from` and `to`.
    fn sounds(&self, from: i64, to: i64) -> bool {
        self.segments.iter().any(|s| s.start < to && s.end() > from)
    }

    /// Adds the track's clips to `left` and `right`, which start at `time`
    /// in the score's seconds and move `step` of them a frame; `step` is 0
    /// while the score isn't playing.
    pub fn process_at(&mut self, left: &mut [f32], right: &mut [f32], time: f64, step: f64) {
        let n = left.len().min(right.len()).min(self.out.len() / 2);
        if n == 0 || self.segments.is_empty() {
            return;
        }
        if step <= 0.0 {
            // Stopped, paused or counting in: what sounded fades out.
            if let Some(next) = self.next.take() {
                let len = self.fade.min(n);
                let speed = self.speed;
                if self.render(next, speed, len, false) {
                    let out = &self.out[..len * 2];
                    add(left, right, out, |i| 1.0 - (i as f32 + 1.0) / len as f32);
                }
            }
            if let Some(s) = &mut self.stretch {
                s.lost = true;
            }
            return;
        }
        let speed = step * self.rate;
        // Not where the last call left off: a start, a seek or a loop.
        let jumped = self.next.is_none_or(|t| (t - time).abs() > 0.5 * step);
        self.next = Some(time + n as f64 * step);
        self.speed = speed;
        if self.render(time, speed, n, jumped) {
            let fade = self.fade;
            let out = &self.out[..n * 2];
            if jumped {
                add(left, right, out, |i| {
                    ((i as f32 + 1.0) / fade as f32).min(1.0)
                });
            } else {
                add(left, right, out, |_| 1.0);
            }
        }
    }

    /// Fills `out` with `n` frames from `time` at `speed`; false when it
    /// is silence (nothing to add).
    fn render(&mut self, time: f64, speed: f64, n: usize, jumped: bool) -> bool {
        let from = (time * self.rate).round() as i64;
        if speed_is_one(speed) {
            if let Some(s) = &mut self.stretch {
                s.lost = true;
            }
            if !self.sounds(from, from + n as i64) {
                return false;
            }
            Self::mix(&self.segments, from, &mut self.out[..n * 2]);
            return true;
        }
        // Away from 100 %: the time stretch, once it has a clip to play.
        let reach = (n as f64 * speed).ceil() as i64 + CHUNK as i64;
        if !self.sounds(from - CHUNK as i64, from + reach) || from >= self.end {
            if let Some(s) = &mut self.stretch {
                s.lost = true;
            }
            return false;
        }
        let Some(s) = &mut self.stretch else {
            return false;
        };
        if s.broken {
            return false;
        }
        if (s.tempo - speed).abs() > 1e-9 {
            // A new tempo takes once the session has run with it; then it
            // seeks, so the sound lines up with the score from the start.
            let Ok(tempo) = TempoRate::new(speed) else {
                return false;
            };
            s.commands += 1;
            if s.session
                .schedule(s.commands, 0, Parameters::keylock(tempo))
                .is_err()
            {
                s.broken = true;
                return false;
            }
            for _ in 0..64 {
                if s.session.applied_parameters().command_id == s.commands {
                    break;
                }
                if !Self::turn(s, &self.segments, &mut self.feed, &mut self.out) {
                    return false;
                }
            }
            s.tempo = speed;
            s.lost = true;
        }
        if s.lost || jumped {
            if s.session.seek(from.max(0) as u64).is_err() {
                s.broken = true;
                return false;
            }
            s.lost = false;
        }
        let mut have = 0;
        // Each turn does a bounded share of the work; a session that makes
        // no progress in that many turns has failed.
        for _ in 0..4096 {
            if have == n {
                break;
            }
            match Self::step(
                s,
                &self.segments,
                &mut self.feed,
                &mut self.out[have * 2..n * 2],
            ) {
                Some(Some(frames)) => have += frames,
                Some(None) => {}
                None if s.broken => return false,
                None => break,
            }
        }
        let out = &mut self.out[..n * 2];
        out[have * 2..].fill(0.0);
        true
    }
}

impl Clips {
    /// One turn of the session: its share of the work, the source it asks
    /// for, and what it has ready into `into`. `Some(Some(frames))` when it
    /// gave frames, `Some(None)` when not yet, `None` when it is done (or
    /// failed: `broken`).
    fn step(
        s: &mut Stretch,
        segments: &[Segment],
        feed: &mut [f32],
        into: &mut [f32],
    ) -> Option<Option<usize>> {
        let report = s.session.advance(16);
        match report.status {
            ProcessStatus::NeedSource => {
                if let Some(request) = s.session.source_request() {
                    let frames = request.frames.min(feed.len() / 2);
                    let feed = &mut feed[..frames * 2];
                    Self::mix(segments, request.start_frame as i64, feed);
                    if s.session.supply(request, feed).is_err() {
                        s.broken = true;
                        return None;
                    }
                }
            }
            ProcessStatus::Fault => {
                s.broken = true;
                return None;
            }
            ProcessStatus::Finished | ProcessStatus::Rendered if report.output_ready == 0 => {
                return None;
            }
            _ => {}
        }
        match s.session.read_output(into) {
            Ok(stamp) if stamp.frames > 0 => {
                let acknowledged = stamp
                    .consumed(stamp.frames)
                    .ok()
                    .is_some_and(|r| s.session.acknowledge(r).is_ok());
                if !acknowledged {
                    s.broken = true;
                    return None;
                }
                Some(Some(stamp.frames))
            }
            Ok(_) => Some(None),
            Err(_) => {
                s.broken = true;
                None
            }
        }
    }

    /// A turn whose output is thrown away (while a new tempo takes); false
    /// when the session failed.
    fn turn(s: &mut Stretch, segments: &[Segment], feed: &mut [f32], scratch: &mut [f32]) -> bool {
        Self::step(s, segments, feed, scratch);
        !s.broken
    }
}

/// 100 % speed: the samples as they are.
fn speed_is_one(speed: f64) -> bool {
    (speed - 1.0).abs() < 1e-9
}

/// Adds `out` (interleaved) into `left` and `right`, frame `i` scaled by
/// `gain(i)`.
fn add(left: &mut [f32], right: &mut [f32], out: &[f32], gain: impl Fn(usize) -> f32) {
    for (i, ((l, r), f)) in left
        .iter_mut()
        .zip(right.iter_mut())
        .zip(out.as_chunks::<2>().0)
        .enumerate()
    {
        let g = gain(i);
        *l += f[0] * g;
        *r += f[1] * g;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clip of `frames` frames whose left side counts them from 1 and
    /// whose right side is minus that, at frame `start`.
    fn counting(start: i64, frames: usize) -> Segment {
        Segment {
            start,
            sound: Arc::new(
                (0..frames)
                    .flat_map(|i| [(i + 1) as f32, -((i + 1) as f32)])
                    .collect(),
            ),
        }
    }

    fn call(clips: &mut Clips, time: f64, step: f64, n: usize) -> (Vec<f32>, Vec<f32>) {
        let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
        clips.process_at(&mut l, &mut r, time, step);
        (l, r)
    }

    #[test]
    fn at_100_percent_a_clip_plays_its_samples_from_its_beat() {
        let rate = 48_000;
        let step = 1.0 / f64::from(rate);
        let mut clips = Clips::new(vec![counting(1000, 500)], rate, 256);
        // Playing from 0: its first frames reach the clip at frame 1000.
        let mut left = Vec::new();
        let mut right = Vec::new();
        for k in 0..8 {
            let (l, r) = call(&mut clips, (k * 256) as f64 * step, step, 256);
            left.extend(l);
            right.extend(r);
        }
        assert!(left[..1000].iter().all(|s| *s == 0.0));
        // The first call faded in, long before the clip: no trace of it.
        assert_eq!(left[1000], 1.0);
        assert_eq!(right[1000], -1.0);
        assert_eq!(left[1499], 500.0);
        assert!(left[1500..].iter().all(|s| *s == 0.0));
    }

    #[test]
    fn a_jump_into_a_clip_plays_from_there_faded_in_and_a_stop_fades_out() {
        let rate = 48_000;
        let step = 1.0 / f64::from(rate);
        let mut clips = Clips::new(vec![counting(0, 48_000)], rate, 512);
        // Into its middle, as a seek or a loop would.
        let (l, _) = call(&mut clips, 24_000.0 * step, step, 512);
        assert!(l[0] < 24_001.0 * 0.1, "faded in: {}", l[0]);
        assert_eq!(l[511], 24_512.0);
        // Then on, unbroken: no fade.
        let (l, _) = call(&mut clips, 24_512.0 * step, step, 512);
        assert_eq!(l[0], 24_513.0);
        // Stopped: a short fade of what came next, then silence.
        let (l, _) = call(&mut clips, 0.0, 0.0, 512);
        assert!(l[0] > 0.0 && l[0] < 25_025.0);
        assert!(l[200..].iter().all(|s| *s == 0.0));
        let (l, _) = call(&mut clips, 0.0, 0.0, 512);
        assert!(l.iter().all(|s| *s == 0.0));
    }

    #[test]
    fn clips_that_overlap_both_sound() {
        let rate = 48_000;
        let step = 1.0 / f64::from(rate);
        let mut clips = Clips::new(vec![counting(0, 100), counting(50, 100)], rate, 256);
        call(&mut clips, -256.0 * step, step, 256);
        let (l, _) = call(&mut clips, 0.0, step, 256);
        assert_eq!(l[60], 61.0 + 11.0);
        assert_eq!(l[120], 71.0);
    }

    #[test]
    fn at_another_speed_a_clip_keeps_its_pitch_and_follows_the_score() {
        let rate = 48_000u32;
        let r = f64::from(rate);
        // Two seconds of 440 Hz from the song's start.
        let tone = Segment {
            start: 0,
            sound: Arc::new(
                (0..2 * rate as usize)
                    .flat_map(|i| {
                        let s = (i as f64 * 440.0 * std::f64::consts::TAU / r).sin() as f32 * 0.5;
                        [s, s]
                    })
                    .collect(),
            ),
        };
        let mut clips = Clips::new(vec![tone], rate, 512);
        // At 50 %: the score moves half a frame each frame.
        let step = 0.5 / r;
        let mut left = Vec::new();
        let mut time = 0.0;
        for _ in 0..200 {
            let (l, _) = call(&mut clips, time, step, 512);
            left.extend(l);
            time += 512.0 * step;
        }
        // The pitch: zero crossings a second, past the start, are 2 × 440.
        let tail = &left[48_000..96_000];
        let crossings = tail
            .windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count();
        assert!(
            (crossings as i64 - 880).abs() < 20,
            "{crossings} crossings: the pitch moved"
        );
        assert!(
            tail.iter().map(|s| s.abs()).fold(0.0, f32::max) > 0.3,
            "it sounds"
        );
    }

    #[test]
    fn at_half_speed_a_clip_starts_when_its_beat_is_heard() {
        let rate = 48_000u32;
        let r = f64::from(rate);
        // From half a second into the score: heard at one second at 50 %.
        let tone = Segment {
            start: 24_000,
            sound: Arc::new(
                (0..rate as usize)
                    .flat_map(|i| {
                        let s = (i as f64 * 0.13).sin() as f32 * 0.5;
                        [s, s]
                    })
                    .collect(),
            ),
        };
        let mut clips = Clips::new(vec![tone], rate, 512);
        let step = 0.5 / r;
        let mut left = Vec::new();
        let mut time = 0.0;
        for _ in 0..200 {
            let (l, _) = call(&mut clips, time, step, 512);
            left.extend(l);
            time += 512.0 * step;
        }
        let onset = left.iter().position(|s| s.abs() > 0.05).unwrap();
        assert!(
            (onset as i64 - 48_000).abs() < 480,
            "heard from frame {onset}, not 48000"
        );
    }
}
