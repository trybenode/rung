# Rung
 
Milestone-based payment protection on Stellar for freelancers and their clients.
 
A client locks the full project payment in a Soroban smart contract. The freelancer is paid as each
milestone is approved. If the client goes quiet, the payment releases on its own. If they disagree,
the milestone freezes, both sides add evidence, an AI agent recommends a ruling, and a human decides.
New freelancers get a longer review window for clients, and the window shortens as they build a record.
 
Status: MVP on Stellar testnet. Built for the Stellar Buildathon (Nigeria).
 
## How it works
 
1. **Client creates a job.** They split it into milestones and the full amount is locked in the contract.
2. **Freelancer accepts the terms.** The review window and the other time limits are part of the terms.
3. **Freelancer submits a milestone** with a fingerprint (SHA-256) of the evidence files.
4. **Client approves** and the milestone pays out, minus a small platform fee.
5. **Client silent?** After the review window anyone can trigger the payout to the freelancer.
6. **Client disputes?** The milestone freezes. Both sides add evidence. The arbiter agent reads it and
   recommends a ruling with reasons. A human on the platform team makes the decision. The contract pays the
   freelancer, refunds the client, or splits it. The arbiter fee is fixed and the same whichever side wins.
Three different addresses are involved: client, freelancer and arbiter. They must all be different.
The arbiter can only choose how one disputed milestone is divided. It cannot send money anywhere else.
Every path has a way out, so funds are never stuck: client silent, arbiter silent, freelancer never
accepts, or freelancer goes quiet.
 
**Trust tiers** (New, Early, Established, Top) are shown to clients and set the review window. They never limit
how much a freelancer can earn. A freelancer with a track record elsewhere can submit proof and start higher.
 
## What the MVP includes
 
- Soroban escrow contract: funding, milestones, auto-release, disputes, evidence fingerprints, arbiter ruling,
  default outcome if the arbiter does not act, cancel and reclaim paths.
- Web app: connect a Freighter wallet, create and fund a job, accept, submit, approve, dispute, and a support
  screen for rulings.
- Backend: profiles and tiers, evidence storage and hashing, dispute queue, signing the arbiter ruling.
- Arbiter agent: reads both sides and recommends a ruling. A human always decides.
**Simulated in the MVP:** identity verification (an admin sets it), naira cash-in and cash-out (test tokens
instead), and one platform account signing as the arbiter.
 
**After the MVP:** real identity checks, cash-out through Stellar anchors, a panel of human reviewers,
mainnet and USDC, importing work history from other platforms.
 
## Repository
 
```
contracts/escrow/   Soroban contract and tests
web/                Web app
backend/            API, database, evidence, tiers
agent/              Arbiter agent
docs/               INTERFACE.md (contract spec), API.md, SPLIT.md, DEMO_CLI.md, POSITIONING.md
deployments/        testnet.json with the deployed contract ID and public addresses
```
 
Each folder with code has its own README for how to run it. If your `Cargo.toml` is at the repo root,
skip the `cd contracts/escrow` lines below.
 
## Getting started (Mac)
 
**1. Tools**
 
```bash
# Rust 1.84 or newer, plus the Soroban build target
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# open a new terminal, then:
rustc --version
rustup target add wasm32v1-none
 
# Stellar CLI (a prebuilt binary; Homebrew may compile it from source, which is slow)
curl -fsSL https://github.com/stellar/stellar-cli/raw/main/install.sh | sh
stellar --version
```
 
If the script does not work: `cargo install --locked stellar-cli`.
 
**2. Testnet identities** (each gets free test XLM from Friendbot)
 
```bash
stellar keys generate alice    --network testnet --fund   # client and deployer
stellar keys generate bob      --network testnet --fund   # freelancer
stellar keys generate carol    --network testnet --fund   # arbiter
stellar keys generate platform --network testnet --fund   # fee recipient
```
 
**3. Wallet.** Install the Freighter browser extension, create a wallet, switch it to Testnet and fund it
from the wallet. The web app uses it.
 
**4. Test and build the contract**
 
```bash
cd contracts/escrow
cargo test
stellar contract build
```
 
**5. Deploy to testnet**
 
```bash
stellar contract deploy \
  --wasm target/wasm32v1-none/release/rung_escrow.wasm \
  --source-account alice --network testnet --alias rung_escrow \
  -- --fee_recipient platform --fee_bps 200
```
 
`fee_bps 200` is a 2% platform fee. The contract refuses anything above 10%.
Put the new contract ID and the public addresses in `deployments/testnet.json`.
 
**6. Try it.** `docs/DEMO_CLI.md` walks through a full job from the terminal: create, accept, submit,
approve, auto-release, dispute and ruling. To print the exact argument names for any function, add
`-- --help` after the function name.
 
**7. Run the web app and backend.** See `web/README.md` and `backend/README.md`.
 
## Rules
 
- Testnet only. Never commit secret keys or `.env` files. Copy the `.env.example` files and fill them locally.
- Contract changes go through the contract owner, with `docs/INTERFACE.md` updated in the same change.
- Read `docs/SPLIT.md` before working in someone else's folder.
 