use std::sync::Arc;
use egui::{Context, FullOutput, TopBottomPanel};
use egui_wgpu::{Renderer, RendererOptions, ScreenDescriptor};
use log::warn;
use wgpu::{Backends, ExperimentalFeatures, Features, Instance, InstanceDescriptor, MemoryHints, SurfaceError, Trace};
use winit::window::Window;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;

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
    egui_ctx: Context,
    egui_state: egui_winit::State,
    draw_egui: bool,
    egui_output: Option<FullOutput>,
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
            .find(|format| format.is_srgb())
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

        let ui_renderer = Renderer::new(&device, texture_format, RendererOptions {
            msaa_samples: 0,
            depth_stencil_format: None,
            dithering: false,
            predictable_texture_filtering: false,
        });
        let egui_ctx = Context::default();

        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            egui_ctx.viewport_id(),
            window.as_ref(),
            egui_ctx.native_pixels_per_point(),
            window.theme(),
            None,
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
            egui_output: None,
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
    }

    pub fn handle_input(&mut self, event: &WindowEvent) -> bool {
        let response = self.egui_state.on_window_event(self.window.as_ref(), &event);
        self.draw_egui = response.repaint;
        response.consumed
    }

    pub fn update(&mut self) {
        let input = self.egui_state.take_egui_input(self.window.as_ref());
        let output = self.egui_ctx.run(input, |ctx| {
            TopBottomPanel::top("menu").show(ctx, |ui| {
                ui.label("aiosdhfoidshifoahjfiposdf");
            });
        });
        self.egui_output = Some(output);
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

        {
            let render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.65,
                            g: 0.98,
                            b: 1.0,
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

            if self.draw_egui {
                let output = self.egui_output.take().unwrap();
                let FullOutput {
                    platform_output,
                    textures_delta,
                    shapes,
                    pixels_per_point,
                    viewport_output
                } = output;

                for _ in viewport_output {
                    warn!("Viewport change is not handled!")
                }

                self.egui_state.handle_platform_output(self.window.as_ref(), platform_output);

                for (id, delta) in textures_delta.set {
                    self.ui_renderer.update_texture(&self.device, &self.queue, id, &delta);
                }
                let descriptor = ScreenDescriptor {
                    size_in_pixels: [self.config.width, self.config.height],
                    pixels_per_point,
                };
                let primitives = self.egui_ctx.tessellate(shapes, pixels_per_point);
                let mut render_pass = render_pass.forget_lifetime();

                self.ui_renderer.update_buffers(&self.device, &self.queue, &mut encoder, &primitives, &descriptor);
                self.ui_renderer.render(&mut render_pass, &primitives, &descriptor);

                for id in textures_delta.free {
                    self.ui_renderer.free_texture(&id)
                }
            }
        }


        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();

        Ok(())
    }
}
