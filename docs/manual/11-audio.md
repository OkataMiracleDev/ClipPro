# Sound

ClipPro listens to every clip as it analyses the picture, in 100 ms steps.

## Measures

- **Loudness**: EBU R128, momentary and short-term.
- **Clean**: no clipping, including clipping that a compressed codec has smoothed over.
- **Noise floor**: how loud the quiet parts are.
- **Speech presence**: whether someone is talking.
- **Wind** and **Hum** (50 or 60 Hz mains hum and its harmonics).

## Flags

Audio clipping, Silence, Wind, Hum, Dead channel (a channel with nothing on it) and Channel imbalance.

The **Audio-First** preset is built for podcasts and vlogs: it rewards clean, present speech and leaves out silence.

## Several audio streams

If a camera recorded more than one audio stream, right-click the clip and choose **Audio stream ›** to pick which one ClipPro measures and plays. The clip's sound is analysed again.

## Listening

The timeline shows each clip's waveform. While playing, the meters show every channel; click a channel to mute it, Alt-click to solo it.
