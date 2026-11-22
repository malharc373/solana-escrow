# 02-escrow

A trustless token swap on Solana. A **maker** locks tokens of mint A in a vault that only the program controls, and names a price in mint B. A **taker** pays the price and receives the whole vault in one atomic transaction. Or the maker cancels and gets everything back, rent included.

Built with **Anchor 1.1.2** and **anchor-spl** (`token_interface`, so it works with both **Token** and **Token-2022**), tested with **LiteSVM**, and exercised on devnet with a **@solana/kit** client.

| | |
|---|---|
| Program ID (devnet) | [`8emUGfskjPAGb2DaxtkgtcpSZhwfgoa3a6f8VipzSB5M`](https://explorer.solana.com/address/8emUGfskjPAGb2DaxtkgtcpSZhwfgoa3a6f8VipzSB5M?cluster=devnet) |
| Example `make` | [`2rFJ1c…prx9b`](https://explorer.solana.com/tx/2rFJ1cNDiAnymFa6Pb9gFk7VDdR68sJdaLQWXXTQ7FDYMcAxn14FmSZ6T3WvSKRJswXVzQGjufr1eyoPkUdprx9b?cluster=devnet) |
| Example `take` | [`4NXpGk…qhoJcQ`](https://explorer.solana.com/tx/4NXpGk8zMLC1PJgoJ1cpzeHauxMHJKa5qVaNRB8Nj16QzVgohx8z7F7jA9Vr1qss49zQhoJcQ9UxLt9D1WcJqhy7?cluster=devnet) |
| Example `refund` | [`5SrfB3…YZfN`](https://explorer.solana.com/tx/5SrfB35VpgQH21cwhw4reyMcPoVgoBH7AzfGYRML7tymgp314nQg1KUJsQQHDyp9bmMkeab4V2Zn8iy1fRM9YZfN?cluster=devnet) |

## How it works

```
make(seed, deposit, receive)
  maker ──deposit A──▶ vault (ATA owned by the escrow PDA)
  creates Escrow { seed, maker, mint_a, mint_b, receive, bump }
          at PDA ["escrow", maker, seed_le]

take()                                         ┐
  taker ──receive B──▶ maker                   │ one transaction:
  vault ──all A──────▶ taker  (PDA signs)      │ all or nothing
  close vault + escrow → rent to maker         ┘

refund()
  vault ──all A──▶ maker  (PDA signs), close vault + escrow → rent to maker
```

- **The vault is owned by a PDA.** No private key exists for it, so the tokens can only move when the program signs with `CpiContext::new_with_signer(["escrow", maker, seed, bump])`.
- **`seed`** lets one maker keep several offers open at once.
- **`transfer_checked`** makes the token program verify the mint and decimals on every transfer.
- **`init_if_needed`** creates the taker's mint-A account and the maker's mint-B account inside `take`; the taker pays for them.
- Large account structs are `Box`ed so the handler fits in the 4 KB SBF stack frame.

## Instructions

| Instruction | Signer | Compute units on devnet* |
|---|---|---|
| `make(seed, deposit, receive)` | maker | 39,609 – 45,600 |
| `take()` | taker | 70,255 – 73,158 |
| `refund()` | maker | 22,217 – 22,314 |

\* The whole transaction, including the token-program CPIs. `make` varies with how many bump iterations the PDA search needs for that seed. `take` includes creating up to two associated token accounts.

## Security properties (each one is tested on Token and Token-2022)

| Property | Enforced by | Test |
|---|---|---|
| Only the maker can refund | PDA seeds re-derived from the signer → `ConstraintSeeds` | `only_the_maker_can_refund` |
| The taker must pay with the real mint B | `has_one = mint_b` → `InvalidMintB` | `take_rejects_a_different_payment_mint` |
| A swap is all-or-nothing | a single transaction; any failed CPI reverts everything | `take_fails_atomically_if_the_taker_cannot_pay` |
| No empty or self-swaps | `ZeroAmount`, `SameMint` | `make_rejects_zero_amounts_and_same_mint` |
| Escrows don't interfere | `seed` in the PDA | `one_maker_can_run_several_escrows` |
| No leftover accounts or rent | `close = maker` + token `close_account` CPI | `take_swaps_atomically_and_closes_everything`, `refund_returns_the_vault_and_closes_everything` |

**Mutation check:** deleting the `has_one = mint_b` line makes `take_rejects_a_different_payment_mint` fail, because the attacker's worthless token buys the vault. That check is what prevents theft.

## Run it

```bash
anchor build
```

```bash
cargo test
```

Eight tests, each run against both token programs (16 scenarios, about 0.35 s).

Devnet demo: creates two test mints and a fresh taker wallet, then runs make → take and make → refund.

```bash
cd client && npm install && npm run demo
```

Close an escrow you left open:

```bash
npm run demo -- refund <mintA> <seed>
```

The client confirms transactions by polling over HTTP and backs off on HTTP 429, because the public devnet WebSocket and rate limits are unreliable.

## What I learned
- **A PDA can be a token authority.** The signer seeds are the account's own seeds plus its bump, and the runtime checks the program really derived them.
- **Anchor 1.x changed `CpiContext::new`.** It now takes the **program ID** (`Pubkey`), not an `AccountInfo`, so older tutorials are out of date.
- **`has_one` on the mints is a security check, not bookkeeping.** Without it, anyone can "pay" with a token they just minted.
- **`token_interface` + `InterfaceAccount`** let one program serve both Token and Token-2022. The Token-2022 instruction builders also accept the classic Token program ID, so one test helper covers both.
- **Atomicity is free.** Putting the payment and the release in one instruction means there's no state where one side has paid and the other hasn't.
- **Devnet tooling is flaky.** A transaction can land even when the client loses its confirmation, so check the chain before you retry.
