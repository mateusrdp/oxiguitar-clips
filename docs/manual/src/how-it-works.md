# Architecture and strategy

The plugin does two things. It draws on the page, which is the speaker, the name and the line to where the clip ends, and it plays as an effect that follows the score. The line takes the score's tempo changes into account, and it ends at a jump in the playing order.

The clip is kept on its rest in the song's `.oxg` file, since no score format holds it. Decoding uses Symphonia for WAV, FLAC and MP3, and `opus-pure` for Opus. Resampling uses Rubato, and the practice speed with the pitch kept uses `timestretch`. All are used as published.

The real-time check confirms that nothing is allocated on the audio thread when clips play, at 100 % speed and away from it.
