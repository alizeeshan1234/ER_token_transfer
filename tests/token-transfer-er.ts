import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { TokenTransferEr } from "../target/types/token_transfer_er";
import { Keypair, PublicKey, SystemProgram } from "@solana/web3.js";
import { createMint, getAssociatedTokenAddressSync, getOrCreateAssociatedTokenAccount, mintTo, TOKEN_PROGRAM_ID } from "@solana/spl-token";
import { ASSOCIATED_PROGRAM_ID } from "@coral-xyz/anchor/dist/cjs/utils/token";
import { min } from "bn.js";
import * as fs from "fs";
import {
  GetCommitmentSignature,
  MAGIC_CONTEXT_ID,
  MAGIC_PROGRAM_ID
} from "@magicblock-labs/ephemeral-rollups-sdk";
import { sendMagicTransaction, getClosestValidator } from "magic-router-sdk";
import { web3 } from "@coral-xyz/anchor";

let provider = anchor.AnchorProvider.env();
anchor.setProvider(provider);
const program = anchor.workspace.tokenTransferEr as Program<TokenTransferEr>;

let wallet = JSON.parse(fs.readFileSync("./wallet.json", "utf-8"));
let user = Keypair.fromSecretKey(new Uint8Array(wallet));

describe("token-transfer-er", () => {

  let tokenEscrowAccount: PublicKey;
  let wSolMint: PublicKey;
  let escrowTokenAccount: PublicKey;
  let userTokenAccount: PublicKey;

  let receiverTokenEscrowAccount: PublicKey;
  let receiverTokenAccount: PublicKey;

  const providerEphemeralRollup = new anchor.AnchorProvider(
    new anchor.web3.Connection(process.env.EPHEMERAL_PROVIDER_ENDPOINT || "https://devnet-as.magicblock.app/", {wsEndpoint: process.env.EPHEMERAL_WS_ENDPOINT || "wss://devnet.magicblock.app/"}
    ),
    anchor.Wallet.local()
  );

  const routerConnection = new web3.Connection(
    process.env.ROUTER_ENDPOINT || "https://devnet-router.magicblock.app",
    {
      wsEndpoint: process.env.ROUTER_WS_ENDPOINT || "wss://devnet-router.magicblock.app",
    }
  );

 before(async () => {
    console.log(provider.wallet.publicKey);
    console.log(user.publicKey);
    wSolMint = await createMint (
      provider.connection,
      provider.wallet.payer,
      provider.wallet.publicKey,
      null,
      6
    );

    [tokenEscrowAccount] = PublicKey.findProgramAddressSync(
      [Buffer.from("token_escrow"), wSolMint.toBuffer(), provider.wallet.publicKey.toBuffer()],
      program.programId
    );

    console.log(tokenEscrowAccount)

    let escrowTokenAccountAta = await getOrCreateAssociatedTokenAccount(
      provider.connection,
      provider.wallet.payer,
      wSolMint,
      tokenEscrowAccount,
      true
    );

    escrowTokenAccount = escrowTokenAccountAta.address;

    [receiverTokenEscrowAccount] = PublicKey.findProgramAddressSync(
      [Buffer.from("token_escrow"), wSolMint.toBuffer(), user.publicKey.toBuffer()],
      program.programId
    );

    console.log(receiverTokenEscrowAccount)

    let receiverTokenAccountAta = await getOrCreateAssociatedTokenAccount(
      provider.connection,
      user,
      wSolMint,
      receiverTokenEscrowAccount,
      true
    );

    receiverTokenAccount = receiverTokenAccountAta.address;

    let userTokenAccountAta = await getOrCreateAssociatedTokenAccount(
      provider.connection,
      provider.wallet.payer,
      wSolMint,
      provider.wallet.publicKey,
    );

    userTokenAccount = userTokenAccountAta.address;
  })

  it("Is initialized!", async () => {
    const tx = await program.methods.initialize().rpc();
    console.log("Your transaction signature", tx);
  });

  it("Create sender token escrow", async () => {
    const tx = await program.methods.createTokenEscrow().accountsPartial({
      authority: provider.wallet.publicKey,
      mint: wSolMint,
      tokenEscrow: tokenEscrowAccount,
      escrowTokenAccount: escrowTokenAccount,
      systemProgram: SystemProgram.programId,
      tokenProgram: TOKEN_PROGRAM_ID,
      associatedTokenProgram: ASSOCIATED_PROGRAM_ID
    }).signers([provider.wallet.payer]).rpc();

    console.log(`Transaction Signature: ${tx}`);
  });

  it("Create receiver token escrow", async () => {
    const tx = await program.methods.createTokenEscrow().accountsPartial({
      authority: user.publicKey,
      mint: wSolMint,
      tokenEscrow: receiverTokenEscrowAccount,
      escrowTokenAccount: receiverTokenAccount,
      systemProgram: SystemProgram.programId,
      tokenProgram: TOKEN_PROGRAM_ID,
      associatedTokenProgram: ASSOCIATED_PROGRAM_ID,
    }).signers([user]).rpc();

    console.log(`Transaction Signature: ${tx}`);
  });

  it("Process sender token escrow deposit", async () => {
    await mintTo(
      provider.connection,
      provider.wallet.payer,
      wSolMint,
      userTokenAccount,
      provider.wallet.publicKey,
      500
    );

    let amount = new anchor.BN(100);

    const tx = await program.methods.processTokenEscrowDeposit(amount).accountsPartial({
      authority: provider.wallet.publicKey,
      mint: wSolMint,
      tokenEscrow: tokenEscrowAccount,
      escrowTokenAccount: escrowTokenAccount,
      userTokenAccount: userTokenAccount,
      tokenProgram: TOKEN_PROGRAM_ID
    }).signers([provider.wallet.payer]).rpc();

    console.log(`Transaction Signature: ${tx}`);
  });

  it("Delegate Sender Escrow Account", async () => {
    let validatorKey = await getClosestValidator(routerConnection);
    let commitFrequency = 30000;
    let tx = await program.methods.delegateEscrow(commitFrequency, validatorKey).accountsPartial({
      payer: provider.wallet.publicKey,
      mint: wSolMint,
      tokenEscrow: tokenEscrowAccount,
    }).transaction();

    const signature = await sendMagicTransaction(
      routerConnection,
      tx,
      [provider.wallet.payer],
    );

    await new Promise(resolve => setTimeout(resolve, 5000));

    console.log("Delegated Senders Escrow");
    console.log("Delegation signature", signature);
  });

  it("Delegate Receiver Escrow Account", async () => {
    let validatorKey = await getClosestValidator(routerConnection);
    let commitFrequency = 30000;
    let tx = await program.methods.delegateEscrow(commitFrequency, validatorKey).accountsPartial({
      payer: user.publicKey,
      mint: wSolMint,
      tokenEscrow: receiverTokenEscrowAccount,
    }).transaction();

    const signature = await sendMagicTransaction(
      routerConnection,
      tx,
      [user]
    );

    await new Promise(resolve => setTimeout(resolve, 5000));

    console.log("Delegated Receivers Escrow");
    console.log("Delegation signature", signature);
  });

  it("Process Token Transfer ER", async () => {
    let amount = new anchor.BN(100);
    const tx = await program.methods.tokenEscrowTransferEr(amount).accountsPartial({
      sender: provider.wallet.publicKey,
      receiver: user.publicKey,
      mint: wSolMint,
      senderEscrowAccount: tokenEscrowAccount,
      receverEscrowAccount: receiverTokenEscrowAccount,
    }).transaction();

    const signature = await sendMagicTransaction(
      routerConnection,
      tx,
      [provider.wallet.payer]
    );

    console.log(`Token Transfer ER Transaction Signature: ${signature}`);
  });

  it("Undelegate Sender and Receiver Escrow Account", async () => {
    const tx = await program.methods.processCommitAndUndelegate().accountsPartial({
      payer: provider.wallet.publicKey,
      sender: provider.wallet.publicKey,
      receiver: user.publicKey,
      mint: wSolMint,
      senderTokenEscrow: tokenEscrowAccount,
      receiverTokenEscrow: receiverTokenEscrowAccount,
    }).transaction();

    const signature = await sendMagicTransaction(
      routerConnection,
      tx,
      [provider.wallet.payer]
    );

    console.log("Undelegated Balances of Sender and Receiver");
    console.log(`Transaction Signature: ${signature}`);

    await new Promise(resolve => setTimeout(resolve, 5000)); 

    let senderEscrowAccountInfo = await provider.connection.getAccountInfo(tokenEscrowAccount);
    let receiverEscrowAccountInfo = await provider.connection.getAccountInfo(receiverTokenEscrowAccount);

    console.log(`Sender Escrow Account Owner: ${senderEscrowAccountInfo?.owner}`);
    console.log(`Receiver Escrow Account Owner: ${receiverEscrowAccountInfo?.owner}`);
  });

  it("Withdraw for escrow on-chain", async () => {
    const amount = new anchor.BN(50);

   const tx = await program.methods.processWithdrawFromEscrow(amount).accountsPartial({
      signer: user.publicKey,  
      sender: user.publicKey,  
      receiver: provider.wallet.publicKey,  
      mint: wSolMint,
      senderTokenEscrow: receiverTokenEscrowAccount, 
      senderEscrowTokenAccount: receiverTokenAccount,
      receiverTokenEscrow: tokenEscrowAccount,  
      receiverEscrowTokenAccount: escrowTokenAccount,  
      tokenProgram: TOKEN_PROGRAM_ID,
    }).signers([user]).rpc();  
    console.log(`Transaction Signature: ${tx}`);
  });

});

