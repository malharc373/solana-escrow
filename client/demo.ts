// End-to-end devnet demo for the escrow program using @solana/kit.
//   npm run demo
// Creates two fresh test mints, funds a throwaway taker wallet, then runs
// make → take on escrow #1 and make → refund on escrow #2, printing explorer
// links and compute units for every escrow instruction.
//   npm run demo -- refund <mintA> <seed>   (close a leftover escrow)
// Env: RPC_URL (default devnet), KEYPAIR (maker; default ~/.config/solana/id.json)
// Confirmation polls over HTTP: the public devnet WebSocket drops connections.

import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import {
  AccountRole,
  address,
  appendTransactionMessageInstructions,
  createKeyPairSignerFromBytes,
  createSolanaRpc,
  createTransactionMessage,
  fetchEncodedAccount,
  generateKeyPairSigner,
  getBase64EncodedWireTransaction,
  getAddressEncoder,
  getProgramDerivedAddress,
  getSignatureFromTransaction,
  getU64Encoder,
  pipe,
  setTransactionMessageFeePayerSigner,
  setTransactionMessageLifetimeUsingBlockhash,
  signTransactionMessageWithSigners,
  type Address,
  type Instruction,
  type KeyPairSigner,
} from "@solana/kit";
import { getCreateAccountInstruction, getTransferSolInstruction, SYSTEM_PROGRAM_ADDRESS } from "@solana-program/system";
import {
  ASSOCIATED_TOKEN_PROGRAM_ADDRESS,
  fetchToken,
  findAssociatedTokenPda,
  getCreateAssociatedTokenIdempotentInstruction,
  getInitializeMint2Instruction,
  getMintSize,
  getMintToInstruction,
  TOKEN_PROGRAM_ADDRESS,
} from "@solana-program/token";

const PROGRAM_ID = address("8emUGfskjPAGb2DaxtkgtcpSZhwfgoa3a6f8VipzSB5M");
const RPC_URL = process.env.RPC_URL ?? "https://api.devnet.solana.com";
const CLUSTER = RPC_URL.includes("devnet") ? "?cluster=devnet" : "";
const DECIMALS = 6;

// Anchor discriminators from target/idl/escrow.json
const DISC = {
  make: [138, 227, 232, 77, 223, 166, 96, 197],
  take: [149, 226, 52, 104, 6, 142, 230, 39],
  refund: [2, 96, 183, 251, 63, 208, 46, 46],
};

const rpc = createSolanaRpc(RPC_URL);
const u64 = getU64Encoder();
const addr = getAddressEncoder();

async function loadMaker(): Promise<KeyPairSigner> {
  const path = process.env.KEYPAIR ?? `${homedir()}/.config/solana/id.json`;
  return createKeyPairSignerFromBytes(new Uint8Array(JSON.parse(readFileSync(path, "utf8"))));
}

/** Sends instructions with `feePayer`; returns [signature, compute units used]. */
async function send(feePayer: KeyPairSigner, instructions: Instruction[]): Promise<[string, bigint]> {
  const { value: blockhash } = await retry(() => rpc.getLatestBlockhash().send());
  const message = pipe(
    createTransactionMessage({ version: 0 }),
    (m) => setTransactionMessageFeePayerSigner(feePayer, m),
    (m) => setTransactionMessageLifetimeUsingBlockhash(blockhash, m),
    (m) => appendTransactionMessageInstructions(instructions, m),
  );
  const tx = await signTransactionMessageWithSigners(message);
  const sig = getSignatureFromTransaction(tx);
  await retry(() => rpc.sendTransaction(getBase64EncodedWireTransaction(tx), { encoding: "base64" }).send());
  await waitForConfirmation(sig);
  const info = await retry(() =>
    rpc.getTransaction(sig, { commitment: "confirmed", encoding: "json", maxSupportedTransactionVersion: 0 }).send(),
  );
  return [sig, info?.meta?.computeUnitsConsumed ?? 0n];
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** Retries an RPC call when the public endpoint rate-limits us (HTTP 429). */
async function retry<T>(call: () => Promise<T>, attempts = 6): Promise<T> {
  for (let i = 1; ; i++) {
    try {
      return await call();
    } catch (e) {
      const status = (e as { context?: { statusCode?: number } }).context?.statusCode;
      if (status !== 429 || i >= attempts) throw e;
      await sleep(2000 * i);
    }
  }
}

async function waitForConfirmation(sig: ReturnType<typeof getSignatureFromTransaction>) {
  for (let i = 0; i < 60; i++) {
    const { value: [status] } = await retry(() => rpc.getSignatureStatuses([sig]).send());
    if (status?.err) throw new Error(`transaction ${sig} failed: ${JSON.stringify(status.err, (_, v) => (typeof v === "bigint" ? v.toString() : v))}`);
    if (status?.confirmationStatus === "confirmed" || status?.confirmationStatus === "finalized") return;
    await sleep(2000);
  }
  throw new Error(`transaction ${sig} not confirmed after 120s`);
}

const ata = async (owner: Address, mint: Address) =>
  (await findAssociatedTokenPda({ owner, mint, tokenProgram: TOKEN_PROGRAM_ADDRESS }))[0];

async function escrowPda(maker: Address, seed: bigint): Promise<Address> {
  const [pda] = await getProgramDerivedAddress({
    programAddress: PROGRAM_ID,
    seeds: ["escrow", addr.encode(maker), u64.encode(seed)],
  });
  return pda;
}

async function createMint(payer: KeyPairSigner): Promise<Address> {
  const mint = await generateKeyPairSigner();
  const space = BigInt(getMintSize());
  const lamports = await retry(() => rpc.getMinimumBalanceForRentExemption(space).send());
  await send(payer, [
    getCreateAccountInstruction({ payer, newAccount: mint, lamports, space, programAddress: TOKEN_PROGRAM_ADDRESS }),
    getInitializeMint2Instruction({ mint: mint.address, decimals: DECIMALS, mintAuthority: payer.address }),
  ]);
  return mint.address;
}

async function fund(authority: KeyPairSigner, owner: Address, mint: Address, amount: bigint) {
  const token = await ata(owner, mint);
  await send(authority, [
    getCreateAssociatedTokenIdempotentInstruction({ payer: authority, ata: token, owner, mint }),
    getMintToInstruction({ mint, token, mintAuthority: authority, amount }),
  ]);
}

const programs = [
  { address: ASSOCIATED_TOKEN_PROGRAM_ADDRESS, role: AccountRole.READONLY },
  { address: TOKEN_PROGRAM_ADDRESS, role: AccountRole.READONLY },
  { address: SYSTEM_PROGRAM_ADDRESS, role: AccountRole.READONLY },
];

async function ixMake(maker: KeyPairSigner, mintA: Address, mintB: Address, seed: bigint, deposit: bigint, receive: bigint) {
  const escrow = await escrowPda(maker.address, seed);
  return {
    programAddress: PROGRAM_ID,
    data: new Uint8Array([...DISC.make, ...u64.encode(seed), ...u64.encode(deposit), ...u64.encode(receive)]),
    accounts: [
      { address: maker.address, role: AccountRole.WRITABLE_SIGNER, signer: maker },
      { address: mintA, role: AccountRole.READONLY },
      { address: mintB, role: AccountRole.READONLY },
      { address: await ata(maker.address, mintA), role: AccountRole.WRITABLE },
      { address: escrow, role: AccountRole.WRITABLE },
      { address: await ata(escrow, mintA), role: AccountRole.WRITABLE },
      ...programs,
    ],
  };
}

async function ixTake(taker: KeyPairSigner, maker: Address, mintA: Address, mintB: Address, seed: bigint) {
  const escrow = await escrowPda(maker, seed);
  return {
    programAddress: PROGRAM_ID,
    data: new Uint8Array(DISC.take),
    accounts: [
      { address: taker.address, role: AccountRole.WRITABLE_SIGNER, signer: taker },
      { address: maker, role: AccountRole.WRITABLE },
      { address: mintA, role: AccountRole.READONLY },
      { address: mintB, role: AccountRole.READONLY },
      { address: await ata(taker.address, mintA), role: AccountRole.WRITABLE },
      { address: await ata(taker.address, mintB), role: AccountRole.WRITABLE },
      { address: await ata(maker, mintB), role: AccountRole.WRITABLE },
      { address: escrow, role: AccountRole.WRITABLE },
      { address: await ata(escrow, mintA), role: AccountRole.WRITABLE },
      ...programs,
    ],
  };
}

async function ixRefund(maker: KeyPairSigner, mintA: Address, seed: bigint) {
  const escrow = await escrowPda(maker.address, seed);
  return {
    programAddress: PROGRAM_ID,
    data: new Uint8Array(DISC.refund),
    accounts: [
      { address: maker.address, role: AccountRole.WRITABLE_SIGNER, signer: maker },
      { address: mintA, role: AccountRole.READONLY },
      { address: await ata(maker.address, mintA), role: AccountRole.WRITABLE },
      { address: escrow, role: AccountRole.WRITABLE },
      { address: await ata(escrow, mintA), role: AccountRole.WRITABLE },
      ...programs,
    ],
  };
}

const balance = async (owner: Address, mint: Address) => (await fetchToken(rpc, await ata(owner, mint))).data.amount;
const log = (label: string, [sig, cu]: [string, bigint]) =>
  console.log(`  ${label.padEnd(8)} ${String(cu).padStart(6)} CU  https://explorer.solana.com/tx/${sig}${CLUSTER}`);

// ---------------------------------------------------------------------------
const maker = await loadMaker();

if (process.argv[2] === "refund") {
  const [mintArg, seedArg] = process.argv.slice(3);
  if (!mintArg || !seedArg) throw new Error("usage: npm run demo -- refund <mintA> <seed>");
  log("refund", await send(maker, [await ixRefund(maker, address(mintArg), BigInt(seedArg))]));
  process.exit(0);
}

// Seeds only need to be unique per maker; time-based ones let the demo rerun.
const seed1 = BigInt(Date.now());
const seed2 = seed1 + 1n;
const taker = await generateKeyPairSigner();
console.log(`maker ${maker.address}\ntaker ${taker.address} (fresh, funded with 0.05 SOL)`);
await send(maker, [getTransferSolInstruction({ source: maker, destination: taker.address, amount: 50_000_000n })]);

const mintA = await createMint(maker);
const mintB = await createMint(maker);
console.log(`mint A ${mintA}\nmint B ${mintB}`);
await fund(maker, maker.address, mintA, 1_000n);
await fund(maker, taker.address, mintB, 1_000n);

console.log(`\nEscrow #1 (seed ${seed1}): make 400 A for 250 B, then take`);
log("make", await send(maker, [await ixMake(maker, mintA, mintB, seed1, 400n, 250n)]));
console.log(`  escrow ${await escrowPda(maker.address, seed1)}`);
log("take", await send(taker, [await ixTake(taker, maker.address, mintA, mintB, seed1)]));

console.log(`\nEscrow #2 (seed ${seed2}): make 100 A for 50 B, then refund`);
log("make", await send(maker, [await ixMake(maker, mintA, mintB, seed2, 100n, 50n)]));
log("refund", await send(maker, [await ixRefund(maker, mintA, seed2)]));

const closed = async (a: Address) => !(await fetchEncodedAccount(rpc, a)).exists;
console.log("\nFinal balances");
console.log(`  maker  A ${await balance(maker.address, mintA)}  B ${await balance(maker.address, mintB)}`);
console.log(`  taker  A ${await balance(taker.address, mintA)}  B ${await balance(taker.address, mintB)}`);
console.log(`  escrow #1 closed: ${await closed(await escrowPda(maker.address, seed1))}, #2 closed: ${await closed(await escrowPda(maker.address, seed2))}`);
