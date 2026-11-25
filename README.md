# Rung escrow: Day 1 contract

Create and fund a job, accept, submit a milestone, approve and release.
The full amount is locked in the contract when the job is created. Each milestone is paid only
when the client approves it. A platform fee (set at deployment) is taken on each release.

Tested: 13 unit tests pass (`cargo test`). Not run from my side: the Stellar CLI commands below
(they follow Stellar's current docs). If one fails, add `-- --help` to see the exact names.

## 1. One-time setup (Mac)

```bash
# Rust 1.84 or newer, plus the Soroban build target
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# restart the terminal, then:
rustc --version
rustup target add wasm32v1-none

# Stellar CLI
brew install stellar-cli
stellar --version

# Testnet identities. --fund gives each free test XLM from Friendbot.
stellar keys generate alice    --network testnet --fund   # client (buyer) and deployer
stellar keys generate bob      --network testnet --fund   # freelancer
stellar keys generate carol    --network testnet --fund   # arbiter
stellar keys generate platform --network testnet --fund   # fee recipient
```

Freighter: install the browser extension, create a wallet, switch it to Testnet, and fund it
with Friendbot from the wallet. You will use it from the app later, not for these CLI tests.

## 2. Run the tests

```bash
cargo test
```

## 3. Build and deploy to testnet

```bash
stellar contract build
# the file lands at target/wasm32v1-none/release/rung_escrow.wasm

stellar contract deploy \
  --wasm target/wasm32v1-none/release/rung_escrow.wasm \
  --source-account alice --network testnet --alias rung_escrow \
  -- --fee_recipient platform --fee_bps 200
```

`fee_bps 200` is a 2% platform fee. The contract refuses anything above 10%.

## 4. Try the whole flow with test XLM

Day 1 uses native testnet XLM as the token (1 XLM = 10,000,000 stroops). Swap in a test USDC later.

```bash
XLM=$(stellar contract id asset --asset native --network testnet)

# client creates and funds a job: three milestones of 1, 2 and 3 XLM, 1 hour review window
stellar contract invoke --id rung_escrow --source-account alice --network testnet -- \
  create_job --client alice --freelancer bob --arbiter carol --token $XLM \
  --milestone_amounts '[10000000,20000000,30000000]' --review_window 3600

# freelancer accepts the terms
stellar contract invoke --id rung_escrow --source-account bob --network testnet -- \
  accept_job --job_id 1

# freelancer submits milestone 0 with a 32-byte evidence fingerprint (64 hex characters)
stellar contract invoke --id rung_escrow --source-account bob --network testnet -- \
  submit_milestone --job_id 1 --index 0 \
  --evidence 0101010101010101010101010101010101010101010101010101010101010101

# client approves: milestone 0 pays out, minus the 2% fee
stellar contract invoke --id rung_escrow --source-account alice --network testnet -- \
  approve_milestone --job_id 1 --index 0

# look at the job
stellar contract invoke --id rung_escrow --source-account alice --network testnet -- \
  get_job --job_id 1
```

## What the contract enforces today

- Client, freelancer and arbiter are three different addresses.
- Milestone amounts are positive, and the review window is above zero.
- The full total is locked at creation. Nothing is paid before approval.
- Only the right party can act: client creates and approves, freelancer accepts and submits.
- Milestones are submitted and released in order, and each can be released once.
- The review window is stored per job. The app picks it from the freelancer's trust tier and
  the freelancer accepts it by accepting the job. It is used for auto-release tomorrow.

## Day 2 (Thursday)

Auto-release after the review window, dispute freeze, evidence fingerprints from both sides,
arbiter ruling (to freelancer, to client, or split), fixed arbiter fee, arbiter timeout.
