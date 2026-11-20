// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! The clip on the page (interface 1.13): at its rest, a small speaker and
//! the file's name, then a line for as long as the clip lasts, which
//! OxiGuitar draws across beats, bars and systems to the point where the
//! clip ends, with the score's tempo changes. Its hook marks the end.
//!
//! What can't play as it looks is laid in the warning red: a clip that
//! runs into the next clip of its track, or past a jump in the playing
//! order (a repeat, an ending, a D.C.), where the score goes elsewhere and
//! the clip plays on. Both still play; the red only says so.

use oxiguitar_plugin::display::{Anchor, Font, Item, Paint, PathCmd, Point};
use oxiguitar_plugin::score2::BeatExt;
use oxiguitar_plugin::{DrawPlace, DrawTimes, Drawn, DrawnRun, PlayedBar};

use crate::clip::Clip;

/// The name's size, in staff spaces.
const TEXT: f32 = 1.2;
/// The box's height.
const HEIGHT: f32 = 1.8;
/// The most characters of the file's name shown.
const NAME_MAX: usize = 24;

/// The clip a place carries, if any.
pub fn clip_at(place: &DrawPlace) -> Option<Clip> {
    match place {
        DrawPlace::Beat { beat, .. } => beat.ext.iter().find_map(|e| match e {
            BeatExt::Plugin { plugin, data } if plugin == crate::ID => Clip::from_text(data),
            _ => None,
        }),
        DrawPlace::Bar { .. } => None,
    }
}

/// What is drawn at `places`, played as `times` says.
pub fn draw(places: &[DrawPlace], times: &DrawTimes) -> Vec<Drawn> {
    let clips: Vec<Option<(u32, Clip)>> = places
        .iter()
        .map(|p| {
            let track = match p {
                DrawPlace::Beat { at, .. } => at[0],
                DrawPlace::Bar { at, .. } => at[0],
            };
            clip_at(p).map(|c| (track, c))
        })
        .collect();
    let start = |i: usize| times.starts.get(i).and_then(|s| s.first()).copied();
    clips
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let (track, clip) = c.as_ref()?;
            let warned = start(i).is_some_and(|from| {
                let to = from + clip.seconds;
                // Another clip of the track starts before this one ends.
                let overlaps = clips.iter().enumerate().any(|(j, o)| {
                    j != i
                        && o.as_ref().is_some_and(|(t, _)| t == track)
                        && start(j).is_some_and(|s| s >= from && s < to && (s > from || j > i))
                });
                overlaps || crosses_a_jump(&times.bars, from, to)
            });
            Some(drawn(i, clip, start(i).is_some(), warned))
        })
        .collect()
}

/// Whether the music between `from` and `to` seconds goes through a jump
/// in the playing order: a bar followed by one that isn't the next.
pub fn crosses_a_jump(bars: &[PlayedBar], from: f64, to: f64) -> bool {
    let Some(mut k) = bars.iter().position(|b| from >= b.start && from < b.end) else {
        return false;
    };
    while to > bars[k].end + 1e-9 {
        match bars.get(k + 1) {
            Some(next) if next.bar == bars[k].bar + 1 => k += 1,
            Some(_) => return true,
            // The song ends first: nothing else plays there.
            None => return false,
        }
    }
    false
}

/// The box for `clip` at place `place`, with its run when the place is
/// played.
fn drawn(place: usize, clip: &Clip, played: bool, warned: bool) -> Drawn {
    let name: String = {
        let n = clip.name();
        if n.chars().count() > NAME_MAX {
            let mut s: String = n.chars().take(NAME_MAX - 1).collect();
            s.push('…');
            s
        } else {
            n.to_owned()
        }
    };
    // About half an em a character, in the text font.
    let width = (1.7 + name.chars().count() as f32 * 0.55 * TEXT + 0.3).min(40.0);
    let p = Point::new;
    let mid = HEIGHT / 2.0;
    let mut items = Vec::new();
    if warned {
        items.push(Item::Rect {
            at: p(0.0, 0.0),
            width,
            height: HEIGHT,
            paint: Paint::Warning,
        });
    }
    // A speaker: its body and its cone.
    items.push(Item::Path {
        cmds: vec![
            PathCmd::MoveTo(p(0.2, mid - 0.3)),
            PathCmd::LineTo(p(0.55, mid - 0.3)),
            PathCmd::LineTo(p(1.0, mid - 0.7)),
            PathCmd::LineTo(p(1.0, mid + 0.7)),
            PathCmd::LineTo(p(0.55, mid + 0.3)),
            PathCmd::LineTo(p(0.2, mid + 0.3)),
            PathCmd::Close,
        ],
        fill: Some(Paint::Ink),
        stroke: None,
    });
    items.push(Item::Text {
        text: name,
        at: p(1.5, mid + 0.4 * TEXT),
        size: TEXT,
        font: Font::Sans,
        anchor: Anchor::Start,
        paint: Paint::Ink,
    });
    Drawn {
        place: place as u32,
        width,
        height: HEIGHT,
        items,
        run: played.then(|| DrawnRun {
            seconds: clip
                .seconds
                .min(oxiguitar_plugin::api::message::RUN_SECONDS_MAX),
            width: 0.12,
            paint: if warned { Paint::Warning } else { Paint::Ink },
            dashed: false,
            hook: true,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxiguitar_plugin::score::{BeatEffects, BeatKind, Duration, NoteValue};
    use oxiguitar_plugin::score2::Beat2;

    fn rest(clip: Option<&Clip>) -> Beat2 {
        Beat2 {
            kind: BeatKind::Rest,
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
            ext: clip
                .map(|c| BeatExt::Plugin {
                    plugin: crate::ID.into(),
                    data: c.to_text(),
                })
                .into_iter()
                .collect(),
        }
    }

    fn place(track: u32, bar: u32, clip: Option<&Clip>) -> DrawPlace {
        DrawPlace::Beat {
            at: [track, 0, bar, 0, 0],
            beat: Box::new(rest(clip)),
        }
    }

    fn clip(seconds: f64) -> Clip {
        Clip {
            path: "audio/intro.wav".into(),
            relative: true,
            seconds,
        }
    }

    /// Bars of two seconds, played in `order`.
    fn bars(order: &[u32]) -> Vec<PlayedBar> {
        order
            .iter()
            .enumerate()
            .map(|(i, &bar)| PlayedBar {
                bar,
                start: 2.0 * i as f64,
                end: 2.0 * (i + 1) as f64,
            })
            .collect()
    }

    #[test]
    fn a_clip_is_its_name_and_a_run_as_long_as_it_lasts() {
        let c = clip(10.0);
        let places = [place(0, 0, Some(&c)), place(0, 1, None)];
        let times = DrawTimes {
            starts: vec![vec![1.0], vec![2.0]],
            bars: bars(&[0, 1, 2, 3, 4, 5, 6, 7]),
        };
        let drawn = draw(&places, &times);
        assert_eq!(drawn.len(), 1);
        let d = &drawn[0];
        assert_eq!(d.place, 0);
        assert!(d.problem().is_none(), "{:?}", d.problem());
        let run = d.run.as_ref().unwrap();
        assert_eq!((run.seconds, run.hook, run.paint), (10.0, true, Paint::Ink));
        assert!(
            d.items
                .iter()
                .any(|i| matches!(i, Item::Text { text, .. } if text == "intro.wav"))
        );
        assert!(!d.items.iter().any(|i| matches!(
            i,
            Item::Rect {
                paint: Paint::Warning,
                ..
            }
        )));
        // Never played: no run.
        let never = draw(&places, &DrawTimes::default());
        assert!(never[0].run.is_none());
    }

    #[test]
    fn a_clip_into_the_next_or_past_a_repeat_is_laid_in_red() {
        let long = clip(5.0);
        let places = [
            place(0, 0, Some(&long)),
            place(0, 1, Some(&long)),
            place(1, 1, Some(&long)),
        ];
        let times = DrawTimes {
            starts: vec![vec![0.0], vec![2.0], vec![2.0]],
            bars: bars(&[0, 1, 2, 3, 4, 5]),
        };
        let drawn = draw(&places, &times);
        let red = |d: &Drawn| d.run.as_ref().unwrap().paint == Paint::Warning;
        // The first runs into the second of its track; the second runs on
        // freely; the other track's isn't its business.
        assert!(red(&drawn[0]));
        assert!(!red(&drawn[1]));
        assert!(!red(&drawn[2]));
        // Bar 1 repeated: a clip from bar 0 past its end crosses the jump.
        let times = DrawTimes {
            starts: vec![vec![0.0]],
            bars: bars(&[0, 1, 0, 1, 2]),
        };
        assert!(red(&draw(&places[..1], &times)[0]));
        assert!(crosses_a_jump(&bars(&[0, 1, 0, 1]), 0.0, 4.5));
        assert!(!crosses_a_jump(&bars(&[0, 1, 0, 1]), 0.0, 3.5));
        // Past the song's end isn't a jump.
        assert!(!crosses_a_jump(&bars(&[0, 1]), 0.0, 60.0));
    }

    #[test]
    fn a_long_name_is_cut_and_the_box_keeps_the_limits() {
        let c = Clip {
            path: format!("/{}.wav", "a-very-long-name-".repeat(10)),
            relative: false,
            seconds: 1.0e9,
        };
        let d = &draw(
            &[place(0, 0, Some(&c))],
            &DrawTimes {
                starts: vec![vec![0.0]],
                bars: bars(&[0]),
            },
        )[0];
        assert!(d.problem().is_none(), "{:?}", d.problem());
        assert!(
            d.items
                .iter()
                .any(|i| matches!(i, Item::Text { text, .. } if text.ends_with('…')))
        );
    }
}
