// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! OxiGuitar's audio clips plugin (plugin interface 1.13;
//! mateusrdp/oxiguitar#240): an audio file on a rest, engraved from that
//! beat for as long as it lasts and played along with the score.
//!
//! - `dialog` is what the user sees: the palette's Audio Clip tool and the
//!   dialog it opens, with the system's file dialog to choose the file.
//! - `clip` is what a rest keeps, as this plugin's own data on its beat
//!   (saved beside the score in its `.oxg` file): the file's path, relative
//!   to the score or absolute, and its length.
//! - `draw` is the clip on the page: its name, and a line that OxiGuitar
//!   runs across bars and systems to where the clip ends.
//! - `decode` reads the files oxiguitar-export writes (WAV, FLAC, MP3,
//!   Opus); `sound` plays a track's clips, an effect that follows the
//!   score, at the practice speed with the pitch kept.
//!
//! OxiGuitar knows none of this: it places the tool, shows the dialog,
//! carries the data on the beat, asks the plugin what to draw, and tells
//! the effect its track's clips with the times they play.

use std::path::Path;
use std::sync::Arc;

use oxiguitar_plugin::audio::{PlayedBeat, PrepareEffect};
use oxiguitar_plugin::display::Point;
use oxiguitar_plugin::score2::BeatExt;
use oxiguitar_plugin::{
    DrawPlace, DrawTimes, Drawer, Drawn, Effect, Error, Host, Level, Plugin, Processor, Tab, Tool,
};

pub mod clip;
pub mod decode;
pub mod dialog;
pub mod draw;
pub mod sound;

/// The plugin's id, under which it keeps its data on beats.
pub const ID: &str = "oxiguitar.clips";

/// The effect's id in the manifest.
pub const EFFECT: &str = "clips";

/// The tool's id in the manifest.
pub const TOOL: &str = "clip";

/// The plugin.
#[derive(Debug)]
pub struct ClipsPlugin {
    host: Host,
    tool: dialog::ClipTool,
    dialog: dialog::Dialog,
    shared: Arc<dialog::Shared>,
}

impl Plugin for ClipsPlugin {
    fn start(host: Host) -> Result<ClipsPlugin, Error> {
        let shared = Arc::new(dialog::Shared::default());
        Ok(ClipsPlugin {
            tool: dialog::ClipTool {
                host: host.clone(),
                shared: shared.clone(),
            },
            dialog: dialog::Dialog {
                host: host.clone(),
                shared: shared.clone(),
            },
            shared,
            host,
        })
    }

    fn effect(&self, id: &str) -> Option<&dyn Effect> {
        (id == EFFECT).then_some(self as &dyn Effect)
    }

    fn tool(&self, id: &str) -> Option<&dyn Tool> {
        (id == TOOL).then_some(&self.tool as &dyn Tool)
    }

    fn tab(&self, id: &str) -> Option<&dyn Tab> {
        (id == dialog::DIALOG).then_some(&self.dialog as &dyn Tab)
    }

    fn drawer(&self) -> Option<&dyn Drawer> {
        Some(self)
    }
}

impl Drawer for ClipsPlugin {
    fn draw(&self, places: &[DrawPlace], times: &DrawTimes) -> Result<Vec<Drawn>, Error> {
        Ok(draw::draw(places, times))
    }

    /// A click on a clip opens its dialog: the caret is on its rest.
    fn clicked(&self, _place: &DrawPlace, _at: Point, _double: bool) -> Result<String, Error> {
        dialog::open(&self.host, &self.shared)
    }
}

impl Effect for ClipsPlugin {
    /// Without the score's beats there is nothing to play: the track
    /// passes untouched.
    fn prepare(&self, prepare: &PrepareEffect) -> Result<Box<dyn Processor>, Error> {
        self.prepare_following(prepare, &[])
    }

    fn prepare_following(
        &self,
        prepare: &PrepareEffect,
        beats: &[PlayedBeat],
    ) -> Result<Box<dyn Processor>, Error> {
        let folder = self
            .host
            .score_file()
            .ok()
            .flatten()
            .and_then(|f| f.parent().map(Path::to_path_buf));
        let rate = prepare.rate;
        let mut segments = Vec::new();
        let mut unplayable: Vec<String> = Vec::new();
        for b in beats {
            let Some(clip) = b.ext.iter().find_map(|e| match e {
                BeatExt::Plugin { plugin, data } if plugin == ID => clip::Clip::from_text(data),
                _ => None,
            }) else {
                continue;
            };
            let sound = match clip.file(folder.as_deref()) {
                Some(file) => sound::at_rate(&file, rate),
                None => Err("is kept relative to the score, which isn't saved".to_owned()),
            };
            match sound {
                Ok(sound) => segments.push(sound::Segment {
                    start: (b.start * f64::from(rate)).round() as i64,
                    sound,
                }),
                Err(why) => {
                    let said = format!("{} {why}", clip.name());
                    if !unplayable.contains(&said) {
                        unplayable.push(said);
                    }
                }
            }
        }
        if !unplayable.is_empty() {
            self.host.notify(
                Level::Warning,
                format!(
                    "{} plays without its clip: {}.",
                    prepare.track.info.name,
                    unplayable.join("; ")
                ),
            );
        }
        Ok(Box::new(Playing(sound::Clips::new(
            segments,
            rate,
            prepare.frames,
        ))))
    }
}

/// A track's clips, played.
#[derive(Debug)]
struct Playing(sound::Clips);

impl Processor for Playing {
    /// Told nothing of where the score is: nothing plays.
    fn process(&mut self, _left: &mut [f32], _right: &mut [f32]) {}

    fn process_at(&mut self, left: &mut [f32], right: &mut [f32], time: f64, step: f64) {
        self.0.process_at(left, right, time, step);
    }
}

oxiguitar_plugin::export_plugin!(ClipsPlugin);

#[cfg(test)]
mod missing_files {
    use std::sync::Mutex;

    use oxiguitar_plugin::api::message::{HostAnswer, HostRequest, PreparedTrack, TrackInfo};
    use oxiguitar_plugin::{HostServices, InstrumentClass};

    use super::*;

    /// A host that keeps what it is told, with a score that was never saved.
    #[derive(Default)]
    struct Told(Mutex<Vec<String>>);

    impl HostServices for Told {
        fn request(&self, request: HostRequest<'_>) -> Result<HostAnswer<'static>, Error> {
            match request {
                HostRequest::Notify { level, text } => {
                    assert!(matches!(level, Level::Warning), "{text}");
                    self.0.lock().unwrap().push(text.to_string());
                    Ok(HostAnswer::Done)
                }
                HostRequest::ScoreFile => Ok(HostAnswer::ScoreFile(None)),
                _ => Ok(HostAnswer::Done),
            }
        }
    }

    fn beat(start: f64, clip: &clip::Clip) -> PlayedBeat {
        PlayedBeat {
            start,
            end: start + 0.5,
            staff: 0,
            bar: 0,
            voice: 0,
            beat: 0,
            ext: vec![BeatExt::Plugin {
                plugin: ID.into(),
                data: clip.to_text(),
            }],
        }
    }

    /// A clip whose file is gone, and one kept relative to a score that
    /// was never saved, are named once in the footer, and the track plays
    /// on without them: silence where they were.
    #[test]
    fn a_missing_file_is_named_and_the_track_plays_without_it() {
        let told = Arc::new(Told::default());
        let plugin = ClipsPlugin::start(Host::new(told.clone())).unwrap();
        let gone = clip::Clip {
            path: std::env::temp_dir()
                .join("oxiguitar-clips-no-such-file.wav")
                .to_string_lossy()
                .into_owned(),
            relative: false,
            seconds: 1.0,
        };
        let unsaved = clip::Clip {
            path: "take.wav".into(),
            relative: true,
            seconds: 1.0,
        };
        let prepare = PrepareEffect {
            effect: "clips".into(),
            rate: 48_000,
            frames: 256,
            track: PreparedTrack {
                info: TrackInfo {
                    index: 0,
                    name: "Lead".into(),
                    class: InstrumentClass::Fretted,
                },
                program: 27,
                percussion: false,
                tuning: vec![64, 59, 55, 50, 45, 40],
                capo: 0,
                settings: String::new(),
            },
        };
        let beats = [beat(0.0, &gone), beat(1.0, &gone), beat(2.0, &unsaved)];
        let mut playing = plugin.prepare_following(&prepare, &beats).unwrap();
        let said = told.0.lock().unwrap().clone();
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said[0].starts_with("Lead plays without its clip"),
            "{said:?}"
        );
        assert!(
            said[0].contains("oxiguitar-clips-no-such-file.wav"),
            "{said:?}"
        );
        assert!(
            said[0].contains("take.wav is kept relative to the score"),
            "{said:?}"
        );
        assert_eq!(said[0].matches("no-such-file").count(), 1, "named once");
        let (mut left, mut right) = (vec![0.0f32; 256], vec![0.0f32; 256]);
        playing.process_at(&mut left, &mut right, 0.0, 1.0 / 48_000.0);
        assert!(left.iter().chain(&right).all(|s| *s == 0.0));
    }
}
