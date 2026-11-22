use anchor_lang::prelude::*;

/// One open offer: the maker locked `mint_a` tokens in the vault and wants
/// `receive` tokens of `mint_b` in return.
#[account]
#[derive(InitSpace)]
pub struct Escrow {
    /// Lets one maker run several escrows at once (part of the PDA seeds).
    pub seed: u64,
    pub maker: Pubkey,
    pub mint_a: Pubkey,
    pub mint_b: Pubkey,
    /// Amount of mint B the maker wants for the whole vault.
    pub receive: u64,
    pub bump: u8,
}
