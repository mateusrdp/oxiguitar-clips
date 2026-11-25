// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! The palette's Audio Clip tool and the window it opens (`ui/clips.slint`,
//! plugin interface 1.16: palette tools and their windows are a plugin's
//! own). The tool, or a click on
//! a clip on the page, opens the dialog for the rest at the caret: choose
//! a file (in the system's file dialog, the plugin's own), keep its path
//! relative to the score or absolute, start it a little later or earlier
//! than its rest (its offset), or remove the clip. OK writes it on
//! the rest as one change the user can undo; Cancel forgets it.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use oxiguitar_plugin::score::BeatKind;
use oxiguitar_plugin::score2::{Beat2, BeatExt, Score2};
use oxiguitar_plugin::serde_json::Value;
use oxiguitar_plugin::{Error, Host, Selection2, Tool, Value as View};

use crate::clip::{Clip, OFFSET_MAX};
use crate::decode;

/// The dialog's id in the manifest.
pub const DIALOG: &str = "clip";

/// The choice of how a path is kept, as the dialog lists it.
const PATHS: [&str; 2] = ["Relative to the score", "Absolute"];

/// A beat's place in the score: track, staff, bar, voice, beat.
pub type Place = [usize; 5];

/// What the open dialog is for and what it holds.
#[derive(Clone, Debug, Default, PartialEq)]
struct Editing {
    place: Place,
    /// The clip the rest has.
    had: Option<Clip>,
    /// A file chosen since, and how long it lasts.
    chosen: Option<(PathBuf, f64)>,
    /// The clip is to go.
    removed: bool,
    /// Keep the path relative to the score.
    relative: bool,
    /// Seconds after the rest the clip starts (below zero, before).
    offset: f64,
    /// A sentence about the last thing that went wrong.
    problem: Option<String>,
}

/// What the tool, the drawings' clicks and the dialog share.
#[derive(Debug, Default)]
pub struct Shared {
    editing: Mutex<Option<Editing>>,
}

impl Shared {
    fn editing(&self) -> MutexGuard<'_, Option<Editing>> {
        self.editing.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The beat at `place`.
fn beat_at(score: &Score2, [t, s, bar, v, b]: Place) -> Option<&Beat2> {
    score
        .tracks
        .get(t)?
        .staves
        .get(s)?
        .bars
        .get(bar)?
        .voices
        .get(v)?
        .beats
        .get(b)
}

fn beat_at_mut(score: &mut Score2, [t, s, bar, v, b]: Place) -> Option<&mut Beat2> {
    score
        .tracks
        .get_mut(t)?
        .staves
        .get_mut(s)?
        .bars
        .get_mut(bar)?
        .voices
        .get_mut(v)?
        .beats
        .get_mut(b)
}

/// The clip a beat has.
pub fn clip_of(beat: &Beat2) -> Option<Clip> {
    beat.ext.iter().find_map(|e| match e {
        BeatExt::Plugin { plugin, data } if plugin == crate::ID => Clip::from_text(data),
        _ => None,
    })
}

/// Gives a beat a clip, or none; whatever else it has stays.
pub fn set_clip(beat: &mut Beat2, clip: Option<&Clip>) {
    beat.ext
        .retain(|e| !matches!(e, BeatExt::Plugin { plugin, .. } if plugin == crate::ID));
    if let Some(c) = clip {
        beat.ext.push(BeatExt::Plugin {
            plugin: crate::ID.into(),
            data: c.to_text(),
        });
    }
}

/// The beat a selection starts at: the caret's, or the first selected.
fn first_place(score: &Score2, selection: &Selection2) -> Option<Place> {
    let i = |v: u32| v as usize;
    match *selection {
        Selection2::Caret {
            track,
            staff,
            voice,
            bar,
            beat,
            ..
        } => Some([i(track), i(staff), i(bar), i(voice), i(beat)]),
        Selection2::Beats {
            track,
            staff,
            voice,
            from,
            ..
        } => Some([i(track), i(staff), i(from.bar), i(voice), i(from.beat)]),
        Selection2::Bars {
            first_track,
            first_bar,
            ..
        } => (beat_at(score, [i(first_track), 0, i(first_bar), 0, 0]).is_some()).then_some([
            i(first_track),
            0,
            i(first_bar),
            0,
            0,
        ]),
    }
}

/// A clip starts on a rest: a beat that isn't a grace beat and has no
/// notes. One already there is opened wherever it is.
fn takes_a_clip(beat: &Beat2) -> bool {
    beat.kind == BeatKind::Rest && beat.grace.is_none()
}

/// The folder of the open score, once it is saved.
fn score_folder(host: &Host) -> Option<PathBuf> {
    host.score_file()
        .ok()
        .flatten()
        .and_then(|f| f.parent().map(Path::to_path_buf))
}

/// Whether paths are kept relative to the score: the last choice, which
/// the plugin's settings keep; relative at first.
fn relative_by_default(host: &Host) -> bool {
    host.settings()
        .ok()
        .and_then(|s| {
            s.get("paths")
                .and_then(Value::as_str)
                .map(|p| p != "absolute")
        })
        .unwrap_or(true)
}

/// Opens the dialog for the beat the caret is on.
pub fn open(host: &Host, shared: &Shared) -> Result<String, Error> {
    let Some((_, score)) = host.score2()? else {
        return Ok("No score is open.".into());
    };
    let Some(place) = host.selection()?.and_then(|s| first_place(&score, &s)) else {
        return Ok("Put the caret on a rest: an audio clip starts on one.".into());
    };
    let Some(beat) = beat_at(&score, place) else {
        return Ok("Put the caret on a rest: an audio clip starts on one.".into());
    };
    let had = clip_of(beat);
    if had.is_none() && !takes_a_clip(beat) {
        return Ok("Put the caret on a rest: an audio clip starts on one.".into());
    }
    let relative = had
        .as_ref()
        .map_or_else(|| relative_by_default(host), |c| c.relative);
    let offset = had.as_ref().map_or(0.0, |c| c.offset);
    *shared.editing() = Some(Editing {
        place,
        had,
        relative,
        offset,
        ..Editing::default()
    });
    if let Err(e) = host
        .set_view(DIALOG, properties(shared))
        .and_then(|()| host.show_view(DIALOG))
    {
        *shared.editing() = None;
        return Err(e);
    }
    Ok(String::new())
}

/// The palette's Audio Clip tool.
#[derive(Debug)]
pub struct ClipTool {
    /// The host.
    pub host: Host,
    /// What it shares with the dialog.
    pub shared: Arc<Shared>,
}

impl Tool for ClipTool {
    fn run(&self) -> Result<String, Error> {
        open(&self.host, &self.shared)
    }
}

/// The Audio Clip dialog.
#[derive(Debug)]
pub struct Dialog {
    /// The host.
    pub host: Host,
    /// What it shares with the tool.
    pub shared: Arc<Shared>,
}

/// The offset typed into the dialog, in seconds: a number of seconds
/// ("-0.25", "1,5", "2 s") or of milliseconds ("250 ms"), with a sign for
/// before the rest; empty is none. Kept to the millisecond. `None` when it
/// isn't one, or is more than an hour either way.
pub fn parse_offset(text: &str) -> Option<f64> {
    let t = text.trim().replace(',', ".").replace('\u{2212}', "-");
    let t = t.to_lowercase();
    let (number, unit) = if let Some(n) = t.strip_suffix("ms") {
        (n, 0.001)
    } else if let Some(n) = t.strip_suffix('s') {
        (n, 1.0)
    } else {
        (t.as_str(), 1.0)
    };
    let number = number.trim();
    if number.is_empty() {
        return (unit == 1.0 && t.is_empty()).then_some(0.0);
    }
    // Digits, a point and a sign only: no "inf", "nan" or exponents.
    if !number
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '.' | '+' | '-'))
    {
        return None;
    }
    let seconds = (number.parse::<f64>().ok()? * unit * 1000.0).round() / 1000.0;
    (seconds.is_finite() && seconds.abs() <= OFFSET_MAX).then_some(seconds + 0.0)
}

/// An offset for the dialog's field: "0 s", "+0.25 s", "-1.5 s".
pub fn offset_text(seconds: f64) -> String {
    if seconds == 0.0 {
        "0 s".into()
    } else {
        format!("{seconds:+} s")
    }
}

/// A length for the dialog: "1:02.5" or "10.4 s".
fn length(seconds: f64) -> String {
    if seconds >= 60.0 {
        let m = (seconds / 60.0).floor();
        format!("{m}:{:04.1}", seconds - 60.0 * m)
    } else {
        format!("{seconds:.1} s")
    }
}

/// The window's properties for what it is open for.
pub fn properties(shared: &Shared) -> Vec<(String, View)> {
    let editing = shared.editing();
    let e = editing.clone().unwrap_or_default();
    let (name, seconds) = match (&e.chosen, &e.had, e.removed) {
        (_, _, true) => (None, None),
        (Some((file, s)), _, _) => (
            file.file_name().map(|n| n.to_string_lossy().into_owned()),
            Some(*s),
        ),
        (None, Some(c), _) => (Some(c.name().to_owned()), Some(c.seconds)),
        (None, None, _) => (None, None),
    };
    let text = |s: &str| View::String(s.into());
    vec![
        ("file".into(), text(name.as_deref().unwrap_or("None"))),
        ("has-file".into(), View::Bool(name.is_some())),
        (
            "length".into(),
            text(&seconds.map(length).unwrap_or_default()),
        ),
        (
            "paths".into(),
            View::List(PATHS.iter().map(|p| text(p)).collect()),
        ),
        (
            "path".into(),
            View::Number(f64::from(u8::from(!e.relative))),
        ),
        (
            "path-note".into(),
            text(if e.relative {
                "Kept from the score's folder: move or share that folder with the score and its .oxg file."
            } else {
                "Kept as it is: the score can move, the file stays where it is."
            }),
        ),
        ("offset".into(), text(&offset_text(e.offset))),
        (
            "problem".into(),
            text(e.problem.as_deref().unwrap_or_default()),
        ),
    ]
}

impl Dialog {
    /// The window's callback `callback` with `args` (but OK and Cancel,
    /// which [`Dialog::finish`] takes).
    pub fn on(&self, callback: &str, args: &[View]) {
        match (callback, args.first()) {
            ("choose", _) => self.choose(),
            ("remove", _) => {
                if let Some(e) = self.shared.editing().as_mut() {
                    e.chosen = None;
                    e.removed = true;
                    e.problem = None;
                }
            }
            ("offset-edited", Some(View::String(text))) => {
                if let Some(e) = self.shared.editing().as_mut() {
                    match parse_offset(text) {
                        Some(offset) => {
                            e.offset = offset;
                            e.problem = None;
                        }
                        None => {
                            e.problem = Some(format!(
                                "\"{}\" isn't an offset: type seconds, like -0.25, or milliseconds, like 250 ms, up to an hour.",
                                text.trim()
                            ));
                        }
                    }
                }
            }
            ("path-chosen", Some(View::Number(i))) => {
                if let Some(e) = self.shared.editing().as_mut() {
                    e.relative = *i == 0.0;
                }
            }
            _ => {}
        }
    }

    /// OK writes the clip on the rest, as one change the user can undo;
    /// Cancel, or closing the window, forgets it.
    pub fn finish(&self, ok: bool) -> Result<(), Error> {
        let Some(e) = self.shared.editing().take() else {
            return Ok(());
        };
        if !ok {
            return Ok(());
        }
        let _ = self.host.set_settings(&{
            let mut s = self.host.settings().unwrap_or_default();
            s.insert(
                "paths".into(),
                if e.relative { "relative" } else { "absolute" }.into(),
            );
            s
        });
        let folder = score_folder(&self.host);
        let (clip, said) = match (&e.chosen, &e.had, e.removed) {
            (_, _, true) => (None, None),
            (Some((file, seconds)), _, _) => {
                let (clip, kept) = Clip::of(file, *seconds, e.relative, folder.as_deref());
                (Some(clip), (!kept).then_some(()))
            }
            (None, Some(had), _) if had.relative != e.relative => {
                match had.file(folder.as_deref()) {
                    Some(file) => {
                        let (clip, kept) =
                            Clip::of(&file, had.seconds, e.relative, folder.as_deref());
                        (Some(clip), (!kept).then_some(()))
                    }
                    None => (Some(had.clone()), Some(())),
                }
            }
            (None, Some(had), _) if had.offset != e.offset => (Some(had.clone()), None),
            // Nothing changed.
            _ => return Ok(()),
        };
        // The offset in the dialog, whatever else changed.
        let clip = clip.map(|c| Clip {
            offset: e.offset,
            ..c
        });
        if said.is_some() {
            self.host.notify(
                oxiguitar_plugin::Level::Warning,
                "The score isn't saved yet: the clip's path is kept absolute. Save the score, then choose the path again to keep it relative.",
            );
        }
        let Some((revision, mut score)) = self.host.score2()? else {
            return Ok(());
        };
        let Some(beat) = beat_at_mut(&mut score, e.place) else {
            return Err(Error::new(
                oxiguitar_plugin::ErrorKind::Failed,
                "the rest the dialog was opened for is no longer there",
            ));
        };
        set_clip(beat, clip.as_ref());
        let name = if clip.is_some() {
            "Audio Clip"
        } else {
            "Remove Audio Clip"
        };
        self.host.apply(revision, name, score)
    }

    /// The system's file dialog, then the file read to know its length (and
    /// that it can be played).
    fn choose(&self) {
        let folder = score_folder(&self.host);
        let mut dialog = rfd::FileDialog::new()
            .set_title("Choose an Audio File")
            .add_filter("Audio (WAV, FLAC, MP3, Opus)", &decode::EXTENSIONS);
        if let Some(f) = &folder {
            dialog = dialog.set_directory(f);
        }
        let Some(file) = dialog.pick_file() else {
            return;
        };
        let result = decode::file(&file);
        let mut editing = self.shared.editing();
        let Some(e) = editing.as_mut() else {
            return;
        };
        match result {
            Ok(audio) => {
                e.chosen = Some((file, audio.seconds()));
                e.removed = false;
                e.problem = None;
            }
            Err(why) => {
                let name = file
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                e.problem = Some(format!("{name} {why}."));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxiguitar_plugin::score::{BeatEffects, Duration, NoteValue};
    use oxiguitar_plugin::score2::{Bar2, Staff2, Track2, Voice2};

    fn beat(kind: BeatKind) -> Beat2 {
        Beat2 {
            kind,
            duration: Duration {
                value: NoteValue::Quarter,
                dots: 0,
                tuplets: Vec::new(),
            },
            notes: Vec::new(),
            grace: None,
            effects: BeatEffects::default(),
            chord: None,
            text: String::new(),
            ext: Vec::new(),
        }
    }

    #[test]
    fn a_clip_goes_on_a_beat_and_comes_off_leaving_the_rest() {
        let mut b = beat(BeatKind::Rest);
        b.ext.push(BeatExt::Plugin {
            plugin: "someone.else".into(),
            data: "{}".into(),
        });
        assert!(takes_a_clip(&b) && clip_of(&b).is_none());
        let c = Clip {
            path: "a.wav".into(),
            relative: true,
            seconds: 2.0,
            offset: 0.0,
        };
        set_clip(&mut b, Some(&c));
        assert_eq!(clip_of(&b), Some(c.clone()));
        set_clip(&mut b, Some(&c));
        assert_eq!(b.ext.len(), 2, "one clip a beat");
        set_clip(&mut b, None);
        assert_eq!(b.ext.len(), 1);
        assert!(!takes_a_clip(&beat(BeatKind::Notes)));
    }

    #[test]
    fn the_caret_names_its_beat() {
        let score = Score2 {
            tracks: vec![Track2 {
                staves: vec![Staff2 {
                    bars: vec![Bar2 {
                        clef: oxiguitar_plugin::score::Clef::Treble,
                        voices: vec![Voice2 {
                            beats: vec![beat(BeatKind::Notes), beat(BeatKind::Rest)],
                            ..Voice2::default()
                        }],
                        ext: Vec::new(),
                    }],
                    ..Staff2::default()
                }],
                ..Track2::default()
            }],
            ..Score2::default()
        };
        let caret = Selection2::Caret {
            track: 0,
            staff: 0,
            voice: 0,
            bar: 0,
            beat: 1,
            string: 1,
        };
        let place = first_place(&score, &caret).unwrap();
        assert_eq!(place, [0, 0, 0, 0, 1]);
        assert!(takes_a_clip(beat_at(&score, place).unwrap()));
        assert_eq!(length(10.42), "10.4 s");
        assert_eq!(length(62.5), "1:02.5");
    }

    #[test]
    fn an_offset_is_typed_in_seconds_or_milliseconds() {
        for (text, seconds) in [
            ("0.25", 0.25),
            ("+0.25 s", 0.25),
            ("-1.5", -1.5),
            ("-1,5 s", -1.5),
            ("\u{2212}2", -2.0),
            ("250 ms", 0.25),
            ("-40ms", -0.04),
            ("0.0004", 0.0),
            ("  ", 0.0),
            ("", 0.0),
            ("3600", 3600.0),
        ] {
            assert_eq!(parse_offset(text), Some(seconds), "{text:?}");
        }
        for text in [
            "abc", "inf", "nan", "1e3", "3601", "ms", "s", "1.2.3", "--1",
        ] {
            assert_eq!(parse_offset(text), None, "{text:?}");
        }
        assert!(parse_offset("-0.0001").unwrap().is_sign_positive(), "no -0");
        // What the field shows reads back as itself.
        for seconds in [0.0, 0.25, -1.5, 0.001, -3600.0] {
            assert_eq!(parse_offset(&offset_text(seconds)), Some(seconds));
        }
        assert_eq!(offset_text(0.0), "0 s");
        assert_eq!(offset_text(-1.5), "-1.5 s");
        assert_eq!(offset_text(0.25), "+0.25 s");
    }

    fn get<'a>(p: &'a [(String, View)], name: &str) -> &'a View {
        &p.iter().find(|(n, _)| n == name).unwrap().1
    }

    /// The window shows what it is open for and changes it: remove, the
    /// offset (or why it isn't one), how the path is kept.
    #[test]
    fn the_window_shows_and_changes_the_clip() {
        let shared = Arc::new(Shared::default());
        *shared.editing() = Some(Editing {
            had: Some(Clip {
                path: "take.wav".into(),
                relative: true,
                seconds: 75.0,
                offset: 0.0,
            }),
            relative: true,
            ..Editing::default()
        });
        let dialog = Dialog {
            host: Host::detached(),
            shared: shared.clone(),
        };
        let p = properties(&shared);
        assert_eq!(get(&p, "file"), &View::String("take.wav".into()));
        assert_eq!(get(&p, "length"), &View::String("1:15.0".into()));
        assert_eq!(get(&p, "path"), &View::Number(0.0));
        dialog.on("offset-edited", &[View::String("250 ms".into())]);
        assert_eq!(
            get(&properties(&shared), "offset"),
            &View::String("+0.25 s".into())
        );
        dialog.on("offset-edited", &[View::String("soon".into())]);
        assert!(
            matches!(get(&properties(&shared), "problem"), View::String(p) if p.contains("isn't an offset"))
        );
        dialog.on("path-chosen", &[View::Number(1.0)]);
        assert!(
            matches!(get(&properties(&shared), "path-note"), View::String(n) if n.starts_with("Kept as it is"))
        );
        dialog.on("remove", &[]);
        let p = properties(&shared);
        assert_eq!(get(&p, "has-file"), &View::Bool(false));
        assert_eq!(get(&p, "length"), &View::String(String::new()));
        // Cancel forgets it all.
        dialog.finish(false).unwrap();
        assert!(shared.editing().is_none());
    }

    /// The file compiles as OxiGuitar compiles it, with every property and
    /// callback the window uses.
    #[test]
    fn the_slint_file_has_what_the_window_sets_and_hears() {
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/clips.slint");
        let compiled = oxiguitar_plugin_test::compile_ui_file(&file).unwrap();
        let c = compiled
            .components
            .iter()
            .find(|c| c.name == "ClipWindow")
            .unwrap();
        for (name, _) in properties(&Shared::default()) {
            assert!(c.properties.iter().any(|p| *p == name), "{name}");
        }
        for name in [
            "choose",
            "remove",
            "path-chosen",
            "offset-edited",
            "ok",
            "cancel",
        ] {
            assert!(c.callbacks.iter().any(|p| p == name), "{name}");
        }
    }
}
