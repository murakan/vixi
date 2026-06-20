// Copyright (c) 2026 Kan Murata
// This software is released under the MIT License, see LICENSE.

//! Streaming video playback backed by the FFmpeg libav* libraries.
//!
//! A dedicated decoder thread reads packets, decodes and scales frames to
//! RGBA8, paces them according to their presentation timestamps, and streams
//! them to the display window. Playback is controlled with [`VideoCommand`]s.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Once;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use ffmpeg_next as ffmpeg;
use ffmpeg::format::{input, Pixel};
use ffmpeg::media::Type;
use ffmpeg::software::scaling::{context::Context as Scaler, flag::Flags};
use ffmpeg::util::frame::video::Video as VideoFrameBuffer;

use crate::pixel::ImageData;

/// FFmpeg's internal time base: seek timestamps are expressed in these units.
const AV_TIME_BASE: f64 = 1_000_000.0;

const SPEED_MIN: f64 = 0.1;
const SPEED_MAX: f64 = 16.0;

static FFMPEG_INIT: Once = Once::new();

/// Static metadata about an opened video.
#[derive(Debug, Clone)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub duration_secs: f64,
    pub fps: f64,
}

/// A playback control command sent to the decoder thread.
#[derive(Debug, Clone, Copy)]
pub enum VideoCommand {
    Play,
    Pause,
    TogglePlay,
    /// Seek to an absolute position, in seconds.
    Seek(f64),
    /// Seek relative to the current position, in seconds.
    SeekRelative(f64),
    /// Set the playback speed multiplier.
    SetSpeed(f64),
    /// Advance a single frame while paused.
    Step,
    /// Shut the decoder thread down.
    Stop,
}

/// A decoded frame delivered to the display window.
pub struct VideoFrame {
    pub data: ImageData,
    pub pts_secs: f64,
}

/// Handle to a running video decoder thread.
pub struct VideoHandle {
    pub info: VideoInfo,
    commands: Sender<VideoCommand>,
    frames: Receiver<VideoFrame>,
}

impl VideoHandle {
    /// Open a video and start its decoder thread.
    ///
    /// `ctx` is used to wake the display window whenever a new frame is ready.
    pub fn open(path: &Path, ctx: eframe::egui::Context) -> Result<Self> {
        FFMPEG_INIT.call_once(|| {
            let _ = ffmpeg::init();
        });

        let path = PathBuf::from(path);
        let (command_tx, command_rx) = mpsc::channel();
        let (frame_tx, frame_rx) = mpsc::channel();
        let (init_tx, init_rx) = mpsc::channel();

        thread::spawn(move || match Player::open(&path) {
            Ok(player) => {
                let _ = init_tx.send(Ok(player.info.clone()));
                player.run(command_rx, frame_tx, ctx);
            }
            Err(error) => {
                let _ = init_tx.send(Err(error));
            }
        });

        let info = init_rx
            .recv()
            .map_err(|_| anyhow!("video decoder thread exited before initializing"))??;

        Ok(Self {
            info,
            commands: command_tx,
            frames: frame_rx,
        })
    }

    /// Take the most recent decoded frame, discarding any older queued frames.
    pub fn latest_frame(&self) -> Option<VideoFrame> {
        let mut latest = None;
        while let Ok(frame) = self.frames.try_recv() {
            latest = Some(frame);
        }
        latest
    }

    pub fn send(&self, command: VideoCommand) {
        let _ = self.commands.send(command);
    }
}

impl Drop for VideoHandle {
    fn drop(&mut self) {
        self.send(VideoCommand::Stop);
    }
}

/// Mutable playback state tracked by the decoder loop.
struct RunState {
    playing: bool,
    speed: f64,
    pending_steps: u32,
    seek_to: Option<f64>,
    current: f64,
    stop: bool,
}

impl RunState {
    fn apply(&mut self, command: VideoCommand) {
        match command {
            VideoCommand::Play => self.playing = true,
            VideoCommand::Pause => self.playing = false,
            VideoCommand::TogglePlay => self.playing = !self.playing,
            VideoCommand::Seek(secs) => self.seek_to = Some(secs.max(0.0)),
            VideoCommand::SeekRelative(delta) => {
                self.seek_to = Some((self.current + delta).max(0.0))
            }
            VideoCommand::SetSpeed(speed) => self.speed = speed.clamp(SPEED_MIN, SPEED_MAX),
            VideoCommand::Step => {
                self.playing = false;
                self.pending_steps += 1;
            }
            VideoCommand::Stop => self.stop = true,
        }
    }
}

struct Player {
    ictx: ffmpeg::format::context::Input,
    decoder: ffmpeg::decoder::Video,
    scaler: Option<Scaler>,
    stream_index: usize,
    time_base: f64,
    info: VideoInfo,
    ended: bool,
    last_present_pts: Option<f64>,
}

impl Player {
    fn open(path: &Path) -> Result<Self> {
        let ictx = input(&path).with_context(|| format!("failed to open {}", path.display()))?;
        let stream = ictx
            .streams()
            .best(Type::Video)
            .ok_or_else(|| anyhow!("no video stream found"))?;
        let stream_index = stream.index();
        let time_base = f64::from(stream.time_base());

        let decoder = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
            .context("failed to build decoder context")?
            .decoder()
            .video()
            .context("failed to open video decoder")?;

        let fps = {
            let rate = f64::from(stream.avg_frame_rate());
            if rate.is_finite() && rate > 0.0 {
                rate
            } else {
                0.0
            }
        };
        let duration_secs = if stream.duration() > 0 {
            stream.duration() as f64 * time_base
        } else if ictx.duration() > 0 {
            ictx.duration() as f64 / AV_TIME_BASE
        } else {
            0.0
        };

        let info = VideoInfo {
            width: decoder.width(),
            height: decoder.height(),
            duration_secs,
            fps,
        };

        Ok(Self {
            ictx,
            decoder,
            scaler: None,
            stream_index,
            time_base,
            info,
            ended: false,
            last_present_pts: None,
        })
    }

    fn run(
        mut self,
        command_rx: Receiver<VideoCommand>,
        frame_tx: Sender<VideoFrame>,
        ctx: eframe::egui::Context,
    ) {
        let mut state = RunState {
            playing: true,
            speed: 1.0,
            pending_steps: 0,
            seek_to: None,
            current: 0.0,
            stop: false,
        };

        loop {
            if state.stop {
                break;
            }

            // When idle, block until a command arrives so the thread sleeps.
            let idle = !state.playing && state.pending_steps == 0 && state.seek_to.is_none();
            if idle {
                match command_rx.recv() {
                    Ok(command) => state.apply(command),
                    Err(_) => break,
                }
                continue;
            }

            while let Ok(command) = command_rx.try_recv() {
                state.apply(command);
            }
            if state.stop {
                break;
            }

            if let Some(target) = state.seek_to.take() {
                if let Err(error) = self.seek(target) {
                    tracing::warn!("seek failed: {error}");
                }
            }

            if self.ended {
                // Hold on the final frame until a seek rewinds the stream.
                state.playing = false;
                continue;
            }

            match self.next_frame() {
                Ok(Some(frame)) => {
                    state.current = frame.pts_secs;
                    let pts = frame.pts_secs;
                    if frame_tx.send(frame).is_err() {
                        break;
                    }
                    ctx.request_repaint();

                    if state.pending_steps > 0 {
                        state.pending_steps -= 1;
                        self.last_present_pts = Some(pts);
                    } else if state.playing {
                        let delta = self
                            .last_present_pts
                            .map(|prev| (pts - prev).max(0.0))
                            .unwrap_or(0.0);
                        self.last_present_pts = Some(pts);
                        pace(delta / state.speed, &command_rx, &mut state);
                    }
                }
                Ok(None) => {
                    self.ended = true;
                    state.playing = false;
                    self.last_present_pts = None;
                }
                Err(error) => {
                    tracing::error!("decode error: {error}");
                    break;
                }
            }
        }
    }

    fn seek(&mut self, secs: f64) -> Result<()> {
        let timestamp = (secs * AV_TIME_BASE) as i64;
        self.ictx
            .seek(timestamp, ..timestamp)
            .context("seek failed")?;
        self.decoder.flush();
        self.ended = false;
        self.last_present_pts = None;
        Ok(())
    }

    fn next_frame(&mut self) -> Result<Option<VideoFrame>> {
        let mut decoded = VideoFrameBuffer::empty();
        loop {
            if self.decoder.receive_frame(&mut decoded).is_ok() {
                return Ok(Some(self.scale(&decoded)?));
            }

            let mut packet = ffmpeg::Packet::empty();
            match packet.read(&mut self.ictx) {
                Ok(()) => {
                    if packet.stream() == self.stream_index {
                        self.decoder
                            .send_packet(&packet)
                            .context("failed to send packet to decoder")?;
                    }
                }
                Err(ffmpeg::Error::Eof) => {
                    self.decoder.send_eof().ok();
                    if self.decoder.receive_frame(&mut decoded).is_ok() {
                        return Ok(Some(self.scale(&decoded)?));
                    }
                    return Ok(None);
                }
                Err(error) => return Err(error).context("failed to read packet"),
            }
        }
    }

    fn scale(&mut self, decoded: &VideoFrameBuffer) -> Result<VideoFrame> {
        let scaler = match &mut self.scaler {
            Some(scaler) => scaler,
            None => {
                let scaler = Scaler::get(
                    decoded.format(),
                    decoded.width(),
                    decoded.height(),
                    Pixel::RGBA,
                    decoded.width(),
                    decoded.height(),
                    Flags::BILINEAR,
                )
                .context("failed to create RGBA scaler")?;
                self.scaler.insert(scaler)
            }
        };

        let mut rgba = VideoFrameBuffer::empty();
        scaler.run(decoded, &mut rgba).context("failed to scale frame")?;

        let pts_secs = decoded
            .pts()
            .map(|pts| pts as f64 * self.time_base)
            .or_else(|| self.last_present_pts.map(|prev| prev + frame_step(self.info.fps)))
            .unwrap_or(0.0);

        Ok(VideoFrame {
            data: frame_to_rgba(&rgba),
            pts_secs,
        })
    }
}

fn frame_step(fps: f64) -> f64 {
    if fps > 0.0 {
        1.0 / fps
    } else {
        0.0
    }
}

fn frame_to_rgba(frame: &VideoFrameBuffer) -> ImageData {
    let width = frame.width();
    let height = frame.height();
    let stride = frame.stride(0);
    let source = frame.data(0);
    let row_bytes = width as usize * 4;

    let mut data = Vec::with_capacity(row_bytes * height as usize);
    for row in 0..height as usize {
        let start = row * stride;
        data.extend_from_slice(&source[start..start + row_bytes]);
    }

    ImageData::Rgba8 {
        width,
        height,
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::time::Instant;

    /// Render a short test clip with the ffmpeg CLI. Returns `None` when ffmpeg
    /// is not installed so the test can be skipped instead of failing.
    fn make_test_clip(path: &Path) -> Option<()> {
        let status = Command::new("ffmpeg")
            .args(["-y", "-f", "lavfi", "-i"])
            .arg("testsrc=duration=1:size=320x240:rate=10")
            .args(["-pix_fmt", "yuv420p"])
            .arg(path)
            .status()
            .ok()?;
        status.success().then_some(())
    }

    #[test]
    fn decodes_video_frames() {
        let file = tempfile::Builder::new()
            .suffix(".mp4")
            .tempfile()
            .expect("temp file");
        if make_test_clip(file.path()).is_none() {
            eprintln!("ffmpeg not available; skipping decodes_video_frames");
            return;
        }

        let ctx = eframe::egui::Context::default();
        let handle = VideoHandle::open(file.path(), ctx).expect("open video");
        assert_eq!((handle.info.width, handle.info.height), (320, 240));

        // The decoder starts playing; wait briefly for the first frame.
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut frame = None;
        while Instant::now() < deadline {
            if let Some(received) = handle.latest_frame() {
                frame = Some(received);
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }

        let frame = frame.expect("a decoded frame");
        assert_eq!(frame.data.dimensions(), (320, 240));
    }
}

/// Sleep for `dt` seconds while staying responsive to control commands.
///
/// Returns early if a command changes the playback state (pause, seek, stop).
fn pace(dt: f64, command_rx: &Receiver<VideoCommand>, state: &mut RunState) {
    if dt <= 0.0 {
        return;
    }
    // Cap a single wait so an unusually long inter-frame gap still polls input.
    let deadline = Instant::now() + Duration::from_secs_f64(dt.min(2.0));
    loop {
        let now = Instant::now();
        if now >= deadline {
            return;
        }
        let chunk = (deadline - now).min(Duration::from_millis(15));
        match command_rx.recv_timeout(chunk) {
            Ok(command) => {
                state.apply(command);
                if !state.playing || state.seek_to.is_some() || state.stop {
                    return;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                state.stop = true;
                return;
            }
        }
    }
}
