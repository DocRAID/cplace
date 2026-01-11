//! Tile coordinate system and conversions
//! Uses Web Mercator projection (EPSG:3857) compatible with OSM

use std::f64::consts::PI;

/// Unique identifier for a map tile
#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub struct TileId {
    pub x: u32,
    pub y: u32,
    pub z: u8,
}

impl TileId {
    pub fn new(x: u32, y: u32, z: u8) -> Self {
        Self { x, y, z }
    }

    /// Get the maximum tile coordinate for this zoom level
    pub fn max_tile_coord(&self) -> u32 {
        1 << self.z // 2^z
    }

    /// Get parent tile at a lower zoom level
    pub fn parent_at_zoom(&self, target_z: u8) -> Option<TileId> {
        if target_z >= self.z {
            return None;
        }
        let diff = self.z - target_z;
        Some(TileId {
            x: self.x >> diff,
            y: self.y >> diff,
            z: target_z,
        })
    }

    /// Build OSM tile URL
    pub fn to_osm_url(&self) -> String {
        format!(
            "https://tile.openstreetmap.org/{}/{}/{}.png",
            self.z, self.x, self.y
        )
    }
}

/// Convert longitude/latitude to tile coordinates at given zoom
pub fn lon_lat_to_tile(lon: f64, lat: f64, zoom: u8) -> (u32, u32) {
    let n = (1_u64 << zoom) as f64; // 2^zoom

    // X coordinate (longitude)
    let x = ((lon + 180.0) / 360.0 * n).floor() as u32;

    // Y coordinate (latitude) - Mercator projection
    let lat_rad = lat.to_radians();
    let y = ((1.0 - lat_rad.tan().asinh() / PI) / 2.0 * n).floor() as u32;

    // Clamp to valid range
    let max_tile = (1_u32 << zoom) - 1;
    (x.min(max_tile), y.min(max_tile))
}

/// Convert longitude/latitude to fractional tile coordinates (for sub-tile positioning)
pub fn lon_lat_to_tile_f64(lon: f64, lat: f64, zoom: u8) -> (f64, f64) {
    let n = (1_u64 << zoom) as f64;

    let x = (lon + 180.0) / 360.0 * n;

    let lat_rad = lat.to_radians();
    let y = (1.0 - lat_rad.tan().asinh() / PI) / 2.0 * n;

    (x, y)
}

/// Convert tile coordinates to longitude/latitude (top-left corner of tile)
pub fn tile_to_lon_lat(x: u32, y: u32, zoom: u8) -> (f64, f64) {
    let n = (1_u64 << zoom) as f64;

    let lon = x as f64 / n * 360.0 - 180.0;
    let lat_rad = (PI * (1.0 - 2.0 * y as f64 / n)).sinh().atan();

    (lon, lat_rad.to_degrees())
}

/// Wrap X coordinate for infinite horizontal scrolling
pub fn wrap_tile_x(x: i32, zoom: u8) -> u32 {
    let max_tiles = 1_i32 << zoom;
    ((x % max_tiles) + max_tiles) as u32 % max_tiles as u32
}

/// Check if Y coordinate is valid (no wrapping for latitude)
pub fn is_valid_tile_y(y: i32, zoom: u8) -> bool {
    let max_tiles = 1_i32 << zoom;
    y >= 0 && y < max_tiles
}

/// Normalize longitude to [-180, 180]
pub fn normalize_longitude(lon: f64) -> f64 {
    let mut l = lon;
    while l < -180.0 {
        l += 360.0;
    }
    while l > 180.0 {
        l -= 360.0;
    }
    l
}

/// Clamp latitude to valid Mercator range
pub fn clamp_latitude(lat: f64) -> f64 {
    lat.clamp(-85.05112878, 85.05112878)
}

/// Calculate sub-region UV coordinates when using parent tile as fallback
pub fn calculate_sub_region(target: &TileId, parent: &TileId) -> (f32, f32, f32, f32) {
    if parent.z >= target.z {
        return (0.0, 0.0, 1.0, 1.0);
    }

    let zoom_diff = target.z - parent.z;
    let subdivisions = 1_u32 << zoom_diff;

    let local_x = target.x % subdivisions;
    let local_y = target.y % subdivisions;

    let size = 1.0 / subdivisions as f32;
    let u0 = local_x as f32 * size;
    let v0 = local_y as f32 * size;

    (u0, v0, u0 + size, v0 + size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lon_lat_to_tile() {
        // Seoul (approx 126.9780, 37.5665)
        let (x, y) = lon_lat_to_tile(126.9780, 37.5665, 10);
        assert_eq!(x, 872);
        assert_eq!(y, 395);
    }

    #[test]
    fn test_wrap_tile_x() {
        // At zoom 2, max tiles = 4 (0-3)
        assert_eq!(wrap_tile_x(4, 2), 0);  // Wrap around
        assert_eq!(wrap_tile_x(-1, 2), 3); // Wrap negative
        assert_eq!(wrap_tile_x(2, 2), 2);  // Normal
    }

    #[test]
    fn test_normalize_longitude() {
        assert!((normalize_longitude(190.0) - (-170.0)).abs() < 0.001);
        assert!((normalize_longitude(-190.0) - 170.0).abs() < 0.001);
    }
}
