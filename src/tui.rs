// Copyright (c) 2026 Kan Murata
// This software is released under the MIT License, see LICENSE.

use std::io::{self, Stdout, Write};
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use anyhow::Result;
use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::style::{Attribute, Print, SetAttribute};
use crossterm::terminal::{
    self, disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen,
    LeaveAlternateScreen,
};
use crossterm::{execute, queue};
use winit::event_loop::EventLoopProxy;

use crate::app::{AppCommand, AppSnapshot};
use crate::commands::parse_command;
use crate::plugins::installed_plugin_names;
use crate::viewer::UserEvent;

#[derive(Debug, Clone)]
pub enum TuiEvent {
    Message(String),
    Snapshot(AppSnapshot),
}

struct TuiState {
    snapshot: AppSnapshot,
    messages: Vec<String>,
    commands: Vec<CommandSpec>,
    selected: usize,
    input: String,
}

#[derive(Debug, Clone)]
struct CommandSpec {
    label: String,
    command: CommandTemplate,
    detail: String,
}

#[derive(Debug, Clone)]
enum CommandTemplate {
    Immediate(AppCommand),
    Prefix(&'static str),
}

pub fn run_tui(
    command_sender: Sender<AppCommand>,
    proxy: EventLoopProxy<UserEvent>,
    receiver: Receiver<TuiEvent>,
    snapshot: AppSnapshot,
) -> Result<()> {
    let mut terminal = TerminalGuard::enter()?;
    let mut state = TuiState {
        snapshot,
        messages: vec![
            "Type to filter commands. Enter selects a command or runs typed input.".to_owned(),
            "Commands needing parameters place a template in the input line.".to_owned(),
        ],
        commands: commands(),
        selected: 0,
        input: String::new(),
    };

    let mut dirty = true;
    let mut last_size = terminal::size()?;
    draw(&mut terminal.stdout, &state)?;
    loop {
        while let Ok(event) = receiver.try_recv() {
            match event {
                TuiEvent::Message(message) => push_message(&mut state, message),
                TuiEvent::Snapshot(snapshot) => state.snapshot = snapshot,
            }
            dirty = true;
        }

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) => {
                    if handle_key(key, &mut state, &command_sender, &proxy)? {
                        break;
                    }
                    dirty = true;
                }
                Event::Resize(_, _) => dirty = true,
                _ => {}
            }
        }
        let size = terminal::size()?;
        if size != last_size {
            last_size = size;
            dirty = true;
            queue!(terminal.stdout, Clear(ClearType::All))?;
        }
        if dirty {
            draw(&mut terminal.stdout, &state)?;
            dirty = false;
        }
    }

    Ok(())
}

fn handle_key(
    key: KeyEvent,
    state: &mut TuiState,
    command_sender: &Sender<AppCommand>,
    proxy: &EventLoopProxy<UserEvent>,
) -> Result<bool> {
    match key.code {
        KeyCode::Char('q') if state.input.is_empty() => {
            send_command(command_sender, proxy, AppCommand::Quit);
            return Ok(true);
        }
        KeyCode::Char('?') => {
            push_message(
                state,
                "Keys: type to filter commands, Up/Down select, Enter choose/run, Esc clear, Ctrl-U clear, q quit when input is empty.".to_owned(),
            );
        }
        KeyCode::Up => {
            state.selected = state.selected.saturating_sub(1);
        }
        KeyCode::Down => {
            let max = filtered_commands(state).len().saturating_sub(1);
            state.selected = (state.selected + 1).min(max);
        }
        KeyCode::Enter => {
            submit_input_or_selection(state, command_sender, proxy);
        }
        KeyCode::Esc => {
            state.input.clear();
            state.selected = 0;
        }
        KeyCode::Backspace => {
            state.input.pop();
            clamp_selection(state);
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.input.clear();
            state.selected = 0;
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.input.clear();
            state.selected = 0;
        }
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.input.push(ch);
            state.selected = 0;
        }
        _ => {}
    }
    Ok(false)
}

fn submit_input_or_selection(
    state: &mut TuiState,
    command_sender: &Sender<AppCommand>,
    proxy: &EventLoopProxy<UserEvent>,
) {
    let input = state.input.trim().to_owned();
    if input.contains(' ') || input.starts_with('+') || input.starts_with('-') {
        submit_command_text(state, command_sender, proxy, &input);
        return;
    }

    let filtered = filtered_commands(state);
    let Some(command) = filtered.get(state.selected).or_else(|| filtered.first()) else {
        submit_command_text(state, command_sender, proxy, &input);
        return;
    };

    match &command.command {
        CommandTemplate::Immediate(command) => {
            send_command(command_sender, proxy, command.clone());
            state.input.clear();
            state.selected = 0;
        }
        CommandTemplate::Prefix(prefix) => {
            state.input = format!("{prefix} ");
            state.selected = 0;
        }
    }
}

fn submit_command_text(
    state: &mut TuiState,
    command_sender: &Sender<AppCommand>,
    proxy: &EventLoopProxy<UserEvent>,
    input: &str,
) {
    match parse_command(input) {
        Ok(Some(command)) => {
            if matches!(command, AppCommand::Help) {
                push_message(state, crate::app::help_text());
            } else {
                send_command(command_sender, proxy, command);
            }
        }
        Ok(None) => {}
        Err(err) => push_message(state, format!("error: {err}")),
    }
    state.input.clear();
    state.selected = 0;
}

fn send_command(
    command_sender: &Sender<AppCommand>,
    proxy: &EventLoopProxy<UserEvent>,
    command: AppCommand,
) {
    if command_sender.send(command).is_ok() {
        let _ = proxy.send_event(UserEvent::CommandReady);
    }
}

fn draw(stdout: &mut Stdout, state: &TuiState) -> Result<()> {
    let (width, height) = terminal::size()?;
    queue!(stdout, Hide, MoveTo(0, 0))?;
    draw_box(stdout, 0, 0, width, 3, "vixi")?;
    print_clipped(stdout, 0, 1, width, &state.snapshot.title)?;
    print_clipped(stdout, 0, 2, width, &state.snapshot.status)?;

    let body_y = 4;
    let input_h = 3;
    let history_h = 7.min(height.saturating_sub(10));
    let body_h = height.saturating_sub(5 + input_h + history_h);
    let left_w = width.saturating_mul(44) / 100;
    let right_w = width.saturating_sub(left_w + 1);
    let stats_h = 12.min(body_h.saturating_sub(4));
    let hist_h = body_h.saturating_sub(stats_h + 1);

    draw_commands(stdout, 0, body_y, left_w, body_h, state)?;
    draw_statistics(stdout, left_w + 1, body_y, right_w, stats_h, state)?;
    draw_histogram(
        stdout,
        left_w + 1,
        body_y + stats_h + 1,
        right_w,
        hist_h,
        state,
    )?;
    draw_messages(stdout, 0, body_y + body_h + 1, width, history_h, state)?;

    let palette_y = height.saturating_sub(input_h);
    draw_box(stdout, 0, palette_y, width, input_h, "input")?;
    let palette = if state.input.is_empty() {
        "vixi> ".to_owned()
    } else {
        format!("vixi> {cursor}", cursor = state.input)
    };
    print_clipped(stdout, 0, palette_y + 1, width, &palette)?;
    stdout.flush()?;
    Ok(())
}

fn draw_commands(
    stdout: &mut Stdout,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    state: &TuiState,
) -> Result<()> {
    draw_box(stdout, x, y, width, height, "commands")?;
    let filtered = filtered_commands(state);
    for (index, command) in filtered
        .iter()
        .enumerate()
        .take(height.saturating_sub(2) as usize)
    {
        let yy = y + 1 + index as u16;
        queue!(stdout, MoveTo(x + 1, yy))?;
        if index == state.selected {
            queue!(stdout, SetAttribute(Attribute::Reverse))?;
        }
        let label = format!("{}  {}", command.label, command.detail);
        print_clipped(stdout, x + 1, yy, width.saturating_sub(2), &label)?;
        if index == state.selected {
            queue!(stdout, SetAttribute(Attribute::Reset))?;
        }
    }
    Ok(())
}

fn draw_messages(
    stdout: &mut Stdout,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    state: &TuiState,
) -> Result<()> {
    draw_box(stdout, x, y, width, height, "results")?;
    let visible = height.saturating_sub(2) as usize;
    let start = state.messages.len().saturating_sub(visible);
    for (offset, message) in state.messages.iter().skip(start).enumerate() {
        print_clipped(
            stdout,
            x + 1,
            y + 1 + offset as u16,
            width.saturating_sub(2),
            message,
        )?;
    }
    Ok(())
}

fn draw_statistics(
    stdout: &mut Stdout,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    state: &TuiState,
) -> Result<()> {
    draw_box(stdout, x, y, width, height, "statistics")?;
    for (offset, line) in state
        .snapshot
        .statistics
        .lines()
        .take(height.saturating_sub(2) as usize)
        .enumerate()
    {
        print_clipped(
            stdout,
            x + 1,
            y + 1 + offset as u16,
            width.saturating_sub(2),
            line,
        )?;
    }
    Ok(())
}

fn draw_histogram(
    stdout: &mut Stdout,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    state: &TuiState,
) -> Result<()> {
    draw_box(stdout, x, y, width, height, "histogram")?;
    for (offset, line) in state
        .snapshot
        .histogram
        .lines()
        .take(height.saturating_sub(2) as usize)
        .enumerate()
    {
        print_clipped(
            stdout,
            x + 1,
            y + 1 + offset as u16,
            width.saturating_sub(2),
            line,
        )?;
    }
    Ok(())
}

fn draw_box(
    stdout: &mut Stdout,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    title: &str,
) -> Result<()> {
    if width == 0 || height == 0 {
        return Ok(());
    }
    for yy in y..y.saturating_add(height) {
        clear_line(stdout, yy, x, width)?;
    }
    queue!(
        stdout,
        MoveTo(x, y),
        SetAttribute(Attribute::Bold),
        Print(title.to_ascii_uppercase()),
        SetAttribute(Attribute::Reset)
    )?;
    if width > title.len() as u16 + 2 {
        let start = x + title.len() as u16 + 1;
        for xx in start..x + width {
            queue!(stdout, MoveTo(xx, y), Print("-"))?;
        }
    }
    Ok(())
}

fn print_clipped(stdout: &mut Stdout, x: u16, y: u16, width: u16, text: &str) -> Result<()> {
    let clipped = text.chars().take(width as usize).collect::<String>();
    queue!(stdout, MoveTo(x, y), Print(clipped))?;
    Ok(())
}

fn clear_line(stdout: &mut Stdout, y: u16, x: u16, width: u16) -> Result<()> {
    queue!(stdout, MoveTo(x, y), Print(" ".repeat(width as usize)))?;
    Ok(())
}

fn push_message(state: &mut TuiState, message: String) {
    state.messages.extend(message.lines().map(str::to_owned));
    let keep = 200;
    if state.messages.len() > keep {
        state.messages.drain(0..state.messages.len() - keep);
    }
}

fn filtered_commands(state: &TuiState) -> Vec<&CommandSpec> {
    let query = command_query(&state.input);
    let mut matches = state
        .commands
        .iter()
        .filter(|command| fuzzy_match(&command.label, query) || fuzzy_match(&command.detail, query))
        .collect::<Vec<_>>();
    if matches.is_empty() {
        matches = state.commands.iter().collect();
    }
    matches
}

fn command_query(input: &str) -> &str {
    if input.contains(' ') {
        input.split_whitespace().next().unwrap_or_default()
    } else {
        input.trim()
    }
}

fn fuzzy_match(text: &str, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let mut chars = query.chars().map(|c| c.to_ascii_lowercase());
    let Some(mut needle) = chars.next() else {
        return true;
    };
    for ch in text.chars().map(|c| c.to_ascii_lowercase()) {
        if ch == needle {
            match chars.next() {
                Some(next) => needle = next,
                None => return true,
            }
        }
    }
    false
}

fn clamp_selection(state: &mut TuiState) {
    let max = filtered_commands(state).len().saturating_sub(1);
    state.selected = state.selected.min(max);
}

fn commands() -> Vec<CommandSpec> {
    let mut commands = vec![
        immediate("fit", "fit image to window", AppCommand::Fit),
        immediate("zoom in", "increase zoom", AppCommand::ZoomBy(1.25)),
        immediate("zoom out", "decrease zoom", AppCommand::ZoomBy(0.8)),
        prefix("zoom", "<value>"),
        prefix("pan", "<dx> <dy>"),
        immediate("page next", "next page", AppCommand::PageNext),
        immediate("page prev", "previous page", AppCommand::PagePrev),
        prefix("page", "<number>"),
        immediate("invert", "toggle intensity inversion", AppCommand::Invert),
        immediate("rotate right", "rotate clockwise", AppCommand::RotateRight),
        immediate(
            "rotate left",
            "rotate counterclockwise",
            AppCommand::RotateLeft,
        ),
        immediate("reset", "reset view and window", AppCommand::Reset),
        prefix("brightness", "<delta>"),
        prefix("contrast", "<factor>"),
        prefix("window", "<center> <width>"),
        prefix("pixel", "<x> <y>"),
        prefix("mean", "<x> <y> <width> <height>"),
        prefix("move-window", "<dx> <dy>"),
        prefix("position-window", "<x> <y>"),
        prefix("open", "<path>"),
        immediate(
            "plugin list",
            "list installed plugins",
            AppCommand::PluginList,
        ),
        prefix("plugin install", "<path>"),
        prefix("plugin remove", "<name>"),
        immediate("help", "show command help", AppCommand::Help),
        immediate("quit", "quit vixi", AppCommand::Quit),
    ];
    commands.extend(
        installed_plugin_names()
            .into_iter()
            .map(|name| CommandSpec {
                label: format!("run {name}"),
                detail: "installed plugin".to_owned(),
                command: CommandTemplate::Immediate(AppCommand::RunPlugin {
                    name,
                    args: Vec::new(),
                }),
            }),
    );
    commands
}

fn immediate(label: &str, detail: &str, command: AppCommand) -> CommandSpec {
    CommandSpec {
        label: label.to_owned(),
        detail: detail.to_owned(),
        command: CommandTemplate::Immediate(command),
    }
}

fn prefix(label: &'static str, detail: &str) -> CommandSpec {
    CommandSpec {
        label: label.to_owned(),
        detail: detail.to_owned(),
        command: CommandTemplate::Prefix(label),
    }
}

struct TerminalGuard {
    stdout: Stdout,
}

impl TerminalGuard {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, Hide)?;
        Ok(Self { stdout })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(self.stdout, Show, LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}
