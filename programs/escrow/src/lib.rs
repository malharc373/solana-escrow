//! Token escrow: a maker locks tokens of mint A in a vault owned by a PDA and
//! names a price in mint B. A taker pays the price and receives the vault in a
//! single atomic transaction, or the maker refunds and closes the offer.
//!
//! Concepts practiced:
//! - PDA as a token authority: the escrow PDA owns the vault ATA and signs
//!   CPIs with `CpiContext::new_with_signer`
//! - Token CPIs through `token_interface` (works with Token and Token-2022)
//! - `transfer_checked` (mint + decimals verified by the token program)
//! - `init_if_needed` ATAs, `has_one` relations, `close` on both the state
//!   account and the vault (token-account close via CPI)

pub mod constants;
pub mod error;
pub mod instructions;
pub mod state;

use anchor_lang::prelude::*;

pub use constants::*;
pub use instructions::*;
pub use state::*;

declare_id!("8emUGfskjPAGb2DaxtkgtcpSZhwfgoa3a6f8VipzSB5M");

#[program]
pub mod escrow {
    use super::*;

    /// Open an offer: deposit `deposit` of mint A, ask for `receive` of mint B.
    pub fn make(ctx: Context<Make>, seed: u64, deposit: u64, receive: u64) -> Result<()> {
        crate::instructions::make::handle_make(ctx, seed, deposit, receive)
    }

    /// Accept an offer: pay the maker in mint B, receive the whole vault.
    pub fn take(ctx: Context<Take>) -> Result<()> {
        crate::instructions::take::handle_take(ctx)
    }

    /// Cancel an offer: the maker gets the vault back and all rent is refunded.
    pub fn refund(ctx: Context<Refund>) -> Result<()> {
        crate::instructions::refund::handle_refund(ctx)
    }
}
