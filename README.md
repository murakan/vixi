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

Formats are detected from file contents, so a misnamed extension is handled
correctly. Animated and multipage inputs are navigated with the `next` / `prev`
/ `page` commands; each frame's display delay is shown in the status panel.
Timed playback arrives with video support.

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
