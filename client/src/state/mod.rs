use std::sync::Arc;

use egui::{Context, FullOutput, TopBottomPanel};
use egui_wgpu::{Renderer, RendererOptions, ScreenDescriptor};
use wgpu::{
    Backends, ExperimentalFeatures, Features, Instance, InstanceDescriptor, MemoryHints,
    SurfaceError, TextureFormat, Trace,
};
use winit::window::Window;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};

use crate::map::MapSystem;

// This will store the state of our game
pub struct State {
    pub window: Arc<Window>,
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    pub is_surface_configured: bool,
    resize_request: Option<PhysicalSize<u32>>,
    ui_renderer: Renderer,
    pub egui_ctx: Context,
    egui_state: egui_winit::State,
    draw_egui: bool,

    // Map system
    map_system: MapSystem,

    // Mouse state for panning
    mouse_pressed: bool,
    last_mouse_pos: Option<(f32, f32)>,
    current_mouse_pos: (f32, f32),
}

impl State {
    // We don't need this to be async right now,
    // but we will in the next tutorial
    pub async fn new(window: Arc<Window>) -> anyhow::Result<Self> {
        let instance = Instance::new(&InstanceDescriptor {
            backends: Backends::all(),
            ..Default::default()
        });

        let surface: wgpu::Surface<'_> = instance.create_surface(window.clone())?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Main Device"),
                required_features: Features::empty(),
                required_limits: if cfg!(target_arch = "wasm32") {
                    wgpu::Limits::downlevel_webgl2_defaults()
                } else {
                    wgpu::Limits::default()
                },
                experimental_features: ExperimentalFeatures::disabled(),
                memory_hints: MemoryHints::Performance,
                trace: Trace::Off,
            })
            .await?;

        let cap: wgpu::SurfaceCapabilities = surface.get_capabilities(&adapter);

        let texture_format = cap
            .formats
            .iter()
            .find(|format| {
                **format == TextureFormat::Rgba8Unorm || **format == TextureFormat::Bgra8Unorm
            })
            .copied()
            .unwrap_or(cap.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: texture_format,
            width: window.inner_size().width,
            height: window.inner_size().height,
            present_mode: cap.present_modes[0],
            alpha_mode: cap.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        let ui_renderer = Renderer::new(
            &device,
            texture_format,
            RendererOptions {
                msaa_samples: 0,
                depth_stencil_format: None,
                dithering: false,
                predictable_texture_filtering: false,
            },
        );
        let egui_ctx = Context::default();

        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            egui_ctx.viewport_id(),
            window.as_ref(),
            egui_ctx.native_pixels_per_point(),
            window.theme(),
            None,
        );

        // Create map system
        let map_system = MapSystem::new(
            &device,
            texture_format,
            window.inner_size().width,
            window.inner_size().height,
        );

        Ok(Self {
            window,
            surface,
            device,
            queue,
            config,
            is_surface_configured: false,
            resize_request: None,
            ui_renderer,
            egui_ctx,
            egui_state,
            draw_egui: true,
            map_system,
            mouse_pressed: false,
            last_mouse_pos: None,
            current_mouse_pos: (0.0, 0.0),
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            if !self.is_surface_configured {
                self.apply_size(width, height);
                self.is_surface_configured = true;
            } else {
                self.resize_request = Some(PhysicalSize::new(width, height));
            }
        }
    }

    fn apply_size(&mut self, width: u32, height: u32) {
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.map_system.resize(width, height);
    }

    pub fn handle_input(&mut self, event: &WindowEvent) -> bool {
        let response = self
            .egui_state
            .on_window_event(self.window.as_ref(), &event);
        self.draw_egui = response.repaint;

        // If egui consumed it, don't process map input
        if response.consumed {
            return true;
        }

        // Handle map-specific input
        match event {
            WindowEvent::MouseInput { state, button, .. } => {
                if *button == MouseButton::Left {
                    self.mouse_pressed = *state == ElementState::Pressed;
                    if !self.mouse_pressed {
                        self.last_mouse_pos = None;
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let (x, y) = (position.x as f32, position.y as f32);
                self.current_mouse_pos = (x, y);

                if self.mouse_pressed {
                    if let Some((last_x, last_y)) = self.last_mouse_pos {
                        let dx = x - last_x;
                        let dy = y - last_y;
                        self.map_system.pan(dx, dy);
                    }
                    self.last_mouse_pos = Some((x, y));
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let zoom_delta = match delta {
                    MouseScrollDelta::LineDelta(_, y) => *y as f64 * 0.5,
                    MouseScrollDelta::PixelDelta(pos) => pos.y as f64 * 0.01,
                };
                let (mx, my) = self.current_mouse_pos;
                self.map_system.zoom_at(zoom_delta, mx, my);
            }
            _ => {}
        }

        response.consumed
    }

    pub fn update(&mut self) {
        // Update map system
        self.map_system.update(&self.device, &self.queue);
    }

    fn draw_egui(&mut self) -> FullOutput {
        let input = self.egui_state.take_egui_input(self.window.as_ref());
        let context = self.egui_ctx.clone();
        let output = context.run(input, |ctx| {
            self.egui(ctx);
        });
        output
    }

    fn egui(&mut self, ctx: &Context) {
        // Update egui
        let map_center = self.map_system.center();
        let map_zoom = self.map_system.zoom_level();
        let cache_stats = self.map_system.cache_stats();
        let pending = self.map_system.pending_tiles();

        TopBottomPanel::top("menu").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "Zoom: {:.1} | Center: ({:.4}, {:.4})",
                    map_zoom, map_center.0, map_center.1
                ));
                ui.separator();
                ui.label(format!(
                    "Cache: {}/{} ({:.0}%)",
                    cache_stats.tile_count,
                    cache_stats.max_tiles,
                    cache_stats.tile_usage_percent()
                ));
                if pending > 0 {
                    ui.separator();
                    ui.label(format!("Loading: {}", pending));
                }
            });
        });
    }

    pub fn render(&mut self) -> Result<(), SurfaceError> {
        self.window.request_redraw();

        if !self.is_surface_configured {
            return Ok(());
        }

        if let Some(PhysicalSize { width, height }) = self.resize_request.take() {
            self.apply_size(width, height)
        }

        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(_) => {
                self.surface.configure(&self.device, &self.config);
                self.surface.get_current_texture()?
            }
        };

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        // Render map tiles first
        {
            let output = self.draw_egui();
            let FullOutput {
                platform_output,
                textures_delta,
                shapes,
                pixels_per_point,
                .. // viewport is ignored
            } = output;

            self.egui_state
                .handle_platform_output(self.window.as_ref(), platform_output);

            for (id, delta) in textures_delta.set {
                self.ui_renderer
                    .update_texture(&self.device, &self.queue, id, &delta);
            }
            let primitives = self.egui_ctx.tessellate(shapes, pixels_per_point);
            let descriptor = ScreenDescriptor {
                size_in_pixels: [self.config.width, self.config.height],
                pixels_per_point,
            };
            self.ui_renderer.update_buffers(
                &self.device,
                &self.queue,
                &mut encoder,
                &primitives,
                &descriptor,
            );

            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.8,
                            g: 0.85,
                            b: 0.9,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });

            self.map_system.render(&mut render_pass, &self.device);
            let mut render_pass = render_pass.forget_lifetime();
            if self.draw_egui {
                self.ui_renderer
                    .render(&mut render_pass, &primitives, &descriptor);
            }
            for id in textures_delta.free {
                self.ui_renderer.free_texture(&id);
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();

        Ok(())
    }
}
