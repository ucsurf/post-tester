//! Storage for large payloads (bodies, frames) referenced from events.
//!
//! The live stream only carries short previews. When a user clicks "show
//! more", the full payload is fetched from here by its [`BlobId`].

use super::event::{BlobId, Payload};
use bytes::Bytes;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

/// Length of the text preview embedded in events.
pub const PREVIEW_BYTES: usize = 2 * 1024;

/// A stored payload.
#[derive(Debug, Clone)]
pub struct Blob {
    pub content_type: Option<String>,
    pub data: Bytes,
}

/// An in-memory store with a total size limit. The oldest blobs are evicted first.
#[derive(Debug)]
pub struct BlobStore {
    capacity: usize,
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    blobs: HashMap<BlobId, Blob>,
    order: VecDeque<BlobId>,
    used: usize,
}

impl BlobStore {
    pub fn new(capacity_bytes: usize) -> Self {
        Self { capacity: capacity_bytes, state: Mutex::new(State::default()) }
    }

    /// Stores data and returns its id. Data larger than the whole store is not kept.
    pub fn put(&self, data: Bytes, content_type: Option<String>) -> Option<BlobId> {
        if data.len() > self.capacity {
            return None;
        }
        let id = BlobId::new();
        let mut state = self.state.lock().expect("blob store lock poisoned");
        while state.used + data.len() > self.capacity {
            let Some(oldest) = state.order.pop_front() else { break };
            if let Some(evicted) = state.blobs.remove(&oldest) {
                state.used -= evicted.data.len();
            }
        }
        state.used += data.len();
        state.order.push_back(id);
        state.blobs.insert(id, Blob { content_type, data });
        Some(id)
    }

    pub fn get(&self, id: BlobId) -> Option<Blob> {
        self.state.lock().expect("blob store lock poisoned").blobs.get(&id).cloned()
    }

    /// Builds a [`Payload`]: a preview for the live stream, plus the full data
    /// stored as a blob when it does not fit in the preview.
    pub fn payload(&self, data: &Bytes, content_type: Option<String>) -> Payload {
        let truncated = data.len() > PREVIEW_BYTES;
        let preview = preview_text(data);
        let blob = if data.is_empty() { None } else { self.put(data.clone(), content_type.clone()) };
        Payload { preview, size: data.len() as u64, truncated, blob, content_type }
    }
}

/// The first [`PREVIEW_BYTES`] of `data` as text, never splitting a UTF-8 character.
pub fn preview_text(data: &[u8]) -> String {
    if data.len() <= PREVIEW_BYTES {
        return String::from_utf8_lossy(data).into_owned();
    }
    let mut end = PREVIEW_BYTES;
    // Step back over UTF-8 continuation bytes (0b10xxxxxx).
    while end > 0 && (data[end] & 0b1100_0000) == 0b1000_0000 {
        end -= 1;
    }
    String::from_utf8_lossy(&data[..end]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evicts_oldest_when_full() {
        let store = BlobStore::new(10);
        let a = store.put(Bytes::from_static(b"123456"), None).unwrap();
        let b = store.put(Bytes::from_static(b"7890ab"), None).unwrap();
        assert!(store.get(a).is_none(), "oldest blob should be evicted");
        assert!(store.get(b).is_some());
    }

    #[test]
    fn preview_never_splits_characters() {
        let text = "é".repeat(PREVIEW_BYTES); // 2 bytes each
        let preview = preview_text(text.as_bytes());
        assert!(!preview.contains('\u{FFFD}'));
        assert!(preview.len() <= PREVIEW_BYTES);
    }
}
