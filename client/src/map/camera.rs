//! Map camera for viewport management, panning, and zooming

use super::tile::{
    clamp_latitude, is_valid_tile_y, lon_lat_to_tile_f64, normalize_longitude, wrap_tile_x, TileId,
};

/// Tile size in pixels (standard OSM tile size)
pub const TILE_SIZE: f64 = 256.0;

/// Map camera state
pub struct MapCamera {
    /// Center position (longitude, latitude)
    pub center: (f64, f64),

    /// Current zoom level (fractional for smooth zooming)
    pub zoom: f64,

    /// Viewport size in pixels
    pub viewport_width: u32,
    pub viewport_height: u32,
}

impl MapCamera {
    pub fn new(lon: f64, lat: f64, zoom: f64, width: u32, height: u32) -> Self {
        Self {
            center: (normalize_longitude(lon), clamp_latitude(lat)),
            zoom: zoom.clamp(0.0, 19.0),
            viewport_width: width,
            viewport_height: height,
        }
    }

    /// Update viewport size
    pub fn set_viewport(&mut self, width: u32, height: u32) {
        self.viewport_width = width;
        self.viewport_height = height;
    }

    /// Get the integer zoom level for tile loading
    pub fn tile_zoom(&self) -> u8 {
        self.zoom.floor() as u8
    }

    /// Get scale factor for current fractional zoom
    pub fn zoom_scale(&self) -> f64 {
        2.0_f64.powf(self.zoom - self.zoom.floor())
    }

    /// Meters per pixel at current zoom and latitude
    pub fn meters_per_pixel(&self) -> f64 {
        let earth_circumference = 40075016.686; // meters
        let lat_rad = self.center.1.to_radians();
        earth_circumference * lat_rad.cos() / (TILE_SIZE * 2.0_f64.powf(self.zoom))
    }

    /// Pan the map by pixel delta
    pub fn pan(&mut self, dx_pixels: f32, dy_pixels: f32) {
        let meters_per_pixel = self.meters_per_pixel();

        // Longitude change (X axis - wraps infinitely)
        let cos_lat = self.center.1.to_radians().cos().max(0.01);
        let lon_delta = (dx_pixels as f64) * meters_per_pixel / (111320.0 * cos_lat);
        self.center.0 = normalize_longitude(self.center.0 - lon_delta);

        // Latitude change (Y axis - clamped)
        let lat_delta = (dy_pixels as f64) * meters_per_pixel / 111320.0;
        self.center.1 = clamp_latitude(self.center.1 + lat_delta);
    }

    /// Zoom at a specific screen point
    pub fn zoom_at(&mut self, delta: f64, screen_x: f32, screen_y: f32) {
        let old_zoom = self.zoom;
        self.zoom = (self.zoom + delta).clamp(0.0, 19.0);

        if (self.zoom - old_zoom).abs() < 0.001 {
            return;
        }

        // Calculate the world position under the cursor before zoom
        let offset_x = screen_x - (self.viewport_width as f32 / 2.0);
        let offset_y = screen_y - (self.viewport_height as f32 / 2.0);

        // Adjust center to keep the point under cursor stationary
        let scale_change = 2.0_f64.powf(self.zoom - old_zoom);
        let new_offset_x = offset_x as f64 * (1.0 - 1.0 / scale_change);
        let new_offset_y = offset_y as f64 * (1.0 - 1.0 / scale_change);

        // Convert pixel offset to geo offset
        let meters_per_pixel = self.meters_per_pixel();
        let cos_lat = self.center.1.to_radians().cos().max(0.01);

        let lon_delta = new_offset_x * meters_per_pixel / (111320.0 * cos_lat);
        let lat_delta = new_offset_y * meters_per_pixel / 111320.0;

        self.center.0 = normalize_longitude(self.center.0 + lon_delta);
        self.center.1 = clamp_latitude(self.center.1 - lat_delta);
    }

    /// Simple zoom (centered)
    pub fn zoom_by(&mut self, delta: f64) {
        self.zoom = (self.zoom + delta).clamp(0.0, 19.0);
    }

    /// Get list of visible tiles with buffer for pre-loading
    pub fn visible_tiles(&self) -> Vec<TileId> {
        self.visible_tiles_with_buffer(1)
    }

    /// Get visible tiles with specified buffer tiles around viewport
    pub fn visible_tiles_with_buffer(&self, buffer: i32) -> Vec<TileId> {
        let z = self.tile_zoom();
        let scale = self.zoom_scale();
        let scaled_tile_size = TILE_SIZE * scale;

        // Center tile position (fractional)
        let (cx, cy) = lon_lat_to_tile_f64(self.center.0, self.center.1, z);

        // How many tiles fit in the viewport
        let tiles_x = (self.viewport_width as f64 / scaled_tile_size).ceil() as i32 + 1;
        let tiles_y = (self.viewport_height as f64 / scaled_tile_size).ceil() as i32 + 1;

        // Calculate tile range
        let half_tiles_x = tiles_x / 2 + buffer;
        let half_tiles_y = tiles_y / 2 + buffer;

        let min_x = cx.floor() as i32 - half_tiles_x;
        let max_x = cx.ceil() as i32 + half_tiles_x;
        let min_y = cy.floor() as i32 - half_tiles_y;
        let max_y = cy.ceil() as i32 + half_tiles_y;

        // Collect tiles with X-axis wrapping
        let mut tiles = Vec::new();
        for ty in min_y..=max_y {
            if !is_valid_tile_y(ty, z) {
                continue;
            }
            for tx in min_x..=max_x {
                let wrapped_x = wrap_tile_x(tx, z);
                tiles.push(TileId::new(wrapped_x, ty as u32, z));
            }
        }

        tiles
    }

    /// Convert tile coordinates to screen position (top-left corner)
    pub fn tile_to_screen(&self, tile: &TileId) -> (f32, f32) {
        let z = self.tile_zoom();
        let scale = self.zoom_scale();
        let scaled_tile_size = TILE_SIZE * scale;

        // Center tile position (fractional)
        let (cx, cy) = lon_lat_to_tile_f64(self.center.0, self.center.1, z);

        // Tile position relative to center
        let mut rel_x = tile.x as f64 - cx;
        let mut rel_y = tile.y as f64 - cy;

        // Handle world wrapping for X axis
        let max_tiles = (1_u64 << z) as f64;
        if rel_x > max_tiles / 2.0 {
            rel_x -= max_tiles;
        } else if rel_x < -max_tiles / 2.0 {
            rel_x += max_tiles;
        }

        // Convert to screen coordinates
        let screen_x = (self.viewport_width as f64 / 2.0) + (rel_x * scaled_tile_size);
        let screen_y = (self.viewport_height as f64 / 2.0) + (rel_y * scaled_tile_size);

        (screen_x as f32, screen_y as f32)
    }

    /// Get the screen size of a tile at current zoom
    pub fn tile_screen_size(&self) -> f32 {
        let scale = self.zoom_scale();
        (TILE_SIZE * scale) as f32
    }

    /// Convert screen coordinates to world coordinates (lon, lat)
    pub fn screen_to_world(&self, screen_x: f32, screen_y: f32) -> (f64, f64) {
        let meters_per_pixel = self.meters_per_pixel();
        let cos_lat = self.center.1.to_radians().cos().max(0.01);

        let offset_x = screen_x - (self.viewport_width as f32 / 2.0);
        let offset_y = screen_y - (self.viewport_height as f32 / 2.0);

        let lon_delta = (offset_x as f64) * meters_per_pixel / (111320.0 * cos_lat);
        let lat_delta = (offset_y as f64) * meters_per_pixel / 111320.0;

        let lon = normalize_longitude(self.center.0 + lon_delta);
        let lat = clamp_latitude(self.center.1 - lat_delta);

        (lon, lat)
    }
}

impl Default for MapCamera {
    fn default() -> Self {
        // Default to Seoul at zoom 10
        Self::new(126.9780, 37.5665, 10.0, 800, 600)
    }
}
