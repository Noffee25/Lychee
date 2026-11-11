//! Front covers from the Cover Art Archive (coverartarchive.org).

use crate::{Client, Error, Result};

/// Largest download we accept (some uploads are huge scans).
const MAX_BYTES: u64 = 25 * 1024 * 1024;

impl Client {
    /// The front cover of a release, falling back to its release group. `size` is the
    /// thumbnail edge in pixels (250, 500 or 1200); the original is tried last.
    pub fn front_cover(&self, release_id: &str, release_group_id: &str, size: u32) -> Result<Option<Vec<u8>>> {
        let mut urls = Vec::new();
        for (kind, id) in [("release", release_id), ("release-group", release_group_id)] {
            if id.is_empty() {
                continue;
            }
            urls.push(format!("https://coverartarchive.org/{kind}/{id}/front-{size}"));
            urls.push(format!("https://coverartarchive.org/{kind}/{id}/front"));
        }
        for url in urls {
            let Some(mut response) = self.get(&url, &[], "image/*")? else { continue };
            let bytes = response
                .body_mut()
                .with_config()
                .limit(MAX_BYTES)
                .read_to_vec()
                .map_err(|e| Error::Network(e.to_string()))?;
            if !bytes.is_empty() {
                return Ok(Some(bytes));
            }
        }
        Ok(None)
    }
}
