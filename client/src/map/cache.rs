//! LRU tile cache for GPU textures

use std::collections::HashMap;
use std::sync::Arc;
use web_time::Instant;

use super::tile::TileId;

/// Cached tile with GPU resources
pub struct CachedTile {
    pub texture: wgpu::Texture,
    pub texture_view: wgpu::TextureView,
    pub bind_group: wgpu::BindGroup,
    pub memory_size: usize,
    pub created_at: Instant,
}

/// LRU cache for map tiles
pub struct TileCache {
    tiles: HashMap<TileId, Arc<CachedTile>>,
    access_order: Vec<TileId>,
    max_tiles: usize,
    current_memory: usize,
    max_memory: usize,
}

impl TileCache {
    /// Create a new tile cache
    /// - max_tiles: Maximum number of tiles to cache (e.g., 256)
    /// - max_memory: Maximum GPU memory in bytes (e.g., 64MB)
    pub fn new(max_tiles: usize, max_memory: usize) -> Self {
        Self {
            tiles: HashMap::with_capacity(max_tiles),
            access_order: Vec::with_capacity(max_tiles),
            max_tiles,
            current_memory: 0,
            max_memory,
        }
    }

    /// Check if tile exists in cache
    pub fn contains(&self, tile_id: &TileId) -> bool {
        self.tiles.contains_key(tile_id)
    }

    /// Get a tile from cache, updating access order
    pub fn get(&mut self, tile_id: &TileId) -> Option<Arc<CachedTile>> {
        if self.tiles.contains_key(tile_id) {
            self.update_access_order(*tile_id);
            self.tiles.get(tile_id).cloned()
        } else {
            None
        }
    }

    /// Get a tile without updating access order (for read-only checks)
    pub fn peek(&self, tile_id: &TileId) -> Option<Arc<CachedTile>> {
        self.tiles.get(tile_id).cloned()
    }

    /// Insert a new tile into cache, evicting old tiles if necessary
    pub fn insert(&mut self, tile_id: TileId, tile: CachedTile) {
        let memory_size = tile.memory_size;

        // Evict tiles if we're over capacity
        while self.should_evict(memory_size) {
            if !self.evict_oldest() {
                break;
            }
        }

        // Remove if already exists (update case)
        if let Some(old) = self.tiles.remove(&tile_id) {
            self.current_memory -= old.memory_size;
            self.access_order.retain(|id| id != &tile_id);
        }

        self.current_memory += memory_size;
        self.tiles.insert(tile_id, Arc::new(tile));
        self.access_order.push(tile_id);
    }

    /// Check if we need to evict tiles
    fn should_evict(&self, new_tile_memory: usize) -> bool {
        !self.tiles.is_empty()
            && (self.tiles.len() >= self.max_tiles
                || self.current_memory + new_tile_memory > self.max_memory)
    }

    /// Evict the oldest (least recently used) tile
    fn evict_oldest(&mut self) -> bool {
        if let Some(oldest_id) = self.access_order.first().cloned() {
            if let Some(tile) = self.tiles.remove(&oldest_id) {
                self.current_memory -= tile.memory_size;
                self.access_order.remove(0);
                log::debug!("Evicted tile {:?}", oldest_id);
                return true;
            }
        }
        false
    }

    /// Update access order for LRU tracking
    fn update_access_order(&mut self, tile_id: TileId) {
        if let Some(pos) = self.access_order.iter().position(|id| id == &tile_id) {
            self.access_order.remove(pos);
            self.access_order.push(tile_id);
        }
    }

    /// Remove a specific tile from cache
    pub fn remove(&mut self, tile_id: &TileId) -> Option<Arc<CachedTile>> {
        if let Some(tile) = self.tiles.remove(tile_id) {
            self.current_memory -= tile.memory_size;
            self.access_order.retain(|id| id != tile_id);
            Some(tile)
        } else {
            None
        }
    }

    /// Clear all tiles from cache
    pub fn clear(&mut self) {
        self.tiles.clear();
        self.access_order.clear();
        self.current_memory = 0;
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            tile_count: self.tiles.len(),
            max_tiles: self.max_tiles,
            memory_used: self.current_memory,
            max_memory: self.max_memory,
        }
    }

    /// Get number of cached tiles
    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    /// Iterate over all cached tile IDs
    pub fn tile_ids(&self) -> impl Iterator<Item = &TileId> {
        self.tiles.keys()
    }
}

/// Cache statistics for debugging/UI
#[derive(Debug, Clone, Copy)]
pub struct CacheStats {
    pub tile_count: usize,
    pub max_tiles: usize,
    pub memory_used: usize,
    pub max_memory: usize,
}

impl CacheStats {
    pub fn memory_usage_percent(&self) -> f32 {
        if self.max_memory == 0 {
            0.0
        } else {
            (self.memory_used as f32 / self.max_memory as f32) * 100.0
        }
    }

    pub fn tile_usage_percent(&self) -> f32 {
        if self.max_tiles == 0 {
            0.0
        } else {
            (self.tile_count as f32 / self.max_tiles as f32) * 100.0
        }
    }
}

impl Default for TileCache {
    fn default() -> Self {
        // Default: 256 tiles, 64MB max
        Self::new(256, 64 * 1024 * 1024)
    }
}
