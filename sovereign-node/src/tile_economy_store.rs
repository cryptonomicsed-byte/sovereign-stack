//! In-memory tile economy state — updated on each Àṣẹ mint.
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use twin_protocol::ase::{TileEconomy, AseMintResult, update_tile_economy, DEFAULT_USAGE_FEE_PCT};

#[derive(Clone, Default)]
pub struct TileEconomyStore(Arc<RwLock<HashMap<String, TileEconomy>>>);

impl TileEconomyStore {
    pub fn new() -> Self { Self::default() }

    pub async fn apply_mint(&self, tile_id: &str, result: &AseMintResult) {
        let mut map = self.0.write().await;
        let economy = map.entry(tile_id.to_string()).or_insert_with(|| TileEconomy {
            tile_id:       tile_id.to_string(),
            usage_fee_pct: DEFAULT_USAGE_FEE_PCT,
            ..Default::default()
        });
        update_tile_economy(economy, result);
    }

    pub async fn get(&self, tile_id: &str) -> Option<TileEconomy> {
        self.0.read().await.get(tile_id).cloned()
    }

    pub async fn list(&self) -> Vec<TileEconomy> {
        self.0.read().await.values().cloned().collect()
    }

    /// Set the owner_did for a tile, creating the economy entry if it doesn't exist.
    pub async fn claim(&self, tile_id: &str, owner_did: &str) {
        let mut map = self.0.write().await;
        let economy = map.entry(tile_id.to_string()).or_insert_with(|| TileEconomy {
            tile_id:       tile_id.to_string(),
            usage_fee_pct: DEFAULT_USAGE_FEE_PCT,
            ..Default::default()
        });
        economy.owner_did = Some(owner_did.to_string());
    }
}
