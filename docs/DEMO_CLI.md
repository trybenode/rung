# Testing the Day 2 contract from the terminal (testnet)

Run from `contracts/escrow`. These commands follow Stellar's current docs. They are untested from my side,
so if one fails, add `-- --help` to see the exact argument names and formats.

## Build and deploy

```bash
stellar contract build
stellar contract deploy \
  --wasm target/wasm32v1-none/release/rung_escrow.wasm \
  --source-account alice --network testnet --alias rung_v2 \
  -- --fee_recipient platform --fee_bps 200
```

## Shortcuts for this terminal window

```bash
ALICE=$(stellar keys address alice)
BOB=$(stellar keys address bob)
CAROL=$(stellar keys address carol)
PLATFORM=$(stellar keys address platform)
XLM=$(stellar contract id asset --asset native --network testnet)
ESCROW=$(stellar contract alias show rung_v2 2>/dev/null)
EVID=$(printf '01%.0s' {1..32})
TERMS='{"review_window":60,"dispute_window":60,"arbiter_window":60,"inactivity_window":600,"arbiter_fee":"1000000"}'
```

All times are seconds. 1 XLM = 10,000,000 stroops, so the arbiter fee above is 0.1 XLM.
If the CLI rejects the numbers inside `TERMS`, put quotes around them (`"60"`).

## A. Auto-release (client stays silent)

```bash
stellar contract invoke --id rung_v2 --source-account alice --network testnet -- \
  create_job --client $ALICE --freelancer $BOB --arbiter $CAROL --token $XLM \
  --milestone_amounts '["10000000","20000000","30000000"]' --terms "$TERMS"
stellar contract invoke --id rung_v2 --source-account bob --network testnet -- accept_job --job_id 1
stellar contract invoke --id rung_v2 --source-account bob --network testnet -- \
  submit_milestone --job_id 1 --index 0 --evidence $EVID
# wait 60 seconds, then anyone can claim:
stellar contract invoke --id rung_v2 --source-account alice --network testnet -- \
  claim_after_window --job_id 1 --index 0
```
Calling `claim_after_window` earlier should fail with `WindowOpen` (error 9).

## B. Dispute and ruling

```bash
# job 2, same create and accept as above, then:
stellar contract invoke --id rung_v2 --source-account bob --network testnet -- \
  submit_milestone --job_id 2 --index 0 --evidence $EVID
stellar contract invoke --id rung_v2 --source-account alice --network testnet -- \
  raise_dispute --job_id 2 --index 0 --evidence $EVID
stellar contract invoke --id rung_v2 --source-account bob --network testnet -- \
  add_evidence --job_id 2 --index 0 --party $BOB --evidence $EVID
stellar contract invoke --id rung_v2 --source-account carol --network testnet -- \
  resolve_dispute --job_id 2 --index 0 --ruling '"ToClient"' --ruling_hash $EVID
```
Other rulings: `'"ToFreelancer"'` and a split such as `'{"Split":6000}'` (60% to the freelancer).
The ruling is an enum. If the CLI refuses the format, run `resolve_dispute --help`, or try `'{"ToClient":null}'`.

## C. Arbiter silent

Raise a dispute, do not rule, wait 120 seconds (dispute window plus arbiter window), then:

```bash
stellar contract invoke --id rung_v2 --source-account alice --network testnet -- \
  arbiter_timeout --job_id 3 --index 0
```
Result: a 50/50 split of the milestone, no arbiter fee.

## Check balances any time

```bash
stellar contract invoke --id $XLM --source-account alice --network testnet -- balance --id $ESCROW
stellar contract invoke --id $XLM --source-account alice --network testnet -- balance --id $BOB
```
