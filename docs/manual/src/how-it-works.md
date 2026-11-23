# Architecture and strategy

## How it fits into OxiGuitar

The plugin does two jobs through OxiGuitar's plugin interface:

1. It **draws on the page**: the speaker, the file's name and the line to where the clip ends. The line follows the score's tempo changes and stops at a jump in the playing order (a repeat, a D.C.).
2. It **plays as an effect that follows the score**: OxiGuitar tells it which beats the track plays, from wherever playing starts and through loops, and the plugin mixes in each clip from its beat.

The clip is kept on its rest in the song's `.oxg` file beside it, since no score format has a place for it. Its path is stored relative to the score or absolute, as you choose.

Audio files are decoded with Symphonia (WAV, FLAC and MP3) and opus-pure (Opus), resampled with Rubato, and stretched at practice speeds with timestretch. All are used as published. The files are decoded and the stretcher set up before the song starts; a real-time check confirms nothing is allocated on the audio thread while clips play.

## Current limitations

- **Clips start on rests only.** A clip can't start on a note.
- **No trimming, fades or gain.** The whole file plays, at its own level, through the track's volume.
- **Overlaps are flagged, not managed.** A clip that runs into the next clip of its track, or past a repeat or a D.C., is drawn in red. It still plays on, even where the score goes elsewhere.
- **No waveform** on the page, only the line.
- **Files must stay put.** If you move the audio files, pick the path again. A missing file is named in the footer and the track plays without it.

## Left out on purpose

- **Recording.** OxiGuitar plays scores; it has no audio input. A multitrack recorder is a different program, and there are excellent free ones.
- **Audio editing.** Cutting, fading and processing audio belong in an audio editor. Prepare the clip there, then lay it on the score.
- **Embedding audio in the score.** The clip stays a separate file, so the score stays small and nothing gets copied behind your back.

## Where you can help

- **Clip gain and fades**, saved with the clip on its rest.
- **A waveform** drawn along the line on the page.
- **Starting on notes**, which needs a way to pick the clip's beat that doesn't fight with editing the note.

Every change needs its tests and a row on the [Features](features.md) page; the [Testing](testing.md) page says how to run them.
