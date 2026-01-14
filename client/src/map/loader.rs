//! Asynchronous tile loader with platform-specific implementations

use std::collections::HashSet;

use super::tile::TileId;

/// Result of a tile load operation
#[derive(Debug)]
pub enum TileLoadResult {
    Success(TileId, Vec<u8>),
    Failed(TileId, String),
}

/// Tile loading request
#[derive(Debug, Clone)]
struct TileRequest {
    tile_id: TileId,
    url: String,
}

// Platform-specific channel types
#[cfg(not(target_arch = "wasm32"))]
type ResultReceiver = std::sync::mpsc::Receiver<TileLoadResult>;
#[cfg(not(target_arch = "wasm32"))]
type RequestSender = std::sync::mpsc::Sender<TileRequest>;

#[cfg(target_arch = "wasm32")]
use std::sync::{Arc, Mutex};
use log::debug;

#[cfg(target_arch = "wasm32")]
type ResultReceiver = Arc<Mutex<Vec<TileLoadResult>>>;
#[cfg(target_arch = "wasm32")]
type RequestSender = (); // Not used in WASM

/// Tile loader with async HTTP fetching
pub struct TileLoader {
    result_rx: ResultReceiver,
    #[cfg(not(target_arch = "wasm32"))]
    request_tx: RequestSender,
    pending: HashSet<TileId>,
    user_agent: String,
    #[cfg(not(target_arch = "wasm32"))]
    _worker_handle: Option<std::thread::JoinHandle<()>>,
}

impl TileLoader {
    /// Create a new tile loader
    pub fn new(user_agent: &str) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (request_tx, request_rx) = std::sync::mpsc::channel::<TileRequest>();
            let (result_tx, result_rx) = std::sync::mpsc::channel::<TileLoadResult>();

            let _worker_handle = {
                let user_agent = user_agent.to_string();
                Some(std::thread::spawn(move || {
                    Self::worker_thread(request_rx, result_tx, user_agent);
                }))
            };

            Self {
                result_rx,
                request_tx,
                pending: HashSet::new(),
                user_agent: user_agent.to_string(),
                _worker_handle,
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            let result_rx = Arc::new(Mutex::new(Vec::new()));

            Self {
                result_rx,
                pending: HashSet::new(),
                user_agent: user_agent.to_string(),
            }
        }
    }

    /// Request a tile to be loaded
    pub fn request(&mut self, tile_id: TileId) {
        if self.pending.contains(&tile_id) {
            return; // Already loading
        }

        let url = tile_id.to_osm_url();
        // debug!("Requesting tile {}", url);
        let request = TileRequest { tile_id, url };

        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.request_tx.send(request).is_ok() {
                self.pending.insert(tile_id);
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            self.pending.insert(tile_id);
            self.spawn_wasm_fetch(request);
        }
    }

    /// Poll for completed tile loads
    pub fn poll(&mut self) -> Option<TileLoadResult> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            match self.result_rx.try_recv() {
                Ok(result) => {
                    match &result {
                        TileLoadResult::Success(id, _) | TileLoadResult::Failed(id, _) => {
                            self.pending.remove(id);
                        }
                    }
                    Some(result)
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => None,
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            let mut results = self.result_rx.lock().unwrap();
            if let Some(result) = results.pop() {
                match &result {
                    TileLoadResult::Success(id, _) | TileLoadResult::Failed(id, _) => {
                        self.pending.remove(id);
                    }
                }
                Some(result)
            } else {
                None
            }
        }
    }

    /// Check if a tile is currently being loaded
    pub fn is_loading(&self, tile_id: &TileId) -> bool {
        self.pending.contains(tile_id)
    }

    /// Get number of pending requests
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Cancel all pending requests (tiles will still complete but be ignored)
    pub fn clear_pending(&mut self) {
        self.pending.clear();
    }

    // Native implementation
    #[cfg(not(target_arch = "wasm32"))]
    fn worker_thread(
        request_rx: std::sync::mpsc::Receiver<TileRequest>,
        result_tx: std::sync::mpsc::Sender<TileLoadResult>,
        user_agent: String,
    ) {
        let client = reqwest::blocking::Client::builder()
            .user_agent(&user_agent)
            .build()
            .expect("Failed to create HTTP client");

        while let Ok(request) = request_rx.recv() {
            let result = match client.get(&request.url).send() {
                Ok(response) => {
                    if response.status().is_success() {
                        match response.bytes() {
                            Ok(bytes) => {
                                TileLoadResult::Success(request.tile_id, bytes.to_vec())
                            }
                            Err(e) => {
                                TileLoadResult::Failed(request.tile_id, e.to_string())
                            }
                        }
                    } else {
                        TileLoadResult::Failed(
                            request.tile_id,
                            format!("HTTP {}", response.status()),
                        )
                    }
                }
                Err(e) => TileLoadResult::Failed(request.tile_id, e.to_string()),
            };

            if result_tx.send(result).is_err() {
                break; // Receiver dropped, exit thread
            }
        }
    }

    // WASM implementation using web-sys fetch API
    #[cfg(target_arch = "wasm32")]
    fn spawn_wasm_fetch(&self, request: TileRequest) {
        use wasm_bindgen::prelude::*;
        use wasm_bindgen::JsCast;
        use wasm_bindgen_futures::JsFuture;
        use web_sys::{Request, RequestInit, RequestMode, Response};

        let result_buffer = self.result_rx.clone();
        let user_agent = self.user_agent.clone();

        wasm_bindgen_futures::spawn_local(async move {
            let result = async {
                // Create request with proper headers
                let mut opts = RequestInit::new();
                opts.method("GET");
                opts.mode(RequestMode::Cors);

                let web_request = Request::new_with_str_and_init(&request.url, &opts)
                    .map_err(|e| format!("Failed to create request: {:?}", e))?;

                // Set User-Agent header (note: may not work in all browsers due to CORS)
                web_request
                    .headers()
                    .set("User-Agent", &user_agent)
                    .map_err(|e| format!("Failed to set User-Agent: {:?}", e))?;

                // Fetch the tile
                let window = web_sys::window().ok_or("No window object")?;
                let resp_value = JsFuture::from(window.fetch_with_request(&web_request))
                    .await
                    .map_err(|e| format!("Fetch failed: {:?}", e))?;

                let resp: Response = resp_value
                    .dyn_into()
                    .map_err(|_| "Response is not a Response object")?;

                if !resp.ok() {
                    return Err(format!("HTTP {}", resp.status()));
                }

                // Get response as array buffer
                let array_buffer = JsFuture::from(
                    resp.array_buffer()
                        .map_err(|e| format!("Failed to get array buffer: {:?}", e))?,
                )
                .await
                .map_err(|e| format!("Failed to read array buffer: {:?}", e))?;

                // Convert to Vec<u8>
                let uint8_array = js_sys::Uint8Array::new(&array_buffer);
                let bytes = uint8_array.to_vec();

                Ok(bytes)
            }
            .await;

            // Store result in shared buffer
            let tile_result = match result {
                Ok(bytes) => TileLoadResult::Success(request.tile_id, bytes),
                Err(err) => TileLoadResult::Failed(request.tile_id, err),
            };

            if let Ok(mut results) = result_buffer.lock() {
                results.push(tile_result);
            }
        });
    }
}

impl Default for TileLoader {
    fn default() -> Self {
        Self::new("CPlace/0.1 (https://github.com/antegral/cplace)")
    }
}

/// Create GPU texture from image bytes
pub fn decode_tile_image(data: &[u8]) -> Result<image::RgbaImage, image::ImageError> {
    let img = image::load_from_memory(data)?;
    Ok(img.to_rgba8())
}

/// Calculate memory size for a tile texture
pub fn tile_memory_size(width: u32, height: u32) -> usize {
    (width * height * 4) as usize // RGBA8 = 4 bytes per pixel
}
