use anchor_lang::prelude::*;

/// Escrow PDA seeds: [ESCROW_SEED, maker, seed.to_le_bytes()]
#[constant]
pub const ESCROW_SEED: &[u8] = b"escrow";
