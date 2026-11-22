use anchor_lang::prelude::*;

#[error_code]
pub enum EscrowError {
    #[msg("Deposit and receive amounts must be greater than zero")]
    ZeroAmount,
    #[msg("The offered and requested mints must be different")]
    SameMint,
    #[msg("Maker does not match the escrow")]
    InvalidMaker,
    #[msg("Mint A does not match the escrow")]
    InvalidMintA,
    #[msg("Mint B does not match the escrow")]
    InvalidMintB,
}
