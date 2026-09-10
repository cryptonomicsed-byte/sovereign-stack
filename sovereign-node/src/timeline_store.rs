//! Disk-backed store for TwinTimelines.
//!
//! Persists to: {data_dir}/timelines/{timeline_id_safe}.json
//! One file per timeline (device or tile).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use serde_json;
use tokio::sync::RwLock;
use tracing::{info, warn};

use twin_protocol::{TwinTimeline, TwinTimelineEntry};

#[derive(Clone)]
pub struct TimelineStore {
    dir:   PathBuf,
    cache: Arc<RwLock<HashMap<String, TwinTimeline>>>,
}

impl TimelineStore {
    pub async fn open(data_dir: &Path) -> Self {
        let dir = data_dir.join("timelines");
        if let Err(e) = tokio::fs::create_dir_all(&dir).await {
            warn!(dir = %dir.display(), error = %e, "could not create timelines dir");
        }
        let cache = load_all(&dir).await;
        info!(dir = %dir.display(), count = cache.len(), "timeline store loaded");
        Self { dir, cache: Arc::new(RwLock::new(cache)) }
    }

    pub fn in_memory() -> Self {
        Self {
            dir:   PathBuf::from("/dev/null"),
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Append an entry to the named timeline (creating it if needed).
    pub async fn append(&self, timeline_id: &str, entry: TwinTimelineEntry) {
        let mut cache = self.cache.write().await;
        let tl = cache.entry(timeline_id.to_string())
            .or_insert_with(|| TwinTimeline::new(timeline_id));
        tl.append(entry);
        let tl_clone = tl.clone();
        drop(cache);
        self.persist(&tl_clone).await;
    }

    pub async fn get(&self, timeline_id: &str) -> Option<TwinTimeline> {
        self.cache.read().await.get(timeline_id).cloned()
    }

    pub async fn all(&self) -> Vec<TwinTimeline> {
        self.cache.read().await.values().cloned().collect()
    }

    async fn persist(&self, tl: &TwinTimeline) {
        let safe = sanitize(&tl.timeline_id);
        let path = self.dir.join(format!("{safe}.json"));
        let json = match serde_json::to_string_pretty(tl) {
            Ok(j)  => j,
            Err(e) => { warn!(error = %e, "timeline serialize failed"); return; }
        };
        if let Err(e) = tokio::fs::write(&path, &json).await {
            warn!(path = %path.display(), error = %e, "timeline write failed");
        } else {
            info!(timeline_id = %tl.timeline_id, version = tl.version, "timeline persisted");
        }
    }
}

async fn load_all(dir: &Path) -> HashMap<String, TwinTimeline> {
    let mut map = HashMap::new();
    let mut rd = match tokio::fs::read_dir(dir).await {
        Ok(r) => r,
        Err(_) => return map,
    };
    while let Ok(Some(entry)) = rd.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") { continue; }
        match tokio::fs::read_to_string(&path).await {
            Err(e) => warn!(path = %path.display(), error = %e, "could not read timeline"),
            Ok(text) => match serde_json::from_str::<TwinTimeline>(&text) {
                Err(e) => warn!(path = %path.display(), error = %e, "could not parse timeline"),
                Ok(tl) => { map.insert(tl.timeline_id.clone(), tl); }
            }
        }
    }
    map
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}
