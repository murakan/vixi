// Copyright (c) 2026 Kan Murata
// This software is released under the MIT License, see LICENSE.

use crate::windowing::AutoWindowMode;

/// A control command issued from the terminal REPL to the display window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    /// Show the help text (handled locally in the REPL, never sent to the GUI).
    Help,
    /// Request a fresh status snapshot.
    Status,
    /// Close the viewer.
    Quit,
    NextPage,
    PrevPage,
    GotoPage(usize),
    ZoomIn,
    ZoomOut,
    ZoomSet(f32),
    Fit,
    Pan(f32, f32),
    PanReset,
    ResetWindow,
    SetWindowCenter(f32),
    SetWindowWidth(f32),
    SetWindow { center: f32, width: f32 },
    AutoWindow(AutoWindowMode),
    Invert,
    RotateLeft,
    RotateRight,
    FlipX,
    FlipY,
    OrientReset,
}

/// Parse a single line of REPL input into a [`Command`].
///
/// Returns `Ok(None)` for blank lines, `Err` with a human readable message for
/// invalid input.
pub fn parse_command(line: &str) -> Result<Option<Command>, String> {
    let mut tokens = line.split_whitespace();
    let Some(head) = tokens.next() else {
        return Ok(None);
    };
    let rest: Vec<&str> = tokens.collect();

    let command = match head.to_ascii_lowercase().as_str() {
        "help" | "h" | "?" => Command::Help,
        "status" | "info" | "s" => Command::Status,
        "quit" | "q" | "exit" => Command::Quit,
        "next" | "n" => Command::NextPage,
        "prev" | "p" => Command::PrevPage,
        "page" => Command::GotoPage(parse_index(arg(&rest, 0, "page <index>")?)?),
        "zoom" | "z" => parse_zoom(&rest)?,
        "fit" | "f" => Command::Fit,
        "pan" => parse_pan(&rest)?,
        "window" | "w" => parse_window(&rest)?,
        "reset" | "r" => Command::ResetWindow,
        "autowindow" | "aw" => parse_auto_window(&rest)?,
        "invert" | "inv" => Command::Invert,
        "rotate" | "rot" => parse_rotate(&rest)?,
        "rl" => Command::RotateLeft,
        "rr" => Command::RotateRight,
        "flip" => parse_flip(&rest)?,
        "orient" => parse_orient(&rest)?,
        other => return Err(format!("unknown command: '{other}' (type 'help')")),
    };

    Ok(Some(command))
}

fn parse_zoom(rest: &[&str]) -> Result<Command, String> {
    let arg = arg(rest, 0, "zoom in|out|reset|fit|<factor>")?;
    Ok(match arg.to_ascii_lowercase().as_str() {
        "in" | "+" => Command::ZoomIn,
        "out" | "-" => Command::ZoomOut,
        "reset" | "1" => Command::ZoomSet(1.0),
        "fit" => Command::Fit,
        value => {
            let factor = parse_float(value)?;
            if factor <= 0.0 {
                return Err("zoom factor must be a positive number".to_owned());
            }
            Command::ZoomSet(factor)
        }
    })
}

fn parse_pan(rest: &[&str]) -> Result<Command, String> {
    if rest.first().map(|v| v.eq_ignore_ascii_case("reset")) == Some(true) {
        return Ok(Command::PanReset);
    }
    let dx = parse_float(arg(rest, 0, "pan <dx> <dy> | pan reset")?)?;
    let dy = parse_float(arg(rest, 1, "pan <dx> <dy> | pan reset")?)?;
    Ok(Command::Pan(dx, dy))
}

fn parse_window(rest: &[&str]) -> Result<Command, String> {
    match rest.first().map(|v| v.to_ascii_lowercase()) {
        Some(key) if key == "center" || key == "c" => Ok(Command::SetWindowCenter(parse_float(
            arg(rest, 1, "window center <value>")?,
        )?)),
        Some(key) if key == "width" || key == "w" => Ok(Command::SetWindowWidth(parse_float(
            arg(rest, 1, "window width <value>")?,
        )?)),
        _ => {
            let center = parse_float(arg(rest, 0, "window <center> <width>")?)?;
            let width = parse_float(arg(rest, 1, "window <center> <width>")?)?;
            Ok(Command::SetWindow { center, width })
        }
    }
}

fn parse_auto_window(rest: &[&str]) -> Result<Command, String> {
    let arg = arg(rest, 0, "autowindow minmax|percentile")?;
    Ok(match arg.to_ascii_lowercase().as_str() {
        "minmax" | "mm" => Command::AutoWindow(AutoWindowMode::MinMax),
        "percentile" | "pct" | "p" => Command::AutoWindow(AutoWindowMode::Percentile {
            low: 1.0,
            high: 99.0,
        }),
        other => return Err(format!("unknown auto-window mode: '{other}'")),
    })
}

fn parse_rotate(rest: &[&str]) -> Result<Command, String> {
    let arg = arg(rest, 0, "rotate left|right")?;
    Ok(match arg.to_ascii_lowercase().as_str() {
        "left" | "l" => Command::RotateLeft,
        "right" | "r" => Command::RotateRight,
        other => return Err(format!("rotation must be left or right: '{other}'")),
    })
}

fn parse_flip(rest: &[&str]) -> Result<Command, String> {
    let arg = arg(rest, 0, "flip x|y")?;
    Ok(match arg.to_ascii_lowercase().as_str() {
        "x" | "h" => Command::FlipX,
        "y" | "v" => Command::FlipY,
        other => return Err(format!("flip axis must be x or y: '{other}'")),
    })
}

fn parse_orient(rest: &[&str]) -> Result<Command, String> {
    let arg = arg(rest, 0, "orient reset")?;
    match arg.to_ascii_lowercase().as_str() {
        "reset" | "0" => Ok(Command::OrientReset),
        other => Err(format!("orient only supports reset: '{other}'")),
    }
}

fn arg<'a>(rest: &[&'a str], index: usize, usage: &str) -> Result<&'a str, String> {
    rest.get(index)
        .copied()
        .ok_or_else(|| format!("missing argument: {usage}"))
}

fn parse_index(value: &str) -> Result<usize, String> {
    value
        .parse::<usize>()
        .map_err(|_| format!("expected an integer: '{value}'"))
}

fn parse_float(value: &str) -> Result<f32, String> {
    value
        .parse::<f32>()
        .map_err(|_| format!("expected a number: '{value}'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_blank_line_as_none() {
        assert_eq!(parse_command("   "), Ok(None));
    }

    #[test]
    fn parses_aliases() {
        assert_eq!(parse_command("n"), Ok(Some(Command::NextPage)));
        assert_eq!(parse_command("q"), Ok(Some(Command::Quit)));
        assert_eq!(parse_command("rl"), Ok(Some(Command::RotateLeft)));
    }

    #[test]
    fn parses_window_variants() {
        assert_eq!(
            parse_command("window 100 200"),
            Ok(Some(Command::SetWindow {
                center: 100.0,
                width: 200.0
            }))
        );
        assert_eq!(
            parse_command("window center 50"),
            Ok(Some(Command::SetWindowCenter(50.0)))
        );
    }

    #[test]
    fn parses_zoom_and_pan() {
        assert_eq!(parse_command("zoom in"), Ok(Some(Command::ZoomIn)));
        assert_eq!(parse_command("zoom 2.5"), Ok(Some(Command::ZoomSet(2.5))));
        assert_eq!(parse_command("pan 10 -5"), Ok(Some(Command::Pan(10.0, -5.0))));
        assert_eq!(parse_command("pan reset"), Ok(Some(Command::PanReset)));
    }

    #[test]
    fn reports_unknown_command() {
        assert!(parse_command("frobnicate").is_err());
    }

    #[test]
    fn reports_missing_argument() {
        assert!(parse_command("page").is_err());
    }
}
