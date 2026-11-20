# Changelog

All notable changes to the audio clips plugin. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.0] - 2026-11-20

### Added

- **Audio clips** (mateusrdp/oxiguitar#240): the palette's **Clip** tool (Plugins section; also Plugins → Audio Clip…) gives the rest at the caret an audio file, chosen in the system's file dialog: WAV, FLAC, MP3 or Opus, the formats the audio export plugin writes. The file's path is kept relative to the score's folder (to move or share the folder whole) or absolute (the score can move, the file stays), as chosen in the dialog; the choice is remembered.
- **On the page**: a speaker and the file's name at the rest, then a line from there for as long as the clip lasts, across beats, bars and systems, with the score's tempo changes, ending in a hook. Clicking it opens the dialog. A clip that runs into the next clip of its track, or past a jump in the playing order (a repeat, an ending, a D.C.), is laid in red; it plays anyway.
- **Played along**: through its track's volume, pan, mute and solo, after the track's instrument and effects; from wherever playing starts, through loops and seeks; in audio exports too. At a practice speed other than 100 % the clip keeps its pitch (timestretch). A clip whose file is missing or unreadable is named in the footer, and its track plays without it.
