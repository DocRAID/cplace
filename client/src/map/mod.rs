//! Map system with tile rendering, caching, and pixel grid overlay

pub mod cache;
pub mod camera;
pub mod grid;
pub mod loader;
pub mod renderer;
pub mod tile;

use cache::TileCache;
use camera::MapCamera;
use grid::PixelGrid;
use loader::{TileLoadResult, TileLoader};
use renderer::{screen_to_ndc, size_to_ndc, TileRenderer};
use tile::TileId;

/// Integrated map system
pub struct MapSystem {
    pub camera: MapCamera,
    tile_cache: TileCache,
    tile_loader: TileLoader,
    tile_renderer: TileRenderer,
    pub pixel_grid: PixelGrid,

    /// Tiles to render this frame (calculated in update)
    render_tiles: Vec<(TileId, (f32, f32), f32)>,
}

impl MapSystem {
    /// Create a new map system
    pub fn new(
        device: &wgpu::Device,
        texture_format: wgpu::TextureFormat,
        viewport_width: u32,
        viewport_height: u32,
    ) -> Self {
        // Default camera: Seoul at zoom 12
        let camera = MapCamera::new(126.9780, 37.5665, 12.0, viewport_width, viewport_height);

        let tile_cache = TileCache::default();
        let tile_loader = TileLoader::default();
        let tile_renderer = TileRenderer::new(device, texture_format);

        // Pixel grid with ~10m cell size at equator
        let pixel_grid = PixelGrid::new(device, texture_format, 0.0001);

        Self {
            camera,
            tile_cache,
            tile_loader,
            tile_renderer,
            pixel_grid,
            render_tiles: Vec::new(),
        }
    }

    /// Update the map system (call each frame)
    pub fn update(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        // 1. Get visible tiles
        let visible = self.camera.visible_tiles();

        // 2. Request loading for tiles not in cache
        for tile_id in &visible {
            if !self.tile_cache.contains(tile_id) && !self.tile_loader.is_loading(tile_id) {
                self.tile_loader.request(*tile_id);
            }
        }

        // 3. Process completed loads
        while let Some(result) = self.tile_loader.poll() {
            match result {
                TileLoadResult::Success(id, data) => {
                    match self.tile_renderer.create_cached_tile(device, queue, &data) {
                        Ok(cached) => {
                            log::debug!("Loaded tile {:?}", id);
                            self.tile_cache.insert(id, cached);
                        }
                        Err(e) => {
                            log::warn!("Failed to decode tile {:?}: {}", id, e);
                        }
                    }
                }
                TileLoadResult::Failed(id, err) => {
                    log::warn!("Failed to load tile {:?}: {}", id, err);
                }
            }
        }

        // 4. Build render list with screen positions
        self.render_tiles.clear();
        let tile_size = self.camera.tile_screen_size();

        for tile_id in &visible {
            // Only add to render list if cached
            if self.tile_cache.contains(tile_id) {
                let (x, y) = self.camera.tile_to_screen(tile_id);

                // Convert to NDC
                let (ndc_x, ndc_y) =
                    screen_to_ndc(x, y, self.camera.viewport_width, self.camera.viewport_height);
                let (ndc_w, ndc_h) = size_to_ndc(
                    tile_size,
                    self.camera.viewport_width,
                    self.camera.viewport_height,
                );

                self.render_tiles.push((*tile_id, (ndc_x, ndc_y), ndc_w));
            }
        }

        // 5. Update pixel grid
        self.pixel_grid.update(device, &self.camera);
    }

    /// Render the map
    pub fn render<'a>(
        &'a self,
        render_pass: &mut wgpu::RenderPass<'a>,
        device: &wgpu::Device,
    ) {
        // Render tiles
        self.tile_renderer
            .render(render_pass, device, &self.render_tiles, &self.tile_cache);

        // Render pixel grid overlay
        self.pixel_grid.render(render_pass);
    }

    /// Handle viewport resize
    pub fn resize(&mut self, width: u32, height: u32) {
        self.camera.set_viewport(width, height);
    }

    /// Pan the map by pixel delta
    pub fn pan(&mut self, dx: f32, dy: f32) {
        self.camera.pan(dx, dy);
    }

    /// Zoom at screen position
    pub fn zoom_at(&mut self, delta: f64, screen_x: f32, screen_y: f32) {
        self.camera.zoom_at(delta, screen_x, screen_y);
    }

    /// Zoom centered
    pub fn zoom(&mut self, delta: f64) {
        self.camera.zoom_by(delta);
    }

    /// Convert screen position to world coordinates
    pub fn screen_to_world(&self, screen_x: f32, screen_y: f32) -> (f64, f64) {
        self.camera.screen_to_world(screen_x, screen_y)
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> cache::CacheStats {
        self.tile_cache.stats()
    }

    /// Get pending tile count
    pub fn pending_tiles(&self) -> usize {
        self.tile_loader.pending_count()
    }

    /// Get current zoom level
    pub fn zoom_level(&self) -> f64 {
        self.camera.zoom
    }

    /// Get current center position
    pub fn center(&self) -> (f64, f64) {
        self.camera.center
    }

    /// Set center position
    pub fn set_center(&mut self, lon: f64, lat: f64) {
        self.camera.center = (
            tile::normalize_longitude(lon),
            tile::clamp_latitude(lat),
        );
    }

    /// Set zoom level
    pub fn set_zoom(&mut self, zoom: f64) {
        self.camera.zoom = zoom.clamp(0.0, 19.0);
    }
}
