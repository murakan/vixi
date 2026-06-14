// Copyright (c) 2026 Kan Murata
// This software is released under the MIT License, see LICENSE.

use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;

use anyhow::{Context, Result};
use futures_executor::block_on;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, Event, WindowEvent};
use winit::event_loop::{EventLoopBuilder, EventLoopProxy};
use winit::keyboard::{KeyCode as WinitKeyCode, PhysicalKey};
use winit::window::{Window, WindowBuilder};

use crate::app::{AppCommand, AppSnapshot, AppState, CommandResult};
use crate::tui::{run_tui, TuiEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayBackend {
    Auto,
    Wayland,
    X11,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum UserEvent {
    CommandReady,
}

pub fn run_viewer(mut app: AppState, backend: DisplayBackend) -> Result<()> {
    let mut event_loop_builder = EventLoopBuilder::<UserEvent>::with_user_event();
    configure_display_backend(&mut event_loop_builder, backend);
    let event_loop = event_loop_builder.build()?;
    let window = Arc::new(
        WindowBuilder::new()
            .with_title(app.title())
            .with_inner_size(LogicalSize::new(960.0, 720.0))
            .build(&event_loop)?,
    );
    let (command_tx, command_rx) = mpsc::channel();
    let (tui_tx, tui_rx) = mpsc::channel();
    spawn_tui(
        command_tx,
        event_loop.create_proxy(),
        tui_rx,
        app.snapshot(),
    );

    let mut renderer =
        block_on(GpuRenderer::new(window.clone())).context("failed to initialize GPU renderer")?;
    window.request_redraw();

    event_loop.run(move |event, elwt| match event {
        Event::UserEvent(UserEvent::CommandReady) | Event::AboutToWait => {
            if process_commands(&window, &mut app, &command_rx, &tui_tx, elwt) {
                window.set_title(&app.title());
                window.request_redraw();
            }
        }
        Event::WindowEvent { event, window_id } if window_id == window.id() => match event {
            WindowEvent::CloseRequested => elwt.exit(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if let Some(command) = key_command(event.physical_key) {
                    match app.apply_command(command) {
                        Ok(CommandResult::Quit) => elwt.exit(),
                        Ok(CommandResult::Message(message)) => {
                            send_tui_message(&tui_tx, message);
                        }
                        Ok(CommandResult::Updated(_)) => {
                            send_tui_snapshot(&tui_tx, &app);
                            window.request_redraw();
                        }
                        Err(err) => send_tui_message(&tui_tx, format!("error: {err}")),
                    }
                }
            }
            WindowEvent::Resized(size) => {
                renderer.resize(size.width, size.height);
                window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                let size = window.inner_size();
                if size.width == 0 || size.height == 0 {
                    return;
                }

                if app.is_frame_dirty() || renderer.needs_frame(size.width, size.height) {
                    let frame = app.render_window(size.width, size.height);
                    renderer.update_frame(size.width, size.height, &frame);
                    app.mark_frame_clean();
                }

                window.pre_present_notify();
                if let Err(err) = renderer.draw() {
                    eprintln!("render error: {err}");
                }
            }
            _ => {}
        },
        _ => {}
    })?;

    Ok(())
}

#[cfg(target_os = "linux")]
fn configure_display_backend(builder: &mut EventLoopBuilder<UserEvent>, backend: DisplayBackend) {
    use std::env;
    use winit::platform::wayland::EventLoopBuilderExtWayland;
    use winit::platform::x11::EventLoopBuilderExtX11;

    match backend {
        DisplayBackend::X11 => {
            builder.with_x11();
            eprintln!("vixi: using X11 display backend");
        }
        DisplayBackend::Wayland => {
            builder.with_wayland();
            eprintln!("vixi: using Wayland display backend");
        }
        DisplayBackend::Auto => {
            if env::var_os("DISPLAY").is_some() {
                builder.with_x11();
                eprintln!("vixi: using X11 display backend (auto)");
            } else if env::var_os("WAYLAND_DISPLAY").is_some() {
                builder.with_wayland();
                eprintln!("vixi: using Wayland display backend (auto)");
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn configure_display_backend(_builder: &mut EventLoopBuilder<UserEvent>, _backend: DisplayBackend) {
}

fn spawn_tui(
    sender: mpsc::Sender<AppCommand>,
    proxy: EventLoopProxy<UserEvent>,
    receiver: Receiver<TuiEvent>,
    snapshot: AppSnapshot,
) {
    thread::spawn(move || {
        if let Err(err) = run_tui(sender, proxy, receiver, snapshot) {
            eprintln!("TUI error: {err}");
        }
    });
}

fn process_commands(
    window: &Window,
    app: &mut AppState,
    receiver: &Receiver<AppCommand>,
    tui_sender: &mpsc::Sender<TuiEvent>,
    elwt: &winit::event_loop::EventLoopWindowTarget<UserEvent>,
) -> bool {
    let mut redraw = false;
    while let Ok(command) = receiver.try_recv() {
        if handle_window_command(window, &command, tui_sender) {
            continue;
        }
        match app.apply_command(command) {
            Ok(CommandResult::Quit) => elwt.exit(),
            Ok(CommandResult::Message(message)) => send_tui_message(tui_sender, message),
            Ok(CommandResult::Updated(message)) => {
                send_tui_message(tui_sender, message);
                send_tui_snapshot(tui_sender, app);
                redraw = true;
            }
            Err(err) => send_tui_message(tui_sender, format!("error: {err}")),
        }
    }
    redraw
}

fn handle_window_command(
    window: &Window,
    command: &AppCommand,
    tui_sender: &mpsc::Sender<TuiEvent>,
) -> bool {
    match command {
        AppCommand::MoveWindow { dx, dy } => {
            match window.outer_position() {
                Ok(position) => {
                    let next = PhysicalPosition::new(position.x + dx, position.y + dy);
                    window.set_outer_position(next);
                    send_tui_message(tui_sender, format!("move-window {dx} {dy}"));
                }
                Err(err) => send_tui_message(
                    tui_sender,
                    format!("window position is not supported by this backend: {err}"),
                ),
            }
            true
        }
        AppCommand::PositionWindow { x, y } => {
            window.set_outer_position(PhysicalPosition::new(*x, *y));
            send_tui_message(tui_sender, format!("position-window {x} {y}"));
            true
        }
        _ => false,
    }
}

fn send_tui_snapshot(sender: &mpsc::Sender<TuiEvent>, app: &AppState) {
    let _ = sender.send(TuiEvent::Snapshot(app.snapshot()));
}

fn send_tui_message(sender: &mpsc::Sender<TuiEvent>, message: String) {
    let _ = sender.send(TuiEvent::Message(message));
}

fn key_command(key: PhysicalKey) -> Option<AppCommand> {
    match key {
        PhysicalKey::Code(WinitKeyCode::Equal) | PhysicalKey::Code(WinitKeyCode::NumpadAdd) => {
            Some(AppCommand::ZoomBy(1.25))
        }
        PhysicalKey::Code(WinitKeyCode::Minus)
        | PhysicalKey::Code(WinitKeyCode::NumpadSubtract) => Some(AppCommand::ZoomBy(0.8)),
        PhysicalKey::Code(WinitKeyCode::KeyH) => Some(AppCommand::Pan { dx: -40.0, dy: 0.0 }),
        PhysicalKey::Code(WinitKeyCode::KeyJ) => Some(AppCommand::Pan { dx: 0.0, dy: 40.0 }),
        PhysicalKey::Code(WinitKeyCode::KeyK) => Some(AppCommand::Pan { dx: 0.0, dy: -40.0 }),
        PhysicalKey::Code(WinitKeyCode::KeyL) => Some(AppCommand::Pan { dx: 40.0, dy: 0.0 }),
        PhysicalKey::Code(WinitKeyCode::BracketLeft) => Some(AppCommand::PagePrev),
        PhysicalKey::Code(WinitKeyCode::BracketRight) => Some(AppCommand::PageNext),
        PhysicalKey::Code(WinitKeyCode::KeyF) => Some(AppCommand::Fit),
        PhysicalKey::Code(WinitKeyCode::KeyI) => Some(AppCommand::Invert),
        PhysicalKey::Code(WinitKeyCode::KeyR) => Some(AppCommand::RotateRight),
        PhysicalKey::Code(WinitKeyCode::KeyQ) | PhysicalKey::Code(WinitKeyCode::Escape) => {
            Some(AppCommand::Quit)
        }
        _ => None,
    }
}

struct GpuRenderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    frame_width: u32,
    frame_height: u32,
}

impl GpuRenderer {
    async fn new(window: Arc<Window>) -> Result<Self> {
        let size = window.inner_size();
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window)?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .context("no suitable GPU adapter found")?;
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("vixi device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                        .using_resolution(adapter.limits()),
                },
                None,
            )
            .await?;
        let config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .context("surface is not supported by the selected adapter")?;
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("vixi shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("vixi bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("vixi pipeline layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("vixi pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("vixi sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let texture = create_texture(&device, size.width.max(1), size.height.max(1));
        let bind_group = create_bind_group(&device, &bind_group_layout, &texture, &sampler);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            bind_group_layout,
            sampler,
            texture,
            bind_group,
            frame_width: size.width.max(1),
            frame_height: size.height.max(1),
        })
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    fn needs_frame(&self, width: u32, height: u32) -> bool {
        self.frame_width != width || self.frame_height != height
    }

    fn update_frame(&mut self, width: u32, height: u32, frame: &[u8]) {
        if self.needs_frame(width, height) {
            self.texture = create_texture(&self.device, width, height);
            self.bind_group = create_bind_group(
                &self.device,
                &self.bind_group_layout,
                &self.texture,
                &self.sampler,
            );
            self.frame_width = width;
            self.frame_height = height;
        }

        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            frame,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }

    fn draw(&mut self) -> Result<()> {
        let output = match self.surface.get_current_texture() {
            Ok(output) => output,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            Err(wgpu::SurfaceError::Timeout) => return Ok(()),
            Err(err) => return Err(err.into()),
        };
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("vixi encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("vixi render pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit(Some(encoder.finish()));
        output.present();
        Ok(())
    }
}

fn create_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("vixi frame texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn create_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    texture: &wgpu::Texture,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("vixi bind group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

const SHADER: &str = r#"
struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOut {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    var uvs = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(2.0, 1.0),
        vec2<f32>(0.0, -1.0),
    );

    var out: VertexOut;
    out.position = vec4<f32>(positions[vertex_index], 0.0, 1.0);
    out.uv = uvs[vertex_index];
    return out;
}

@group(0) @binding(0) var image_texture: texture_2d<f32>;
@group(0) @binding(1) var image_sampler: sampler;

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    return textureSample(image_texture, image_sampler, in.uv);
}
"#;
