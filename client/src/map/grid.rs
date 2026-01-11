//! Pixel grid overlay for drawing on the map

use bytemuck::{Pod, Zeroable};
use std::collections::HashMap;
use wgpu::include_wgsl;
use wgpu::util::DeviceExt;

/// Grid vertex for colored quads
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct GridVertex {
    pub position: [f32; 3],
    pub color: [f32; 4],
}

impl GridVertex {
    const ATTRIBS: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![
        0 => Float32x3,
        1 => Float32x4,
    ];

    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<GridVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

/// A single pixel in the grid
#[derive(Clone, Copy, Debug)]
pub struct Pixel {
    pub color: [f32; 4], // RGBA
}

impl Default for Pixel {
    fn default() -> Self {
        Self {
            color: [0.0, 0.0, 0.0, 0.0], // Transparent
        }
    }
}

/// Grid coordinates (world-space pixel position)
#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub struct GridCoord {
    pub x: i64,
    pub y: i64,
}

impl GridCoord {
    pub fn new(x: i64, y: i64) -> Self {
        Self { x, y }
    }
}

/// Pixel grid overlay system
pub struct PixelGrid {
    /// Stored pixels (sparse storage)
    pixels: HashMap<GridCoord, Pixel>,

    /// Grid cell size in world units (degrees)
    pub cell_size: f64,

    /// Render pipeline
    render_pipeline: wgpu::RenderPipeline,

    /// Cached vertex buffer (rebuilt when pixels change)
    vertex_buffer: Option<wgpu::Buffer>,
    vertex_count: u32,

    /// Dirty flag for buffer rebuild
    dirty: bool,
}

impl PixelGrid {
    /// Create a new pixel grid
    /// cell_size: size of each pixel in degrees (e.g., 0.0001 for ~10m at equator)
    pub fn new(device: &wgpu::Device, texture_format: wgpu::TextureFormat, cell_size: f64) -> Self {
        let shader = device.create_shader_module(include_wgsl!("../shader/grid.wgsl"));

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Grid Pipeline Layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Grid Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[GridVertex::desc()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: texture_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Self {
            pixels: HashMap::new(),
            cell_size,
            render_pipeline,
            vertex_buffer: None,
            vertex_count: 0,
            dirty: false,
        }
    }

    /// Set a pixel at grid coordinates
    pub fn set_pixel(&mut self, coord: GridCoord, color: [f32; 4]) {
        self.pixels.insert(coord, Pixel { color });
        self.dirty = true;
    }

    /// Get a pixel at grid coordinates
    pub fn get_pixel(&self, coord: &GridCoord) -> Option<&Pixel> {
        self.pixels.get(coord)
    }

    /// Remove a pixel
    pub fn remove_pixel(&mut self, coord: &GridCoord) -> Option<Pixel> {
        self.dirty = true;
        self.pixels.remove(coord)
    }

    /// Clear all pixels
    pub fn clear(&mut self) {
        self.pixels.clear();
        self.dirty = true;
    }

    /// Convert world coordinates (lon, lat) to grid coordinates
    pub fn world_to_grid(&self, lon: f64, lat: f64) -> GridCoord {
        GridCoord {
            x: (lon / self.cell_size).floor() as i64,
            y: (lat / self.cell_size).floor() as i64,
        }
    }

    /// Convert grid coordinates to world coordinates (center of cell)
    pub fn grid_to_world(&self, coord: &GridCoord) -> (f64, f64) {
        let lon = (coord.x as f64 + 0.5) * self.cell_size;
        let lat = (coord.y as f64 + 0.5) * self.cell_size;
        (lon, lat)
    }

    /// Get number of pixels
    pub fn pixel_count(&self) -> usize {
        self.pixels.len()
    }

    /// Update vertex buffer if dirty
    pub fn update(
        &mut self,
        device: &wgpu::Device,
        camera: &super::camera::MapCamera,
    ) {
        if !self.dirty && self.vertex_buffer.is_some() {
            return;
        }

        let mut vertices = Vec::new();

        for (coord, pixel) in &self.pixels {
            // Convert grid to world coordinates
            let (lon, lat) = self.grid_to_world(coord);

            // Check if visible (rough culling)
            let (center_lon, center_lat) = camera.center;
            let view_range = 180.0 / 2.0_f64.powf(camera.zoom); // Approximate visible range

            if (lon - center_lon).abs() > view_range * 2.0
                || (lat - center_lat).abs() > view_range * 2.0
            {
                continue;
            }

            // Convert to screen coordinates, then to NDC
            // This is simplified - in production you'd use proper projection
            let half_cell = self.cell_size / 2.0;

            // Get screen position for the cell corners
            let corners = [
                (lon - half_cell, lat - half_cell), // Bottom-left
                (lon + half_cell, lat - half_cell), // Bottom-right
                (lon + half_cell, lat + half_cell), // Top-right
                (lon - half_cell, lat + half_cell), // Top-left
            ];

            // Convert corners to screen positions
            let screen_corners: Vec<(f32, f32)> = corners
                .iter()
                .map(|(lo, la)| world_to_screen(*lo, *la, camera))
                .collect();

            // Create two triangles for the quad
            let color = pixel.color;

            // Triangle 1: 0, 1, 2
            vertices.push(GridVertex {
                position: [screen_corners[0].0, screen_corners[0].1, 0.0],
                color,
            });
            vertices.push(GridVertex {
                position: [screen_corners[1].0, screen_corners[1].1, 0.0],
                color,
            });
            vertices.push(GridVertex {
                position: [screen_corners[2].0, screen_corners[2].1, 0.0],
                color,
            });

            // Triangle 2: 0, 2, 3
            vertices.push(GridVertex {
                position: [screen_corners[0].0, screen_corners[0].1, 0.0],
                color,
            });
            vertices.push(GridVertex {
                position: [screen_corners[2].0, screen_corners[2].1, 0.0],
                color,
            });
            vertices.push(GridVertex {
                position: [screen_corners[3].0, screen_corners[3].1, 0.0],
                color,
            });
        }

        self.vertex_count = vertices.len() as u32;

        if !vertices.is_empty() {
            self.vertex_buffer = Some(device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Grid Vertex Buffer"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            }));
        } else {
            self.vertex_buffer = None;
        }

        self.dirty = false;
    }

    /// Render the grid overlay
    pub fn render<'a>(&'a self, render_pass: &mut wgpu::RenderPass<'a>) {
        if self.vertex_count == 0 {
            return;
        }

        if let Some(ref buffer) = self.vertex_buffer {
            render_pass.set_pipeline(&self.render_pipeline);
            render_pass.set_vertex_buffer(0, buffer.slice(..));
            render_pass.draw(0..self.vertex_count, 0..1);
        }
    }

    /// Mark as dirty (forces rebuild on next update)
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }
}

/// Convert world coordinates to NDC screen position
fn world_to_screen(lon: f64, lat: f64, camera: &super::camera::MapCamera) -> (f32, f32) {
    use super::tile::lon_lat_to_tile_f64;

    let z = camera.tile_zoom();
    let scale = camera.zoom_scale();
    let tile_size = super::camera::TILE_SIZE * scale;

    // Get tile coordinates
    let (tx, ty) = lon_lat_to_tile_f64(lon, lat, z);
    let (cx, cy) = lon_lat_to_tile_f64(camera.center.0, camera.center.1, z);

    // Relative position
    let rel_x = tx - cx;
    let rel_y = ty - cy;

    // Screen position (centered)
    let screen_x = (camera.viewport_width as f64 / 2.0) + (rel_x * tile_size);
    let screen_y = (camera.viewport_height as f64 / 2.0) + (rel_y * tile_size);

    // Convert to NDC
    let ndc_x = (screen_x / camera.viewport_width as f64) as f32 * 2.0 - 1.0;
    let ndc_y = 1.0 - (screen_y / camera.viewport_height as f64) as f32 * 2.0;

    (ndc_x, ndc_y)
}
