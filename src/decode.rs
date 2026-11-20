// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! Reading a clip's file: the formats oxiguitar-export writes. WAV, FLAC
//! and MP3 through Symphonia; Opus (Ogg Opus, `.opus`) through opus-pure,
//! as Symphonia has no Opus decoder of its own. The file's format is told
//! by its first bytes, never by its name alone.

use std::path::Path;

use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

/// The extensions the file dialog offers: what oxiguitar-export writes.
pub const EXTENSIONS: [&str; 4] = ["wav", "flac", "mp3", "opus"];

/// A clip's sound: stereo, interleaved, at its own rate.
#[derive(Clone, Debug, PartialEq)]
pub struct Audio {
    /// Frames a second.
    pub rate: u32,
    /// Left and right, frame by frame.
    pub samples: Vec<f32>,
}

impl Audio {
    /// How many frames it has.
    pub fn frames(&self) -> usize {
        self.samples.len() / 2
    }

    /// How long it lasts, in seconds.
    pub fn seconds(&self) -> f64 {
        self.frames() as f64 / f64::from(self.rate.max(1))
    }
}

/// The sound in the file at `path`, or why it can't be read: a sentence
/// for the user.
pub fn file(path: &Path) -> Result<Audio, String> {
    let data = std::fs::read(path).map_err(|e| format!("can't be read: {e}"))?;
    bytes(&data, path.extension().and_then(|e| e.to_str()))
}

/// The sound in `data` (the extension `extension` a hint at most).
pub fn bytes(data: &[u8], extension: Option<&str>) -> Result<Audio, String> {
    let audio = if is_ogg_opus(data) {
        opus(data)?
    } else {
        other(data, extension)?
    };
    if audio.frames() == 0 {
        return Err("holds no sound".to_owned());
    }
    if audio.samples.iter().any(|s| !s.is_finite()) {
        return Err("holds samples that aren't numbers".to_owned());
    }
    Ok(audio)
}

/// An Ogg stream whose first packet is an Opus header (RFC 7845).
fn is_ogg_opus(data: &[u8]) -> bool {
    data.starts_with(b"OggS") && data.get(28..36) == Some(b"OpusHead".as_slice())
}

/// Ogg Opus: the encoder's delay off the front and the end-trim off the
/// back, as RFC 7845 says, through opus-pure's `Trim`.
fn opus(data: &[u8]) -> Result<Audio, String> {
    use opus_pure::{MAX_PACKET_SAMPLES, OggOpusReader, Trim};
    let bad = |e: opus_pure::Error| format!("isn't an Opus file it can read: {e}");
    let mut reader = OggOpusReader::new(std::io::Cursor::new(data)).map_err(bad)?;
    let head = reader.head().clone();
    let channels = usize::from(head.channel_count);
    if !(1..=2).contains(&channels) {
        return Err(format!("has {channels} channels; mono and stereo are read"));
    }
    let mut decoder = head.decoder(48_000).map_err(bad)?;
    let mut trim = Trim::new(&head, 48_000, channels).map_err(bad)?;
    let mut block = vec![0.0f32; MAX_PACKET_SAMPLES * channels];
    let mut samples = Vec::new();
    for packet in reader.packets() {
        let packet = packet.map_err(bad)?;
        let n = decoder
            .decode(&packet.data, MAX_PACKET_SAMPLES, &mut block)
            .map_err(bad)?;
        push_stereo(
            &mut samples,
            trim.keep(&packet, &block[..n * channels]),
            channels,
        );
    }
    Ok(Audio {
        rate: 48_000,
        samples,
    })
}

/// WAV, FLAC and MP3, through Symphonia's probe.
fn other(data: &[u8], extension: Option<&str>) -> Result<Audio, String> {
    let source = MediaSourceStream::new(
        Box::new(std::io::Cursor::new(data.to_vec())),
        Default::default(),
    );
    let mut hint = Hint::new();
    if let Some(e) = extension {
        hint.with_extension(e);
    }
    let unknown =
        |e: SymphoniaError| format!("isn't a WAV, FLAC, MP3 or Opus file it can read ({e})");
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            source,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(unknown)?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or("has no sound in it")?;
    let id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or("has no sound in it")?
        .clone();
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(unknown)?;
    let mut samples = Vec::new();
    let mut rate = params.sample_rate.unwrap_or(0);
    let mut block: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(e) => return Err(unknown(e)),
        };
        if packet.track_id != id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(buf) => {
                let channels = buf.spec().channels().count();
                if !(1..=2).contains(&channels) {
                    return Err(format!("has {channels} channels; mono and stereo are read"));
                }
                rate = buf.spec().rate();
                block.resize(buf.samples_interleaved(), 0.0);
                buf.copy_to_slice_interleaved(&mut block);
                push_stereo(&mut samples, &block, channels);
            }
            // A damaged packet: skipped, as players do.
            Err(SymphoniaError::DecodeError(_)) => {}
            Err(e) => return Err(unknown(e)),
        }
    }
    if rate == 0 {
        return Err("doesn't say its sample rate".to_owned());
    }
    Ok(Audio { rate, samples })
}

/// Adds `block` (`channels` interleaved, 1 or 2) to `out` as stereo.
fn push_stereo(out: &mut Vec<f32>, block: &[f32], channels: usize) {
    if channels == 1 {
        out.reserve(block.len() * 2);
        for s in block {
            out.extend([*s, *s]);
        }
    } else {
        out.extend_from_slice(block);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A 16-bit WAV file of `frames` frames of a ramp, `channels` of them.
    pub(crate) fn wav(rate: u32, channels: u16, frames: usize) -> Vec<u8> {
        let data_len = frames * usize::from(channels) * 2;
        let mut out = Vec::new();
        out.extend(b"RIFF");
        out.extend(((36 + data_len) as u32).to_le_bytes());
        out.extend(b"WAVEfmt ");
        out.extend(16u32.to_le_bytes());
        out.extend(1u16.to_le_bytes());
        out.extend(channels.to_le_bytes());
        out.extend(rate.to_le_bytes());
        out.extend((rate * u32::from(channels) * 2).to_le_bytes());
        out.extend((channels * 2).to_le_bytes());
        out.extend(16u16.to_le_bytes());
        out.extend(b"data");
        out.extend((data_len as u32).to_le_bytes());
        for i in 0..frames {
            for c in 0..channels {
                let v = ((i % 100) as i16 - 50) * 100 + i16::try_from(c).unwrap_or(0);
                out.extend(v.to_le_bytes());
            }
        }
        out
    }

    #[test]
    fn a_wav_file_is_read_as_stereo_at_its_rate() {
        let audio = bytes(&wav(44_100, 1, 4410), Some("wav")).unwrap();
        assert_eq!((audio.rate, audio.frames()), (44_100, 4410));
        assert!((audio.seconds() - 0.1).abs() < 1e-9);
        // Mono doubled to both sides.
        assert_eq!(audio.samples[0], audio.samples[1]);
        assert!((audio.samples[2] - (-49.0 * 100.0 / 32768.0)).abs() < 1e-4);
        let stereo = bytes(&wav(48_000, 2, 480), None).unwrap();
        assert_eq!((stereo.rate, stereo.frames()), (48_000, 480));
        assert_ne!(stereo.samples[0], stereo.samples[1]);
    }

    #[test]
    fn an_opus_file_is_read_whole_and_gapless() {
        use opus_pure::{Application, MAX_PACKET_BYTES, OggOpusWriter, OpusEncoder, OpusHead};
        let (rate, frame) = (48_000, 960);
        let tone: Vec<f32> = (0..frame * 2 * 50)
            .map(|i| ((i / 2) as f32 * 0.05).sin() * 0.3)
            .collect();
        let mut encoder = OpusEncoder::new(rate, 2, Application::Audio).unwrap();
        let head = OpusHead::for_encoder(&encoder, rate as u32);
        let mut writer = OggOpusWriter::new(Vec::new(), head).unwrap();
        let mut packet = vec![0u8; MAX_PACKET_BYTES];
        for block in tone.chunks_exact(frame * 2) {
            let n = encoder.encode(block, frame, &mut packet).unwrap();
            writer.write_packet(&packet[..n]).unwrap();
        }
        let file = writer.finish().unwrap();
        let audio = bytes(&file, Some("opus")).unwrap();
        assert_eq!(audio.rate, 48_000);
        // One second in; back less the delay this simple stream never
        // flushed (opus-pure's own example says the same).
        assert!(
            audio.frames() > 47_000 && audio.frames() <= 48_000,
            "{}",
            audio.frames()
        );
    }

    /// A second of a 440 Hz tone at 48 kHz, stereo.
    fn tone() -> Vec<f32> {
        (0..48_000)
            .flat_map(|i| {
                let s =
                    (f64::from(i) * 440.0 * std::f64::consts::TAU / 48_000.0).sin() as f32 * 0.5;
                [s, s]
            })
            .collect()
    }

    #[test]
    fn a_flac_file_is_read_sample_for_sample() {
        let pcm = tone();
        let ints: Vec<i32> = pcm.iter().map(|s| (s * 32767.0).round() as i32).collect();
        let mut encoder = rusty_flac::Encoder::new(48_000, 2, 16).unwrap();
        let mut file = encoder.stream_info_placeholder().stream_header();
        encoder.push_interleaved(&ints).unwrap();
        file.extend(encoder.take_frames().into_iter().flat_map(|f| f.data));
        let (last, info) = encoder.finish_frames();
        file.extend(last.into_iter().flat_map(|f| f.data));
        let header = info.stream_header();
        file[..header.len()].copy_from_slice(&header);
        let audio = bytes(&file, Some("flac")).unwrap();
        assert_eq!((audio.rate, audio.frames()), (48_000, 48_000));
        for (a, b) in audio.samples.iter().zip(&ints) {
            assert!((a * 32768.0 - *b as f32).abs() < 1.5, "{a} vs {b}");
        }
    }

    #[test]
    fn an_mp3_file_is_read() {
        let mut encoder = rusty_mp3::Mp3Encoder::new(rusty_mp3::Mp3EncoderConfig {
            bitrate_kbps: 192,
            vbr_quality: None,
        });
        encoder.push_pcm_f32(&tone(), 2, 48_000).unwrap();
        encoder.finish();
        let mut file = Vec::new();
        while let Ok(frame) = encoder.next_packet() {
            file.extend(frame);
        }
        if let Some(tag) = encoder.lame_tag_frame() {
            file[..tag.len()].copy_from_slice(tag);
        }
        let audio = bytes(&file, Some("mp3")).unwrap();
        assert_eq!(audio.rate, 48_000);
        // About a second (an MP3's frames may add a little).
        assert!((audio.seconds() - 1.0).abs() < 0.1, "{}", audio.seconds());
        assert!(audio.samples.iter().map(|s| s.abs()).fold(0.0, f32::max) > 0.3);
    }

    #[test]
    fn what_isnt_a_sound_file_says_so() {
        assert!(bytes(b"hello, I'm no sound", Some("wav")).is_err());
        assert!(bytes(&wav(44_100, 2, 0), Some("wav")).is_err());
        assert!(file(Path::new("/nowhere/at/all.wav")).is_err());
    }
}
