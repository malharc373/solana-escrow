use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{
        close_account, transfer_checked, CloseAccount, Mint, TokenAccount, TokenInterface,
        TransferChecked,
    },
};

use crate::{constants::ESCROW_SEED, error::EscrowError, state::Escrow};

#[derive(Accounts)]
pub struct Refund<'info> {
    #[account(mut)]
    pub maker: Signer<'info>,

    #[account(mint::token_program = token_program)]
    pub mint_a: Box<InterfaceAccount<'info, Mint>>,

    /// `init_if_needed` in case the maker closed their token account meanwhile.
    #[account(
        init_if_needed,
        payer = maker,
        associated_token::mint = mint_a,
        associated_token::authority = maker,
        associated_token::token_program = token_program,
    )]
    pub maker_ata_a: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        close = maker,
        has_one = maker @ EscrowError::InvalidMaker,
        has_one = mint_a @ EscrowError::InvalidMintA,
        seeds = [ESCROW_SEED, maker.key().as_ref(), &escrow.seed.to_le_bytes()],
        bump = escrow.bump,
    )]
    pub escrow: Account<'info, Escrow>,

    #[account(
        mut,
        associated_token::mint = mint_a,
        associated_token::authority = escrow,
        associated_token::token_program = token_program,
    )]
    pub vault: Box<InterfaceAccount<'info, TokenAccount>>,

    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

pub fn handle_refund(ctx: Context<Refund>) -> Result<()> {
    let accounts = &ctx.accounts;

    let maker_key = accounts.maker.key();
    let seed_bytes = accounts.escrow.seed.to_le_bytes();
    let signer_seeds: &[&[&[u8]]] = &[&[
        ESCROW_SEED,
        maker_key.as_ref(),
        &seed_bytes,
        &[accounts.escrow.bump],
    ]];

    transfer_checked(
        CpiContext::new_with_signer(
            accounts.token_program.key(),
            TransferChecked {
                from: accounts.vault.to_account_info(),
                mint: accounts.mint_a.to_account_info(),
                to: accounts.maker_ata_a.to_account_info(),
                authority: accounts.escrow.to_account_info(),
            },
            signer_seeds,
        ),
        accounts.vault.amount,
        accounts.mint_a.decimals,
    )?;

    close_account(CpiContext::new_with_signer(
        accounts.token_program.key(),
        CloseAccount {
            account: accounts.vault.to_account_info(),
            destination: accounts.maker.to_account_info(),
            authority: accounts.escrow.to_account_info(),
        },
        signer_seeds,
    ))?;

    msg!("Escrow {} refunded", accounts.escrow.seed);
    Ok(())
}
