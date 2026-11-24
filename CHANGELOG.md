# Changelog

All notable changes to the audio clips plugin. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed

- **CI outlasts a slow Ubuntu mirror**: it installs packages with `scripts/apt-install.sh`, the same file as in every OxiGuitar repository. apt is stopped only when its downloads have not grown for 2 minutes, then tried again on the next mirror; while they grow, it may take as long as it needs (24 Nov 2026: fixed 5-minute tries failed jobs on mirrors at 80 to 185 kB/s).

## [0.1.1] - 2026-11-24

### Added

- **An offset for each clip** (#8): the Audio Clip dialog's **Offset** field, under the path, starts the clip a little after its rest, or before it with a minus, to line up a recording without editing the file. Type seconds (`0.25`, `-1.5 s`) or milliseconds (`250 ms`) and press Enter; it is zero unless set, kept to the millisecond, at most an hour either way. A clip moved before the song's start is cut there. The line on the page still starts at the rest and ends where the sound does, and the red warning goes by where the clip sounds. Scores saved before keep their clips as they were.
- **Every test with one command** (mateusrdp/oxiguitar#273): `scripts/test-all.py` builds and runs all of this repository's tests, and tests the plugin against the newest OxiGuitar release, with a progress bar, a summary to paste into an issue and a log to diagnose from. The manual has a Features page naming the tests of every feature (CI checks it) and a Testing page.

### Changed

- **The manual explains the ideas behind the plugin** (its overview, with pictures), shows how to use it with screenshots, says in Architecture and strategy what it can't do yet, what is left out on purpose and where help is wanted, and has a Credits page listing every project it uses, with links and thanks. The README has a plain-words introduction and a Credits section.

## [0.1.0] - 2026-11-20

### Added

- **Audio clips** (mateusrdp/oxiguitar#240): the palette's **Clip** tool (Plugins section; also Plugins → Audio Clip…) gives the rest at the caret an audio file, chosen in the system's file dialog: WAV, FLAC, MP3 or Opus, the formats the audio export plugin writes. The file's path is kept relative to the score's folder (to move or share the folder whole) or absolute (the score can move, the file stays), as chosen in the dialog; the choice is remembered.
- **On the page**: a speaker and the file's name at the rest, then a line from there for as long as the clip lasts, across beats, bars and systems, with the score's tempo changes, ending in a hook. Clicking it opens the dialog. A clip that runs into the next clip of its track, or past a jump in the playing order (a repeat, an ending, a D.C.), is laid in red; it plays anyway.
- **Played along**: through its track's volume, pan, mute and solo, after the track's instrument and effects; from wherever playing starts, through loops and seeks; in audio exports too. At a practice speed other than 100 % the clip keeps its pitch (timestretch). A clip whose file is missing or unreadable is named in the footer, and its track plays without it.
