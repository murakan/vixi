// Copyright (c) 2026 Kan Murata
// This software is released under the MIT License, see LICENSE.

use std::io::{self, BufRead, Write};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use crate::command::{parse_command, Command};

/// A snapshot of the viewer state, rendered by the terminal TUI panel.
#[derive(Debug, Clone)]
pub struct StatusSnapshot {
    pub path: String,
    pub page: usize,
    pub page_count: usize,
    pub width: u32,
    pub height: u32,
    pub sample: String,
    pub zoom: f32,
    pub pan: (f32, f32),
    pub windowable: bool,
    pub window_center: f32,
    pub window_width: f32,
    pub invert: bool,
    pub rotation_degrees: u32,
    pub flip_x: bool,
    pub flip_y: bool,
    pub auto_window: String,
}

/// Channels shared between the terminal control thread and the display window.
pub struct ControlChannels {
    pub commands: Sender<Command>,
    pub status: Receiver<StatusSnapshot>,
    pub context: Receiver<eframe::egui::Context>,
}

/// Run the terminal REPL. Blocks until the user quits or the window closes.
///
/// This is expected to run on a dedicated thread while `eframe` owns the main
/// thread for the display window.
pub fn run_repl(channels: ControlChannels) {
    // Wait for the display window to hand us its egui context so we can wake it
    // up whenever a command is issued.
    let Ok(ctx) = channels.context.recv() else {
        return;
    };

    // Prime the panel with the initial state.
    let mut snapshot = match request(&channels, &ctx, Command::Status) {
        Some(snapshot) => snapshot,
        None => return,
    };
    let mut message = "'help' でコマンド一覧を表示します".to_owned();

    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();

    loop {
        render(&snapshot, &message);
        prompt();

        let Some(line) = lines.next() else {
            // EOF (Ctrl-D): close the viewer cleanly.
            let _ = channels.commands.send(Command::Quit);
            ctx.request_repaint();
            break;
        };
        let line = match line {
            Ok(line) => line,
            Err(_) => break,
        };

        match parse_command(&line) {
            Ok(None) => continue,
            Ok(Some(Command::Help)) => {
                message = help_text();
                continue;
            }
            Ok(Some(Command::Quit)) => {
                let _ = channels.commands.send(Command::Quit);
                ctx.request_repaint();
                break;
            }
            Ok(Some(command)) => match request(&channels, &ctx, command) {
                Some(next) => {
                    snapshot = next;
                    message = "ok".to_owned();
                }
                None => {
                    message = "表示ウィンドウが閉じられました。終了します。".to_owned();
                    render(&snapshot, &message);
                    break;
                }
            },
            Err(error) => message = error,
        }
    }
}

/// Send a command and wait for the resulting status snapshot.
///
/// Returns `None` if the display window has gone away.
fn request(
    channels: &ControlChannels,
    ctx: &eframe::egui::Context,
    command: Command,
) -> Option<StatusSnapshot> {
    if channels.commands.send(command).is_err() {
        return None;
    }
    ctx.request_repaint();

    loop {
        match channels.status.recv_timeout(Duration::from_secs(5)) {
            Ok(snapshot) => return Some(snapshot),
            Err(RecvTimeoutError::Timeout) => {
                // The window may be idle; nudge it again and keep waiting.
                ctx.request_repaint();
            }
            Err(RecvTimeoutError::Disconnected) => return None,
        }
    }
}

fn prompt() {
    print!("vixi> ");
    let _ = io::stdout().flush();
}

fn render(snapshot: &StatusSnapshot, message: &str) {
    let mut out = String::new();
    // Clear screen and move cursor home.
    out.push_str("\x1b[2J\x1b[H");

    let rule = "─".repeat(48);
    out.push_str(&format!(" vixi — {}\n", snapshot.path));
    out.push_str(&format!(" {rule}\n"));
    out.push_str(&format!(
        " Page     {} / {}\n",
        snapshot.page,
        snapshot.page_count.saturating_sub(1)
    ));
    out.push_str(&format!(
        " Size     {} x {}   {}\n",
        snapshot.width, snapshot.height, snapshot.sample
    ));
    out.push_str(&format!(
        " Zoom     {:.0}%        Pan {:.0}, {:.0}\n",
        snapshot.zoom * 100.0,
        snapshot.pan.0,
        snapshot.pan.1
    ));
    if snapshot.windowable {
        out.push_str(&format!(
            " Window   center {:.3}   width {:.3}\n",
            snapshot.window_center, snapshot.window_width
        ));
        out.push_str(&format!(" Auto     {}\n", snapshot.auto_window));
    }
    out.push_str(&format!(
        " Invert   {}\n",
        if snapshot.invert { "on" } else { "off" }
    ));
    out.push_str(&format!(
        " Orient   {}°   flip-x:{} flip-y:{}\n",
        snapshot.rotation_degrees,
        on_off(snapshot.flip_x),
        on_off(snapshot.flip_y)
    ));
    out.push_str(&format!(" {rule}\n"));
    out.push_str(message);
    out.push('\n');

    print!("{out}");
    let _ = io::stdout().flush();
}

fn on_off(value: bool) -> &'static str {
    if value {
        "on"
    } else {
        "off"
    }
}

fn help_text() -> String {
    [
        "コマンド一覧:",
        "  next / n                 次のページ",
        "  prev / p                 前のページ",
        "  page <番号>              指定ページへ (0始まり)",
        "  zoom in|out|reset|<倍率> ズーム",
        "  zoom fit / fit / f       ウィンドウに合わせる",
        "  pan <dx> <dy>            表示位置を移動 (pan reset で原点)",
        "  window <中心> <幅>       ウィンドウ調整",
        "  window center <値> / window width <値>",
        "  reset / r                ウィンドウ自動調整",
        "  autowindow minmax|percentile  自動調整モード",
        "  invert                   白黒反転トグル",
        "  rotate left|right        回転 (rl / rr)",
        "  flip x|y                 反転",
        "  orient reset             回転/反転をリセット",
        "  status / info / s        状態を再表示",
        "  help / h                 このヘルプ",
        "  quit / q                 終了 (Ctrl-D でも終了)",
    ]
    .join("\n")
}
