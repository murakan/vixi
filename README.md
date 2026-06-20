# vixi

CPU-based image viewer for Linux shell workflows.

## Supported formats

- JPG
- PNG 8-bit and 16-bit
- TIFF 8-bit, 16-bit, and 32-bit float
- Multipage TIFF

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
