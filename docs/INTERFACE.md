# Escrow contract interface (frozen for Build Week)

Owner: Allen. Nobody else changes `contracts/` or this file. If you need a change, ask in the group.
If the contract changes, Allen redeploys, updates `deployments/testnet.json`, and posts the new ID.

Units: amounts are in the token's base units (native XLM: 1 XLM = 10,000,000 stroops). Times are seconds
(ledger timestamp, Unix time). Fees are basis points (100 = 1%). All i128 values are sent as strings in JSON.

## Roles and who signs

| Role | Address | Signs |
|---|---|---|
| Client | buyer | create_job, approve_milestone, raise_dispute, add_evidence (own side), cancel_before_start, reclaim_inactive |
| Freelancer | worker | accept_job, submit_milestone, add_evidence (own side) |
| Arbiter | platform key (held by the backend) | resolve_dispute |
| Anyone | | claim_after_window, arbiter_timeout (money only goes where the rules say, so no signature needed) |

Client, freelancer and arbiter must be three different addresses.

## Types

```
Terms { review_window: u64, dispute_window: u64, arbiter_window: u64, inactivity_window: u64, arbiter_fee: i128 }
Ruling = ToFreelancer | ToClient | Split(u32)        // Split = freelancer share in bps, 1..9999
JobStatus = AwaitingAcceptance | Active | Completed | Cancelled
MilestoneStatus = Pending | Submitted | Disputed | Released | Refunded | Split
Milestone { amount: i128, status, evidence: BytesN<32>, submitted_at: u64 }
Job { client, freelancer, arbiter, token, total: i128, terms: Terms, status, milestones: Vec<Milestone>, last_activity: u64 }
Dispute { raised_at: u64, client_evidence: Vec<BytesN<32>>, freelancer_evidence: Vec<BytesN<32>>, ruling_hash: Option<BytesN<32>> }
```

## Functions

| Function | Who | What it does |
|---|---|---|
| `__constructor(fee_recipient, fee_bps)` | deploy | Platform fee, max 1000 bps (10%) |
| `create_job(client, freelancer, arbiter, token, milestone_amounts, terms) -> job_id` | client | Locks the full total in the contract |
| `accept_job(job_id)` | freelancer | Accepts all terms. Job becomes Active |
| `submit_milestone(job_id, index, evidence)` | freelancer | Evidence = SHA-256 of the evidence bundle. Starts the review window. In order only |
| `approve_milestone(job_id, index)` | client | Releases to the freelancer minus platform fee |
| `claim_after_window(job_id, index)` | anyone | After review_window with no approval or dispute: releases to the freelancer |
| `raise_dispute(job_id, index, evidence)` | client | Only inside the review window. Freezes the milestone |
| `add_evidence(job_id, index, party, evidence)` | client or freelancer | While the dispute window is open. Max 10 per side |
| `resolve_dispute(job_id, index, ruling, ruling_hash)` | arbiter | Pays the fixed arbiter fee, then divides the rest by the ruling |
| `arbiter_timeout(job_id, index)` | anyone | If the arbiter has not ruled by the end of the arbiter window: 50/50 split, no arbiter fee |
| `cancel_before_start(job_id)` | client | Only before the freelancer accepts. Full refund |
| `reclaim_inactive(job_id)` | client | After inactivity_window with nothing submitted or disputed: refunds the unspent milestones |
| `get_job(job_id)`, `get_dispute(job_id, index)` | view | Read state |

## Rules the contract enforces

- The full amount is locked at creation. Money only leaves to: freelancer, client, arbiter fee, platform fee.
- Platform fee applies only to what the freelancer receives. Money returned to the client has no fee.
- Arbiter fee is fixed in the terms and must be smaller than every milestone. Same whichever side wins.
- Arbiter can act only on a disputed milestone, and only choose the split. It cannot send money anywhere else.
- Milestones are submitted in order. The previous one must be settled (Released, Refunded or Split).
- A job is Completed when every milestone is settled.

## Time rules

| Action | Allowed when |
|---|---|
| raise_dispute | now < submitted_at + review_window |
| claim_after_window | now >= submitted_at + review_window |
| add_evidence | now < raised_at + dispute_window |
| arbiter_timeout | now >= raised_at + dispute_window + arbiter_window |
| reclaim_inactive | now >= last_activity + inactivity_window (last_activity moves on accept, submit and every settlement) |

## Terms by trust tier (app chooses them; the freelancer accepts by accepting the job)

Suggested review_window: New 7 days, Early 5 days, Established 3 days, Top 1 day. For the demo use seconds or minutes.

## Errors

| Code | Name | Meaning |
|---|---|---|
| 1 | SameParty | Two of the three roles are the same address |
| 2 | BadAmounts | No milestones, or an amount is not positive |
| 3 | NoSuchJob | |
| 4 | WrongState | Job or milestone is not in the right state for this action |
| 5 | NoSuchMilestone | |
| 6 | OutOfOrder | Previous milestone not settled yet |
| 7 | BadWindow | A window in Terms is zero |
| 8 | BadFee | Platform fee above 10% |
| 9 | WindowOpen | Too early |
| 10 | WindowClosed | Too late |
| 11 | NotAParty | add_evidence from someone who is not client or freelancer |
| 12 | BadRuling | Split must be 1..9999 bps |
| 13 | TooMuchEvidence | More than 10 fingerprints on one side |
| 14 | BadArbiterFee | Negative, or not smaller than every milestone |

## Events (for the app to listen to)

JobCreated(job_id, total), JobAccepted(job_id), JobCancelled(job_id, refunded), MilestoneSubmitted(job_id, index),
MilestoneReleased(job_id, index), AutoReleased(job_id, index), DisputeRaised(job_id, index),
EvidenceAdded(job_id, index, by), DisputeResolved(job_id, index, ruling), ArbiterTimedOut(job_id, index).
