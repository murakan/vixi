// Copyright (c) 2026 Kan Murata
// This software is released under the MIT License, see LICENSE.

use std::path::PathBuf;

use anyhow::{anyhow, bail, Result};

use crate::app::AppCommand;

pub fn parse_command(input: &str) -> Result<Option<AppCommand>> {
    let input = input.trim();
    if input.is_empty() {
        return Ok(None);
    }

    Ok(Some(match input {
        "+" => AppCommand::ZoomBy(1.25),
        "-" => AppCommand::ZoomBy(0.8),
        "h" => AppCommand::Pan { dx: -40.0, dy: 0.0 },
        "j" => AppCommand::Pan { dx: 0.0, dy: 40.0 },
        "k" => AppCommand::Pan { dx: 0.0, dy: -40.0 },
        "l" => AppCommand::Pan { dx: 40.0, dy: 0.0 },
        "[" => AppCommand::PagePrev,
        "]" => AppCommand::PageNext,
        "r" => AppCommand::RotateRight,
        "R" => AppCommand::RotateLeft,
        "f" => AppCommand::Fit,
        "i" => AppCommand::Invert,
        "q" | "quit" | "exit" => AppCommand::Quit,
        "?" | "help" => AppCommand::Help,
        _ => parse_words(input)?,
    }))
}

fn parse_words(input: &str) -> Result<AppCommand> {
    let words = shell_words(input);
    let Some(command) = words.first().map(String::as_str) else {
        bail!("empty command");
    };

    match command {
        "open" => Ok(AppCommand::Open(PathBuf::from(required(
            &words, 1, "path",
        )?))),
        "zoom" | "z" => Ok(AppCommand::ZoomTo(parse_f32(&words, 1, "zoom")?)),
        "pan" => Ok(AppCommand::Pan {
            dx: parse_f32(&words, 1, "dx")?,
            dy: parse_f32(&words, 2, "dy")?,
        }),
        "page" | "p" => parse_page(&words),
        "plugin" => parse_plugin(&words),
        "move-window" | "move_window" | "wm" => Ok(AppCommand::MoveWindow {
            dx: parse_i32(&words, 1, "dx")?,
            dy: parse_i32(&words, 2, "dy")?,
        }),
        "position-window" | "position_window" | "wp" => Ok(AppCommand::PositionWindow {
            x: parse_i32(&words, 1, "x")?,
            y: parse_i32(&words, 2, "y")?,
        }),
        "rotate" | "rot" => parse_rotate(&words),
        "run" => parse_run(&words),
        "fit" => Ok(AppCommand::Fit),
        "invert" => Ok(AppCommand::Invert),
        "reset" => Ok(AppCommand::Reset),
        "brightness" | "bright" | "b" => Ok(AppCommand::Brightness(parse_f32(&words, 1, "delta")?)),
        "contrast" | "c" => Ok(AppCommand::Contrast(parse_f32(&words, 1, "factor")?)),
        "window" | "win" | "w" => Ok(AppCommand::SetWindow {
            center: parse_f32(&words, 1, "center")?,
            width: parse_f32(&words, 2, "width")?,
        }),
        "pixel" | "px" => Ok(AppCommand::Pixel {
            x: parse_u32(&words, 1, "x")?,
            y: parse_u32(&words, 2, "y")?,
        }),
        "mean" | "stats" => Ok(AppCommand::Mean {
            x: parse_u32(&words, 1, "x")?,
            y: parse_u32(&words, 2, "y")?,
            width: parse_u32(&words, 3, "width")?,
            height: parse_u32(&words, 4, "height")?,
        }),
        _ => Err(anyhow!("unknown command: {command}")),
    }
}

fn parse_plugin(words: &[String]) -> Result<AppCommand> {
    match required(words, 1, "plugin subcommand")? {
        "install" | "i" => Ok(AppCommand::PluginInstall(PathBuf::from(required(
            words, 2, "path",
        )?))),
        "list" | "ls" => Ok(AppCommand::PluginList),
        "remove" | "rm" | "uninstall" => Ok(AppCommand::PluginRemove(
            required(words, 2, "name")?.to_owned(),
        )),
        value => Err(anyhow!("unknown plugin subcommand: {value}")),
    }
}

fn parse_run(words: &[String]) -> Result<AppCommand> {
    let name = required(words, 1, "plugin")?.to_owned();
    let args = words.iter().skip(2).cloned().collect();
    Ok(AppCommand::RunPlugin { name, args })
}

fn parse_page(words: &[String]) -> Result<AppCommand> {
    match required(words, 1, "page")? {
        "next" | "+" => Ok(AppCommand::PageNext),
        "prev" | "previous" | "-" => Ok(AppCommand::PagePrev),
        value => Ok(AppCommand::PageSet(
            value
                .parse::<usize>()
                .map_err(|_| anyhow!("invalid page: {value}"))?,
        )),
    }
}

fn parse_rotate(words: &[String]) -> Result<AppCommand> {
    match required(words, 1, "direction")? {
        "left" | "l" | "-90" => Ok(AppCommand::RotateLeft),
        "right" | "r" | "90" => Ok(AppCommand::RotateRight),
        value => Err(anyhow!("invalid rotate direction: {value}")),
    }
}

fn required<'a>(words: &'a [String], index: usize, name: &str) -> Result<&'a str> {
    words
        .get(index)
        .map(String::as_str)
        .ok_or_else(|| anyhow!("missing {name}"))
}

fn parse_f32(words: &[String], index: usize, name: &str) -> Result<f32> {
    let value = required(words, index, name)?;
    value
        .parse::<f32>()
        .map_err(|_| anyhow!("invalid {name}: {value}"))
}

fn parse_u32(words: &[String], index: usize, name: &str) -> Result<u32> {
    let value = required(words, index, name)?;
    value
        .parse::<u32>()
        .map_err(|_| anyhow!("invalid {name}: {value}"))
}

fn parse_i32(words: &[String], index: usize, name: &str) -> Result<i32> {
    let value = required(words, index, name)?;
    value
        .parse::<i32>()
        .map_err(|_| anyhow!("invalid {name}: {value}"))
}

fn shell_words(input: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote = None;

    for ch in input.chars() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (None, '"' | '\'') => quote = Some(ch),
            (None, c) if c.is_whitespace() => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        words.push(current);
    }

    words
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_one_key_commands() {
        assert!(matches!(
            parse_command("+").unwrap(),
            Some(AppCommand::ZoomBy(_))
        ));
        assert!(matches!(
            parse_command("]").unwrap(),
            Some(AppCommand::PageNext)
        ));
    }

    #[test]
    fn parses_structured_commands() {
        assert!(matches!(
            parse_command("window 10 20").unwrap(),
            Some(AppCommand::SetWindow { .. })
        ));
        assert!(matches!(
            parse_command("mean 1 2 3 4").unwrap(),
            Some(AppCommand::Mean { .. })
        ));
        assert!(matches!(
            parse_command("plugin install denoise.wasm").unwrap(),
            Some(AppCommand::PluginInstall(_))
        ));
        assert!(matches!(
            parse_command("run denoise sigma=1.5").unwrap(),
            Some(AppCommand::RunPlugin { .. })
        ));
    }
}
