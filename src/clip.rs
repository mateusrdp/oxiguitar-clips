// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! What a rest keeps: the plugin's own data on its beat (interface 1.8),
//! a JSON object, which OxiGuitar saves beside the score in its `.oxg`
//! file. The clip's length is kept with it, so the page can be engraved
//! without opening the file.

use std::path::{Path, PathBuf};

use oxiguitar_plugin::serde_json::{self, Value};

/// A clip on a rest.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    /// Where the file is: relative to the score's folder, with `/` between
    /// folders, or absolute.
    pub path: String,
    /// The path is relative to the score's folder.
    pub relative: bool,
    /// How long the file lasts, in seconds, when it was chosen.
    pub seconds: f64,
}

impl Clip {
    /// The clip kept as `text`, if it is one.
    pub fn from_text(text: &str) -> Option<Clip> {
        let v: Value = serde_json::from_str(text).ok()?;
        let path = v.get("path")?.as_str()?.to_owned();
        let seconds = v.get("seconds")?.as_f64()?;
        if path.is_empty() || !seconds.is_finite() || seconds <= 0.0 {
            return None;
        }
        Some(Clip {
            path,
            relative: v.get("relative").and_then(Value::as_bool).unwrap_or(false),
            seconds,
        })
    }

    /// The clip as kept on its beat.
    pub fn to_text(&self) -> String {
        serde_json::json!({
            "path": self.path,
            "relative": self.relative,
            "seconds": self.seconds,
        })
        .to_string()
    }

    /// The file's name, for the page and the dialog.
    pub fn name(&self) -> &str {
        self.path
            .rsplit(['/', '\\'])
            .next()
            .filter(|n| !n.is_empty())
            .unwrap_or(&self.path)
    }

    /// Where the file is, for a score in the folder `score_folder` (`None`
    /// for a score never saved): `None` for a relative path without one.
    pub fn file(&self, score_folder: Option<&Path>) -> Option<PathBuf> {
        if self.relative {
            let mut out = score_folder?.to_path_buf();
            out.extend(self.path.split('/').filter(|p| !p.is_empty()));
            Some(out)
        } else {
            Some(PathBuf::from(&self.path))
        }
    }

    /// The clip for `file` (absolute), kept relative to `score_folder` when
    /// `relative` and the score has one; otherwise absolute. Says whether
    /// it could keep it relative.
    pub fn of(
        file: &Path,
        seconds: f64,
        relative: bool,
        score_folder: Option<&Path>,
    ) -> (Clip, bool) {
        let rel = relative
            .then(|| pathdiff::diff_paths(file, score_folder?))
            .flatten()
            .and_then(|p| {
                let parts: Option<Vec<&str>> =
                    p.components().map(|c| c.as_os_str().to_str()).collect();
                parts.map(|p| p.join("/"))
            });
        match rel {
            Some(path) => (
                Clip {
                    path,
                    relative: true,
                    seconds,
                },
                true,
            ),
            None => (
                Clip {
                    path: file.to_string_lossy().into_owned(),
                    relative: false,
                    seconds,
                },
                !relative,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clip_is_kept_as_text_and_read_back() {
        let clip = Clip {
            path: "audio/intro.wav".into(),
            relative: true,
            seconds: 10.5,
        };
        assert_eq!(Clip::from_text(&clip.to_text()), Some(clip.clone()));
        assert_eq!(clip.name(), "intro.wav");
        assert_eq!(Clip::from_text("{}"), None);
        assert_eq!(Clip::from_text(r#"{"path": "a.wav", "seconds": 0}"#), None);
        assert_eq!(Clip::from_text("not json"), None);
    }

    #[test]
    fn a_relative_path_follows_the_score_and_an_absolute_one_doesnt() {
        let folder = Path::new("/songs/mine");
        let file = Path::new("/songs/mine/audio/intro.wav");
        let (rel, kept) = Clip::of(file, 2.0, true, Some(folder));
        assert!(kept);
        assert_eq!(rel.path, "audio/intro.wav");
        assert_eq!(
            rel.file(Some(Path::new("/elsewhere"))),
            Some(Path::new("/elsewhere/audio/intro.wav").to_path_buf())
        );
        assert_eq!(rel.file(None), None);
        // Above the score's folder too.
        let (up, _) = Clip::of(Path::new("/songs/clips/a.wav"), 2.0, true, Some(folder));
        assert_eq!(up.path, "../clips/a.wav");
        // Absolute: where it is, wherever the score goes.
        let (abs, kept) = Clip::of(file, 2.0, false, Some(folder));
        assert!(kept && !abs.relative);
        assert_eq!(abs.file(Some(Path::new("/x"))), Some(file.to_path_buf()));
        // Relative asked of a score never saved: absolute, and said so.
        let (fallback, kept) = Clip::of(file, 2.0, true, None);
        assert!(!kept && !fallback.relative);
    }
}
