# vixi

CPU-based image viewer for Linux shell workflows.

## Supported formats

- JPEG
- PNG 8-bit and 16-bit, plus animated APNG (frames exposed as pages)
- GIF, including animated GIF (frames exposed as pages)
- WebP, including animated WebP (frames exposed as pages)
- TIFF 8-bit, 16-bit, and 32-bit float, including multipage TIFF
- BMP, ICO, TGA, DDS, PNM, QOI, farbfeld
- HDR and OpenEXR (high dynamic range)
- Video (MP4, MKV, MOV, WebM, AVI, and more) via FFmpeg, with streaming playback

Image formats are detected from file contents, so a misnamed extension is
handled correctly. Animated and multipage inputs are navigated with the
`next` / `prev` / `page` commands; each frame's display delay is shown in the
status panel.

## Video

Video files are decoded by a streaming decoder thread (FFmpeg/libav) and start
playing automatically. Control playback from the terminal:

- `play`, `pause`, `toggle`: start/stop playback
- `seek <s>`, `seek +<s>`, `seek -<s>`: jump to an absolute or relative position
- `speed <multiplier>`: change playback speed (e.g. `speed 2`, `speed 0.5`)
- `step`: advance a single frame while paused (`next` also steps; `prev` nudges back)

The status panel shows the playback state, position/duration, and frame rate.

### Building with video support

Video support is behind the default `video` feature and links against the
FFmpeg development libraries (libavcodec, libavformat, libavutil, libswscale)
via `ffmpeg-next`. Install them before building, for example on Debian/Ubuntu:

```bash
sudo apt-get install -y pkg-config libavcodec-dev libavformat-dev \
    libavutil-dev libswscale-dev
```

To build without video support (no FFmpeg dependency), disable the feature:

```bash
cargo build --no-default-features
```

## Architecture

The image is displayed in a windowing-system window, while all control happens
from the terminal. Launching `vixi` opens the display window and starts a REPL in
the terminal: you type commands, and a TUI status panel above the prompt reflects
the current state. The display window is intentionally control-free — it only
shows the image.

## Usage

```bash
cargo run -- path/to/image.tif
cargo run -- path/to/image.tif --page 2
cargo run -- path/to/image.tif --window-center 2048 --window-width 4096
cargo run -- path/to/image.tif --auto-window minmax
vixi path/to/image.tif
vixi path/to/video.mp4
```

## Terminal commands

Type these at the `vixi>` prompt (`help` shows the full list):

- `next` / `n`, `prev` / `p`, `page <index>`: page navigation (zero-based)
- `zoom in|out|reset|<factor>`, `zoom fit` / `fit`: zoom control
- `pan <dx> <dy>`, `pan reset`: move the view
- `window <center> <width>`, `window center <v>`, `window width <v>`: windowing
- `reset`: auto windowing, `autowindow minmax|percentile`: auto mode
- `invert`: toggle inverted intensity
- `rotate left|right` (`rl` / `rr`), `flip x|y`, `orient reset`
- `status` / `info`, `help`, `quit` (or Ctrl-D)

## Notes

Image decoding and windowing are CPU-only. High bit-depth images are converted to
RGBA8 for display after applying windowing.
