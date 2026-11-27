//! LiteSVM tests for the escrow. Run `anchor build` first (the tests load
//! target/deploy/escrow.so). Every scenario runs against both the classic
//! Token program and Token-2022, since the program uses `token_interface`.

use {
    anchor_lang::{
        prelude::Pubkey,
        solana_program::{instruction::Instruction, system_instruction, system_program},
        AccountDeserialize, InstructionData, ToAccountMetas,
    },
    anchor_spl::{
        associated_token::{
            self, get_associated_token_address_with_program_id,
            spl_associated_token_account::instruction::create_associated_token_account,
        },
        token::ID as TOKEN_PROGRAM_ID,
        token_2022::{spl_token_2022, ID as TOKEN_2022_PROGRAM_ID},
        token_interface::TokenAccount,
    },
    escrow::{accounts, instruction, Escrow, ESCROW_SEED},
    litesvm::LiteSVM,
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
};

const SOL: u64 = 1_000_000_000;
const DECIMALS: u8 = 6;
const MINT_LEN: usize = 82;
const TOKEN_PROGRAMS: [Pubkey; 2] = [TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID];

struct Env {
    svm: LiteSVM,
    token_program: Pubkey,
    maker: Keypair,
    taker: Keypair,
    mint_a: Pubkey,
    mint_b: Pubkey,
}

/// Signs and sends; returns the logs on success, the logs on failure.
fn send(
    svm: &mut LiteSVM,
    ixs: &[Instruction],
    payer: &Keypair,
    signers: &[&Keypair],
) -> Result<Vec<String>, String> {
    let msg = Message::new_with_blockhash(ixs, Some(&payer.pubkey()), &svm.latest_blockhash());
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), signers).unwrap();
    let res = svm
        .send_transaction(tx)
        .map(|meta| meta.logs)
        .map_err(|failed| failed.meta.logs.join("\n"));
    svm.expire_blockhash();
    res
}

fn ata(owner: &Pubkey, mint: &Pubkey, token_program: &Pubkey) -> Pubkey {
    get_associated_token_address_with_program_id(owner, mint, token_program)
}

fn balance(svm: &LiteSVM, token_account: &Pubkey) -> u64 {
    let acc = svm
        .get_account(token_account)
        .expect("token account exists");
    TokenAccount::try_deserialize(&mut acc.data.as_slice())
        .unwrap()
        .amount
}

fn is_closed(svm: &LiteSVM, address: &Pubkey) -> bool {
    svm.get_account(address)
        .is_none_or(|a| a.lamports == 0 && a.data.is_empty())
}

fn create_mint(
    svm: &mut LiteSVM,
    mint: &Keypair,
    payer: &Keypair,
    token_program: &Pubkey,
) -> Pubkey {
    let ixs = [
        system_instruction::create_account(
            &payer.pubkey(),
            &mint.pubkey(),
            svm.minimum_balance_for_rent_exemption(MINT_LEN),
            MINT_LEN as u64,
            token_program,
        ),
        spl_token_2022::instruction::initialize_mint2(
            token_program,
            &mint.pubkey(),
            &payer.pubkey(),
            None,
            DECIMALS,
        )
        .unwrap(),
    ];
    send(svm, &ixs, payer, &[payer, mint]).unwrap();
    mint.pubkey()
}

/// Creates `owner`'s ATA for `mint` and mints `amount` into it (`authority` = mint authority).
fn fund(
    svm: &mut LiteSVM,
    authority: &Keypair,
    owner: &Pubkey,
    mint: &Pubkey,
    token_program: &Pubkey,
    amount: u64,
) {
    let ixs = [
        create_associated_token_account(&authority.pubkey(), owner, mint, token_program),
        spl_token_2022::instruction::mint_to(
            token_program,
            mint,
            &ata(owner, mint, token_program),
            &authority.pubkey(),
            &[],
            amount,
        )
        .unwrap(),
    ];
    send(svm, &ixs, authority, &[authority]).unwrap();
}

/// Maker holds 1_000 A, taker holds 1_000 B.
fn setup(token_program: Pubkey) -> Env {
    let mut svm = LiteSVM::new();
    let bytes = include_bytes!(concat!(env!("CARGO_TARGET_TMPDIR"), "/../deploy/escrow.so"));
    svm.add_program(escrow::id(), bytes).unwrap();

    // Fixed keys: the same PDAs and bumps, so the same compute units, every run.
    let [maker, taker, mint_a, mint_b] = [1, 2, 3, 4].map(|n| Keypair::new_from_array([n; 32]));
    svm.airdrop(&maker.pubkey(), 10 * SOL).unwrap();
    svm.airdrop(&taker.pubkey(), 10 * SOL).unwrap();

    // The maker is mint authority for both mints in these tests.
    let mint_a = create_mint(&mut svm, &mint_a, &maker, &token_program);
    let mint_b = create_mint(&mut svm, &mint_b, &maker, &token_program);
    fund(
        &mut svm,
        &maker,
        &maker.pubkey(),
        &mint_a,
        &token_program,
        1_000,
    );
    fund(
        &mut svm,
        &maker,
        &taker.pubkey(),
        &mint_b,
        &token_program,
        1_000,
    );

    Env {
        svm,
        token_program,
        maker,
        taker,
        mint_a,
        mint_b,
    }
}

fn escrow_pda(maker: &Pubkey, seed: u64) -> Pubkey {
    Pubkey::find_program_address(
        &[ESCROW_SEED, maker.as_ref(), &seed.to_le_bytes()],
        &escrow::id(),
    )
    .0
}

fn ix_make(env: &Env, seed: u64, deposit: u64, receive: u64) -> Instruction {
    let maker = env.maker.pubkey();
    let escrow = escrow_pda(&maker, seed);
    Instruction::new_with_bytes(
        escrow::id(),
        &instruction::Make {
            seed,
            deposit,
            receive,
        }
        .data(),
        accounts::Make {
            maker,
            mint_a: env.mint_a,
            mint_b: env.mint_b,
            maker_ata_a: ata(&maker, &env.mint_a, &env.token_program),
            escrow,
            vault: ata(&escrow, &env.mint_a, &env.token_program),
            associated_token_program: associated_token::ID,
            token_program: env.token_program,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn ix_take(env: &Env, seed: u64, taker: &Pubkey, mint_b: &Pubkey) -> Instruction {
    let maker = env.maker.pubkey();
    let escrow = escrow_pda(&maker, seed);
    Instruction::new_with_bytes(
        escrow::id(),
        &instruction::Take {}.data(),
        accounts::Take {
            taker: *taker,
            maker,
            mint_a: env.mint_a,
            mint_b: *mint_b,
            taker_ata_a: ata(taker, &env.mint_a, &env.token_program),
            taker_ata_b: ata(taker, mint_b, &env.token_program),
            maker_ata_b: ata(&maker, mint_b, &env.token_program),
            escrow,
            vault: ata(&escrow, &env.mint_a, &env.token_program),
            associated_token_program: associated_token::ID,
            token_program: env.token_program,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

/// `maker` is whoever signs; the escrow PDA is always derived from `escrow_owner`.
fn ix_refund(env: &Env, seed: u64, signer: &Pubkey, escrow_owner: &Pubkey) -> Instruction {
    let escrow = escrow_pda(escrow_owner, seed);
    Instruction::new_with_bytes(
        escrow::id(),
        &instruction::Refund {}.data(),
        accounts::Refund {
            maker: *signer,
            mint_a: env.mint_a,
            maker_ata_a: ata(signer, &env.mint_a, &env.token_program),
            escrow,
            vault: ata(&escrow, &env.mint_a, &env.token_program),
            associated_token_program: associated_token::ID,
            token_program: env.token_program,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn make(env: &mut Env, seed: u64, deposit: u64, receive: u64) {
    let ix = ix_make(env, seed, deposit, receive);
    send(&mut env.svm, &[ix], &env.maker, &[&env.maker]).unwrap();
}

#[test]
fn make_locks_tokens_in_a_pda_owned_vault() {
    for tp in TOKEN_PROGRAMS {
        let mut env = setup(tp);
        let maker = env.maker.pubkey();
        make(&mut env, 1, 400, 250);

        let escrow = escrow_pda(&maker, 1);
        let vault = ata(&escrow, &env.mint_a, &tp);
        assert_eq!(balance(&env.svm, &vault), 400);
        assert_eq!(balance(&env.svm, &ata(&maker, &env.mint_a, &tp)), 600);

        let state =
            Escrow::try_deserialize(&mut env.svm.get_account(&escrow).unwrap().data.as_slice())
                .unwrap();
        assert_eq!((state.seed, state.maker, state.receive), (1, maker, 250));
        assert_eq!((state.mint_a, state.mint_b), (env.mint_a, env.mint_b));
    }
}

#[test]
fn take_swaps_atomically_and_closes_everything() {
    for tp in TOKEN_PROGRAMS {
        let mut env = setup(tp);
        let (maker, taker) = (env.maker.pubkey(), env.taker.pubkey());
        make(&mut env, 7, 400, 250);
        let escrow = escrow_pda(&maker, 7);
        let vault = ata(&escrow, &env.mint_a, &tp);
        let locked_rent =
            env.svm.get_balance(&escrow).unwrap() + env.svm.get_balance(&vault).unwrap();
        let maker_sol_before = env.svm.get_balance(&maker).unwrap();

        let ix = ix_take(&env, 7, &taker, &env.mint_b);
        send(&mut env.svm, &[ix], &env.taker, &[&env.taker]).unwrap();

        assert_eq!(
            balance(&env.svm, &ata(&taker, &env.mint_a, &tp)),
            400,
            "taker got the vault"
        );
        assert_eq!(
            balance(&env.svm, &ata(&taker, &env.mint_b, &tp)),
            750,
            "taker paid 250 B"
        );
        assert_eq!(
            balance(&env.svm, &ata(&maker, &env.mint_b, &tp)),
            250,
            "maker got 250 B"
        );
        assert!(is_closed(&env.svm, &escrow) && is_closed(&env.svm, &vault));
        // The taker paid the tx fee and the new ATAs; the maker gets back exactly the rent they locked.
        assert_eq!(
            env.svm.get_balance(&maker).unwrap(),
            maker_sol_before + locked_rent
        );
    }
}

#[test]
fn refund_returns_the_vault_and_closes_everything() {
    for tp in TOKEN_PROGRAMS {
        let mut env = setup(tp);
        let maker = env.maker.pubkey();
        make(&mut env, 3, 400, 250);

        let ix = ix_refund(&env, 3, &maker, &maker);
        send(&mut env.svm, &[ix], &env.maker, &[&env.maker]).unwrap();

        let escrow = escrow_pda(&maker, 3);
        assert_eq!(balance(&env.svm, &ata(&maker, &env.mint_a, &tp)), 1_000);
        assert!(
            is_closed(&env.svm, &escrow) && is_closed(&env.svm, &ata(&escrow, &env.mint_a, &tp))
        );
    }
}

#[test]
fn one_maker_can_run_several_escrows() {
    for tp in TOKEN_PROGRAMS {
        let mut env = setup(tp);
        let maker = env.maker.pubkey();
        make(&mut env, 1, 100, 10);
        make(&mut env, 2, 200, 20);
        assert_ne!(escrow_pda(&maker, 1), escrow_pda(&maker, 2));

        let ix = ix_take(&env, 2, &env.taker.pubkey(), &env.mint_b);
        send(&mut env.svm, &[ix], &env.taker, &[&env.taker]).unwrap();
        assert_eq!(
            balance(&env.svm, &ata(&escrow_pda(&maker, 1), &env.mint_a, &tp)),
            100,
            "escrow 1 untouched"
        );
    }
}

#[test]
fn only_the_maker_can_refund() {
    for tp in TOKEN_PROGRAMS {
        let mut env = setup(tp);
        let maker = env.maker.pubkey();
        make(&mut env, 5, 400, 250);

        // The taker tries to refund the maker's escrow to themselves. The PDA
        // seeds are re-derived from the signer, so the escrow address doesn't match.
        let attacker = env.taker.insecure_clone();
        let ix = ix_refund(&env, 5, &attacker.pubkey(), &maker);
        let logs = send(&mut env.svm, &[ix], &attacker, &[&attacker]).unwrap_err();
        assert!(logs.contains("ConstraintSeeds"), "unexpected logs:\n{logs}");
        assert_eq!(
            balance(&env.svm, &ata(&escrow_pda(&maker, 5), &env.mint_a, &tp)),
            400
        );
    }
}

#[test]
fn take_rejects_a_different_payment_mint() {
    for tp in TOKEN_PROGRAMS {
        let mut env = setup(tp);
        make(&mut env, 9, 400, 250);

        // The taker mints a worthless token and tries to pay with it.
        let taker = env.taker.insecure_clone();
        let fake_b = create_mint(&mut env.svm, &Keypair::new(), &taker, &tp);
        fund(&mut env.svm, &taker, &taker.pubkey(), &fake_b, &tp, 1_000);

        let ix = ix_take(&env, 9, &taker.pubkey(), &fake_b);
        let logs = send(&mut env.svm, &[ix], &taker, &[&taker]).unwrap_err();
        assert!(logs.contains("InvalidMintB"), "unexpected logs:\n{logs}");
        assert_eq!(
            balance(
                &env.svm,
                &ata(&escrow_pda(&env.maker.pubkey(), 9), &env.mint_a, &tp)
            ),
            400
        );
    }
}

#[test]
fn take_fails_atomically_if_the_taker_cannot_pay() {
    for tp in TOKEN_PROGRAMS {
        let mut env = setup(tp);
        make(&mut env, 4, 400, 5_000); // asks for more B than the taker holds

        let ix = ix_take(&env, 4, &env.taker.pubkey(), &env.mint_b);
        assert!(send(&mut env.svm, &[ix], &env.taker, &[&env.taker]).is_err());

        // Nothing moved: the vault is still full and the taker still has all their B.
        let escrow = escrow_pda(&env.maker.pubkey(), 4);
        assert_eq!(balance(&env.svm, &ata(&escrow, &env.mint_a, &tp)), 400);
        assert_eq!(
            balance(&env.svm, &ata(&env.taker.pubkey(), &env.mint_b, &tp)),
            1_000
        );
    }
}

#[test]
fn make_rejects_zero_amounts_and_same_mint() {
    for tp in TOKEN_PROGRAMS {
        let mut env = setup(tp);

        for (deposit, receive) in [(0, 10), (10, 0)] {
            let ix = ix_make(&env, 1, deposit, receive);
            let logs = send(&mut env.svm, &[ix], &env.maker, &[&env.maker]).unwrap_err();
            assert!(logs.contains("ZeroAmount"), "unexpected logs:\n{logs}");
        }

        env.mint_b = env.mint_a;
        let ix = ix_make(&env, 1, 10, 10);
        let logs = send(&mut env.svm, &[ix], &env.maker, &[&env.maker]).unwrap_err();
        assert!(logs.contains("SameMint"), "unexpected logs:\n{logs}");
    }
}

/// Sends one instruction that must succeed; returns the transaction's compute units.
fn compute_units(svm: &mut LiteSVM, ix: Instruction, signer: &Keypair) -> u64 {
    let msg = Message::new_with_blockhash(&[ix], Some(&signer.pubkey()), &svm.latest_blockhash());
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[signer]).unwrap();
    let meta = svm
        .send_transaction(tx)
        .unwrap_or_else(|e| panic!("{:?}\n{}", e.err, e.meta.logs.join("\n")));
    svm.expire_blockhash();
    meta.compute_units_consumed
}

#[test]
fn cu_report() {
    let mut report = cu_report::Report::new();
    for (tp, label) in TOKEN_PROGRAMS.into_iter().zip(["token", "token-2022"]) {
        let mut env = setup(tp);
        let (maker, taker) = (env.maker.insecure_clone(), env.taker.insecure_clone());
        let mint_b = env.mint_b;
        let steps = [
            ("make", ix_make(&env, 1, 400, 250), &maker),
            ("take", ix_take(&env, 1, &taker.pubkey(), &mint_b), &taker),
            ("make again", ix_make(&env, 2, 100, 50), &maker),
            (
                "refund",
                ix_refund(&env, 2, &maker.pubkey(), &maker.pubkey()),
                &maker,
            ),
        ];
        for (name, ix, signer) in steps {
            let cu = compute_units(&mut env.svm, ix, signer);
            report.record(format!("{name} ({label})"), cu);
        }
    }
    report.write(cu_report::default_path!()).unwrap();
}
