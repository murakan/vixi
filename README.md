# vixi

CPU-based image viewer for Linux shell workflows.

## Supported formats

- JPG
- PNG 8-bit and 16-bit
- TIFF 8-bit, 16-bit, and 32-bit float
- Multipage TIFF

## Usage

```bash
cargo run -- path/to/image.tif
cargo run -- path/to/image.tif --page 2
cargo run -- path/to/image.tif --window-center 2048 --window-width 4096
cargo run -- path/to/image.tif --auto-window minmax
vixi path/to/image.tif
```

## Controls

- Mouse wheel: zoom
- Right drag on image: windowing (horizontal: width, vertical: center)
- Scroll bars: pan
- `[` / `]` or left/right arrows: previous/next TIFF page
- `R`: reset windowing
- `I`: invert
- `F`: fit to window
- `Q` / `E`: rotate left/right
- `H` / `V`: flip horizontal/vertical
- `0`: reset rotation and flips

## Notes

Image decoding and windowing are CPU-only. High bit-depth images are converted to
RGBA8 for display after applying windowing.
