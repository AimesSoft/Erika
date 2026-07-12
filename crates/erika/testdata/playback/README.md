# Deterministic playback fixture

`playback-fixture.mkv` is a small, deterministic input for Erika playback
state and track-switching tests. It is eight seconds long and contains, in
this exact order:

Every input and output in this directory is original synthetic material made
for the Erika repository. The video and audio come from deterministic FFmpeg
filters, and the subtitle cues and supporting files were authored for this
fixture. No third-party media is included. The inputs, generated output,
script, and documentation are distributed under the repository root's
MPL-2.0 license.

| Index | Type | Contents |
| ---: | --- | --- |
| 0 | Video | MPEG-4 Part 2, 160x90, 30 fps, GOP 30, no B-frames |
| 1 | Audio | FLAC, mono 48 kHz, 880 Hz tone for the first 100 ms of each second |
| 2 | Audio | FLAC, mono 48 kHz, 1320 Hz tone for the first 100 ms of each second |
| 3 | Subtitle | SubRip `track-a.srt`, one labelled cue per second |
| 4 | Subtitle | SubRip `track-b.srt`, one shorter offset cue per second |

The video has 240 frames. Its only keyframes are at 0, 1, 2, 3, 4, 5, 6,
and 7 seconds. These regular landmarks make seek, replay, rate, and track
selection assertions independent of network or decoder timing.

## Verify

Generation is deliberately pinned to FFmpeg and FFprobe 7.1.1. The script
uses bitexact flags and single-threaded native MPEG-4/FLAC encoders. It builds
the file twice, compares the results byte-for-byte, checks the stream layout,
duration, frame count, B-frame count, and keyframe timestamps, then compares
the result and checksums with the committed files.

```sh
cd crates/erika/testdata/playback
./generate.sh
```

To intentionally replace the fixture and refresh `SHA256SUMS` after changing
the generator or either subtitle source:

```sh
./generate.sh --update
```

Review the resulting binary and checksum diff before committing it. Different
FFmpeg versions are rejected because encoder or Matroska muxer changes can
otherwise produce a structurally valid but byte-different fixture.
