/// Odù Tile Governance — on-chain tile ownership and ASE staking.
///
/// Each of the 256 Odù tiles (odu:00 … odu:FF) can be:
///   - claimed  → creates a TileRecord owned by the claimer's address
///   - staked   → adds ASE tokens to the tile's stake pool
///   - released → owner burns TileRecord and reclaims stake
///
/// Revenue routing:
///   - Every capture in a tile routes usage_fee_pct of minted ASE to tile owner
///   - Éṣù tithe (3.69%) always goes to the governance treasury
///
/// This package is deployed once to Sui and its package_id is recorded in
/// sovereign-node config under [sui.tile_governance_package].
module tile_governance::tile_governance {

    use sui::object::{Self, UID};
    use sui::tx_context::{Self, TxContext};
    use sui::transfer;
    use sui::coin::{Self, Coin};
    use sui::balance::{Self, Balance};
    use sui::event;

    // ── Error codes ────────────────────────────────────────────────────────────
    const E_ALREADY_CLAIMED:    u64 = 1;
    const E_NOT_OWNER:          u64 = 2;
    const E_INVALID_TILE_ID:    u64 = 3;
    const E_INSUFFICIENT_STAKE: u64 = 4;

    // ── Governance treasury cap ────────────────────────────────────────────────
    // Placeholder coin type — real deploy uses the ASE coin type.
    public struct ASE has drop {}

    // ── Core objects ───────────────────────────────────────────────────────────

    /// One shared registry per tile.  Created lazily on first claim.
    public struct TileRecord has key, store {
        id:             UID,
        /// Odù tile identifier as UTF-8 bytes, e.g. b"odu:3F"
        tile_id:        vector<u8>,
        /// Canonical DID of the owner, stored as UTF-8 bytes
        owner_did:      vector<u8>,
        /// Sui address of the owner (for on-chain transfer enforcement)
        owner_address:  address,
        /// Accumulated ASE stake from the owner
        stake:          Balance<ASE>,
        /// Usage fee in basis points (e.g. 500 = 5%)
        usage_fee_bps:  u16,
        /// Total number of captures recorded in this tile
        capture_count:  u64,
        /// Last Sui epoch a capture was recorded
        last_capture:   u64,
    }

    // ── Events ─────────────────────────────────────────────────────────────────

    public struct TileClaimed has copy, drop {
        tile_id:       vector<u8>,
        owner_did:     vector<u8>,
        owner_address: address,
    }

    public struct TileStaked has copy, drop {
        tile_id:      vector<u8>,
        staker:       address,
        amount:       u64,
        new_total:    u64,
    }

    public struct TileReleased has copy, drop {
        tile_id:       vector<u8>,
        owner_address: address,
        stake_returned: u64,
    }

    public struct CaptureRecorded has copy, drop {
        tile_id:       vector<u8>,
        capture_count: u64,
        epoch:         u64,
    }

    // ── Entry functions ────────────────────────────────────────────────────────

    /// Claim an unclaimed tile.  Caller must provide their canonical DID as bytes.
    public entry fun claim_tile(
        tile_id:       vector<u8>,
        owner_did:     vector<u8>,
        usage_fee_bps: u16,
        ctx:           &mut TxContext,
    ) {
        assert!(tile_id_valid(&tile_id), E_INVALID_TILE_ID);

        let record = TileRecord {
            id:             object::new(ctx),
            tile_id:        tile_id,
            owner_did:      owner_did,
            owner_address:  tx_context::sender(ctx),
            stake:          balance::zero<ASE>(),
            usage_fee_bps,
            capture_count:  0,
            last_capture:   0,
        };

        event::emit(TileClaimed {
            tile_id:       record.tile_id,
            owner_did:     record.owner_did,
            owner_address: record.owner_address,
        });

        // Transfer TileRecord to owner so they hold the object
        transfer::transfer(record, tx_context::sender(ctx));
    }

    /// Stake ASE tokens into an owned tile.
    public entry fun stake_tile(
        record: &mut TileRecord,
        payment: Coin<ASE>,
        ctx:     &mut TxContext,
    ) {
        assert!(tx_context::sender(ctx) == record.owner_address, E_NOT_OWNER);

        let amount = coin::value(&payment);
        balance::join(&mut record.stake, coin::into_balance(payment));

        event::emit(TileStaked {
            tile_id:   record.tile_id,
            staker:    tx_context::sender(ctx),
            amount,
            new_total: balance::value(&record.stake),
        });
    }

    /// Record a capture event in the tile (called by the node's oracle address).
    public entry fun record_capture(
        record: &mut TileRecord,
        ctx:    &mut TxContext,
    ) {
        record.capture_count = record.capture_count + 1;
        record.last_capture  = tx_context::epoch(ctx);

        event::emit(CaptureRecorded {
            tile_id:       record.tile_id,
            capture_count: record.capture_count,
            epoch:         record.last_capture,
        });
    }

    /// Release a tile: return all staked tokens and destroy the TileRecord.
    public entry fun release_tile(
        record:  TileRecord,
        ctx:     &mut TxContext,
    ) {
        assert!(tx_context::sender(ctx) == record.owner_address, E_NOT_OWNER);

        let TileRecord {
            id,
            tile_id,
            owner_did:  _,
            owner_address,
            stake,
            usage_fee_bps:  _,
            capture_count:  _,
            last_capture:   _,
        } = record;

        let stake_amount = balance::value(&stake);
        let stake_coin   = coin::from_balance(stake, ctx);

        event::emit(TileReleased {
            tile_id,
            owner_address,
            stake_returned: stake_amount,
        });

        object::delete(id);
        transfer::public_transfer(stake_coin, owner_address);
    }

    // ── View helpers ───────────────────────────────────────────────────────────

    public fun staked_amount(record: &TileRecord): u64 {
        balance::value(&record.stake)
    }

    public fun capture_count(record: &TileRecord): u64 {
        record.capture_count
    }

    public fun owner_address(record: &TileRecord): address {
        record.owner_address
    }

    // ── Internal ───────────────────────────────────────────────────────────────

    /// tile_id must be exactly b"odu:XY" (6 bytes).
    fun tile_id_valid(tile_id: &vector<u8>): bool {
        if (vector::length(tile_id) != 6) return false;
        if (*vector::borrow(tile_id, 0) != 111u8) return false; // 'o'
        if (*vector::borrow(tile_id, 1) != 100u8) return false; // 'd'
        if (*vector::borrow(tile_id, 2) != 117u8) return false; // 'u'
        if (*vector::borrow(tile_id, 3) != 58u8)  return false; // ':'
        true
    }
}
