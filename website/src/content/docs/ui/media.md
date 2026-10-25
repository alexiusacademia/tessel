---
title: Sound and video
description: Play sound files with playSound and control them through a Sound, and show videos with VideoPlayer, whose controls Tessel draws.
---

The system decodes sound and video (AVFoundation on macOS, Media
Foundation on Windows), so the files it plays play: MP3, M4A/AAC, WAV and
more for sound; MP4 and MOV (H.264, HEVC) for video. Tessel draws the
video's pictures and controls itself, like its other views.

## Playing a sound

```text
playSound(_ path: String, volume: Float = 1.0, loop: Bool = false) -> Sound
```

Starts playing the file and gives a `Sound` to control it:

```tessel
app Jukebox(width: 320, height: 200) {
    state music: Sound? = nil

    HStack {
        Button("Play") { music = playSound(joinPath(appFolder(), "theme.mp3"), loop: true) }
        Button("Pause") { music?.pause() }
        Button("Resume") { music?.resume() }
        Button("Stop") {
            music?.stop()
            music = nil
        }
    }
}
```

- `volume` is from 0 (silent) to 1. `loop: true` plays it over and over.
- A file that can't be played (it isn't there, or isn't sound) gives a
  `Sound` that does nothing, and a warning on the terminal.
- A short sound is played and forgotten: `playSound(path)` on its own.
- Use `joinPath(appFolder(), …)` for files that come with the app (see
  [appPath and appFolder](/reference/stdlib/#apppath-and-appfolder)).

### Sound

| Method | |
|---|---|
| `pause()` | Stops where it is. |
| `resume()` | Goes on from there (from the start, after its end). |
| `stop()` | Stops, and lets the sound go: it can't be resumed. |
| `seek(to: Float)` | Goes to that many seconds in. |
| `setVolume(Float)` | From 0 to 1. |
| `isPlaying() -> Bool` | Whether it's playing now. |
| `position() -> Float` | Where it is, in seconds. |
| `duration() -> Float` | How long it is, in seconds (0 until that's known). |

## Showing a video

```text
VideoPlayer(_ path: String, playing: bind Bool = …, position: bind Float = …,
            controls: Bool = true, loop: Bool = false, volume: Float = 1.0)
```

```tessel
app Lecture(width: 720, height: 480) {
    state playing = false
    state at = 0.0

    VStack {
        VideoPlayer(joinPath(appFolder(), "lecture.mp4"), playing: playing, position: at)
        Text("{Int(at) / 60} min {Int(at) % 60} s")
    }
    .padding(12)
}
```

- **Controls**: a play (pause) button, a bar that shows where it is and is
  clicked to go there, and its time. They show while it's paused, and while
  the pointer is over it. A click elsewhere on the picture plays or pauses.
  `controls: false` leaves them out.
- **`playing:`**, when it's bound, plays it (`true`) or pauses it, and is
  set back to `false` when the video comes to its end. Without it, the
  video starts paused.
- **`position:`**, when it's bound, is where it is in seconds, kept up to
  date as it plays (a few times a second); setting it jumps there.
- **Size**: the video's own, made smaller to fit the space there is (like
  an [`Image`](/reference/views/#image)); a `.frame` scales it to fit. The
  picture keeps its shape, on black.

## In tests

In a [headless run](/ui/testing/), the players play nothing: each file
there is said to be a minute long, and time goes by on the script's clock
(`wait 2` moves them two seconds on). `tree` shows a video's state:

```text
VideoPlayer "lecture.mp4" playing 2.0/60.0
```
