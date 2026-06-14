# vixi

`vixi` is a Rust image viewer for shell-first workflows.

It opens the image in a lightweight native window and turns the terminal into a
research-oriented control surface. The window is for looking at pixels; the
terminal TUI is for choosing actions, inspecting statistics, using a command
palette, and reviewing results.

## Supported Formats

- JPEG
- PNG, including 16-bit grayscale/RGB/RGBA PNG
- TIFF 8-bit, 16-bit, and 32-bit float
- Multipage TIFF

## Installation

From source:

```bash
cargo install --path .
```

For development:

```bash
cargo run -- path/to/image.tif
```

The project is designed to be distributed as a portable binary as well. Release
packaging is still intentionally simple while the command and plugin APIs settle.

## Usage

```bash
vixi path/to/image.tif
vixi path/to/image.tif --page 2
vixi path/to/image.tif --window-center 2048 --window-width 4096
vixi path/to/image.tif --auto-window minmax
vixi path/to/image.tif --backend x11
```

After startup, `vixi` opens a native image window and switches the terminal into
a split TUI.

## Terminal TUI

```text
Top bar      current image, page, zoom, window/level
Commands     always-visible command list filtered by typed input
Statistics   area, mean, min, max, display state
Histogram    per-channel histogram for gray/RGB/RGBA images
History      command output, plugin messages, measurement output
Input        REPL-like input and command filter
```

Common keys:

```text
Type       filter the command list or enter a command
Enter      select the highlighted command, fill parameters, or run typed command
Up/Down    select from the filtered command list
Esc        clear command input
Ctrl-U     clear command input
q          quit
?          TUI help
```

Commands with parameters first place a template in the input line. For example,
typing `mean`, selecting `mean <x> <y> <width> <height>`, and pressing Enter
changes the input line to `mean `. You can then enter the parameters and press
Enter again.

## Command Input

```text
open <path>
zoom <value>
pan <dx> <dy>
page next
page prev
page <number>
rotate left
rotate right
fit
invert
reset
brightness <delta>
contrast <factor>
window <center> <width>
pixel <x> <y>
mean <x> <y> <width> <height>
move-window <dx> <dy>
position-window <x> <y>
quit
```

Examples:

```text
zoom 2
pan 100 -40
page next
window 2048 4096
pixel 120 300
mean 10 20 100 80
move-window 120 0
position-window 100 80
```

`move-window` and `position-window` are useful when the image window overlaps
the terminal. They depend on window-system support; X11 generally allows this,
while Wayland compositors may ignore or restrict programmatic window placement.

## Linux Display Backend

On Linux, `vixi` can use either X11 or Wayland through `winit`:

```bash
vixi image.tif --backend auto
vixi image.tif --backend x11
vixi image.tif --backend wayland
```

`auto` prefers X11 when `DISPLAY` is available, then falls back to Wayland when
only `WAYLAND_DISPLAY` is available. If you see EGL or Wayland event-loop errors,
try `--backend x11` first. Use `--backend wayland` to force native Wayland.

## Design

`vixi` separates image display from command control:

- `winit` creates the native X11/Wayland/macOS/Windows window.
- `wgpu` displays an RGBA framebuffer in that window.
- `crossterm` renders the terminal TUI control surface.
- TUI command selection, typed input, and window keyboard input use the same
  command model.
- Image decoding remains CPU-based and preserves high-bit-depth image data until
  display mapping.

## Plugins

The plugin system is local and does not require a web server.

Installed plugins live under `$XDG_DATA_HOME/vixi/plugins` or
`~/.local/share/vixi/plugins`.

```text
:plugin install ./denoise
:plugin list
:run denoise sigma=1.5
:plugin remove denoise
```

Today, `run` executes installed external-command plugins and passes the current
image path in the `VIXI_IMAGE` environment variable. A plugin can print a result
to stdout and return a non-zero exit code to report failure.

The preferred long-term plugin format is WebAssembly loaded by the application
through an embedded runtime such as Wasmtime. `.wasm` files can already be
installed, but executing them is intentionally reserved for the future Wasm host
API.
