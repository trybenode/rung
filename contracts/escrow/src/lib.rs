#![no_std]
//! Rung escrow contract.
//!
//! Day 1: create and fund a job, accept, submit, approve and release.
//! Day 2: auto-release after the review window, dispute freeze, evidence fingerprints,
//!        arbiter ruling with a fixed fee, arbiter timeout, cancel and reclaim paths.
//!
//! Money rules:
//! - The full amount is locked when the job is created.
//! - Funds only ever leave to: the freelancer, the client, the arbiter fee, the platform fee.
//! - The arbiter can only act on a disputed milestone, and only choose how that milestone's
//!   money is split between freelancer and client. The arbiter fee is a fixed amount set in the
//!   job terms, the same whichever side wins.
//! - No path leaves funds stuck: client silent -> auto-release; arbiter silent -> default split;
//!   freelancer never accepts -> client cancels; freelancer stops working -> client reclaims.

use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, token, Address, BytesN,
    Env, Vec,
};

const BUMP_THRESHOLD: u32 = 100_000; // ledgers (about 5 days)
const BUMP_TO: u32 = 500_000; // ledgers (about 29 days)
const MAX_EVIDENCE: u32 = 10; // fingerprints per side per dispute
const BPS: i128 = 10_000;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    /// Client, freelancer and arbiter must be three different addresses.
    SameParty = 1,
    /// Milestone list is empty, or an amount is not positive.
    BadAmounts = 2,
    NoSuchJob = 3,
    /// The job or milestone is not in the right state for this action.
    WrongState = 4,
    NoSuchMilestone = 5,
    /// Milestones must be submitted in order.
    OutOfOrder = 6,
    /// A time window in the terms is zero.
    BadWindow = 7,
    BadFee = 8,
    /// Too early: the window has not run out yet.
    WindowOpen = 9,
    /// Too late: the window has already run out.
    WindowClosed = 10,
    /// Only the client or the freelancer can add evidence.
    NotAParty = 11,
    /// A split ruling must give the freelancer between 1 and 9999 basis points.
    BadRuling = 12,
    TooMuchEvidence = 13,
    /// Arbiter fee is negative, or not smaller than every milestone.
    BadArbiterFee = 14,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JobStatus {
    AwaitingAcceptance,
    Active,
    Completed,
    Cancelled,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MilestoneStatus {
    Pending,
    Submitted,
    Disputed,
    /// Paid to the freelancer in full (approved, auto-released, or ruled to freelancer).
    Released,
    /// Returned to the client (ruling to client, or reclaimed).
    Refunded,
    /// Divided between freelancer and client (ruling or arbiter timeout).
    Split,
}

/// The arbiter's decision on a disputed milestone.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ruling {
    ToFreelancer,
    ToClient,
    /// Freelancer's share in basis points (1 to 9999). The client gets the rest.
    Split(u32),
}

/// Terms the freelancer accepts by accepting the job. All windows are in seconds.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Terms {
    /// Time the client has to approve or dispute after a submission. Longer for new freelancers.
    pub review_window: u64,
    /// Time both sides have to add evidence after a dispute is raised.
    pub dispute_window: u64,
    /// Time the arbiter has to rule after the evidence window closes.
    pub arbiter_window: u64,
    /// Time without activity after which the client may reclaim unspent milestones.
    pub inactivity_window: u64,
    /// Fixed fee paid to the arbiter out of a disputed milestone, whichever side wins.
    pub arbiter_fee: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Milestone {
    pub amount: i128,
    pub status: MilestoneStatus,
    /// SHA-256 fingerprint of the delivery evidence (zeros until submitted).
    pub evidence: BytesN<32>,
    /// Ledger time of submission (0 until submitted). The review window runs from here.
    pub submitted_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Job {
    pub client: Address,
    pub freelancer: Address,
    pub arbiter: Address,
    pub token: Address,
    pub total: i128,
    pub terms: Terms,
    pub status: JobStatus,
    pub milestones: Vec<Milestone>,
    /// Ledger time of the last accept, submission or settlement.
    pub last_activity: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Dispute {
    pub raised_at: u64,
    pub client_evidence: Vec<BytesN<32>>,
    pub freelancer_evidence: Vec<BytesN<32>>,
    /// Fingerprint of the written ruling, set when the arbiter rules.
    pub ruling_hash: Option<BytesN<32>>,
}

#[contracttype]
enum DataKey {
    FeeRecipient,
    FeeBps,
    NextId,
    Job(u32),
    Dispute(u32, u32),
}

// ---------- events the app listens to ----------

#[contractevent]
pub struct JobCreated {
    #[topic]
    pub job_id: u32,
    pub total: i128,
}

#[contractevent]
pub struct JobAccepted {
    #[topic]
    pub job_id: u32,
}

#[contractevent]
pub struct JobCancelled {
    #[topic]
    pub job_id: u32,
    pub refunded: i128,
}

#[contractevent]
pub struct MilestoneSubmitted {
    #[topic]
    pub job_id: u32,
    pub index: u32,
}

#[contractevent]
pub struct MilestoneReleased {
    #[topic]
    pub job_id: u32,
    pub index: u32,
}

#[contractevent]
pub struct AutoReleased {
    #[topic]
    pub job_id: u32,
    pub index: u32,
}

#[contractevent]
pub struct DisputeRaised {
    #[topic]
    pub job_id: u32,
    pub index: u32,
}

#[contractevent]
pub struct EvidenceAdded {
    #[topic]
    pub job_id: u32,
    pub index: u32,
    pub by: Address,
}

#[contractevent]
pub struct DisputeResolved {
    #[topic]
    pub job_id: u32,
    pub index: u32,
    pub ruling: Ruling,
}

#[contractevent]
pub struct ArbiterTimedOut {
    #[topic]
    pub job_id: u32,
    pub index: u32,
}

#[contract]
pub struct RungEscrow;

#[contractimpl]
impl RungEscrow {
    /// Platform fee settings, fixed at deployment. fee_bps is in basis points (100 = 1%).
    pub fn __constructor(env: Env, fee_recipient: Address, fee_bps: u32) -> Result<(), Error> {
        if fee_bps > 1_000 {
            return Err(Error::BadFee); // cap at 10%
        }
        env.storage().instance().set(&DataKey::FeeRecipient, &fee_recipient);
        env.storage().instance().set(&DataKey::FeeBps, &fee_bps);
        env.storage().instance().set(&DataKey::NextId, &1u32);
        Ok(())
    }

    // ------------------------------------------------------------------
    // Day 1: create, accept, submit, approve
    // ------------------------------------------------------------------

    /// Client creates a job and locks the full amount in the contract.
    pub fn create_job(
        env: Env,
        client: Address,
        freelancer: Address,
        arbiter: Address,
        token: Address,
        milestone_amounts: Vec<i128>,
        terms: Terms,
    ) -> Result<u32, Error> {
        client.require_auth();

        if client == freelancer || client == arbiter || freelancer == arbiter {
            return Err(Error::SameParty);
        }
        if milestone_amounts.is_empty() {
            return Err(Error::BadAmounts);
        }
        if terms.review_window == 0
            || terms.dispute_window == 0
            || terms.arbiter_window == 0
            || terms.inactivity_window == 0
        {
            return Err(Error::BadWindow);
        }
        if terms.arbiter_fee < 0 {
            return Err(Error::BadArbiterFee);
        }

        let mut total: i128 = 0;
        let mut smallest: i128 = i128::MAX;
        let mut milestones: Vec<Milestone> = Vec::new(&env);
        for amount in milestone_amounts.iter() {
            if amount <= 0 {
                return Err(Error::BadAmounts);
            }
            total = total.checked_add(amount).ok_or(Error::BadAmounts)?;
            if amount < smallest {
                smallest = amount;
            }
            milestones.push_back(Milestone {
                amount,
                status: MilestoneStatus::Pending,
                evidence: BytesN::from_array(&env, &[0u8; 32]),
                submitted_at: 0,
            });
        }
        // The arbiter fee must always be payable out of any single milestone.
        if terms.arbiter_fee >= smallest {
            return Err(Error::BadArbiterFee);
        }

        // Lock the full amount from the start.
        token::Client::new(&env, &token).transfer(&client, &env.current_contract_address(), &total);

        let id: u32 = env.storage().instance().get(&DataKey::NextId).unwrap_or(1);
        env.storage().instance().set(&DataKey::NextId, &(id + 1));
        env.storage().instance().extend_ttl(BUMP_THRESHOLD, BUMP_TO);

        let job = Job {
            client,
            freelancer,
            arbiter,
            token,
            total,
            terms,
            status: JobStatus::AwaitingAcceptance,
            milestones,
            last_activity: env.ledger().timestamp(),
        };
        Self::save(&env, id, &job);
        JobCreated { job_id: id, total }.publish(&env);
        Ok(id)
    }

    /// Freelancer accepts the terms (including all windows). Job becomes Active.
    pub fn accept_job(env: Env, job_id: u32) -> Result<(), Error> {
        let mut job = Self::load(&env, job_id)?;
        job.freelancer.require_auth();
        if job.status != JobStatus::AwaitingAcceptance {
            return Err(Error::WrongState);
        }
        job.status = JobStatus::Active;
        job.last_activity = env.ledger().timestamp();
        Self::save(&env, job_id, &job);
        JobAccepted { job_id }.publish(&env);
        Ok(())
    }

    /// Freelancer submits a milestone with the fingerprint of the evidence.
    pub fn submit_milestone(
        env: Env,
        job_id: u32,
        index: u32,
        evidence: BytesN<32>,
    ) -> Result<(), Error> {
        let mut job = Self::load(&env, job_id)?;
        job.freelancer.require_auth();
        if job.status != JobStatus::Active {
            return Err(Error::WrongState);
        }
        let mut m = job.milestones.get(index).ok_or(Error::NoSuchMilestone)?;
        if m.status != MilestoneStatus::Pending {
            return Err(Error::WrongState);
        }
        if index > 0 {
            let prev = job.milestones.get(index - 1).ok_or(Error::NoSuchMilestone)?;
            if !is_terminal(prev.status) {
                return Err(Error::OutOfOrder);
            }
        }
        let now = env.ledger().timestamp();
        m.status = MilestoneStatus::Submitted;
        m.evidence = evidence;
        m.submitted_at = now;
        job.milestones.set(index, m);
        job.last_activity = now;
        Self::save(&env, job_id, &job);
        MilestoneSubmitted { job_id, index }.publish(&env);
        Ok(())
    }

    /// Client approves a submitted milestone. Releases it to the freelancer minus the platform fee.
    pub fn approve_milestone(env: Env, job_id: u32, index: u32) -> Result<(), Error> {
        let mut job = Self::load(&env, job_id)?;
        job.client.require_auth();
        let m = Self::submitted(&job, index)?;

        Self::pay_freelancer(&env, &job, m.amount);
        Self::finish(&mut job, index, MilestoneStatus::Released, env.ledger().timestamp());
        Self::save(&env, job_id, &job);
        MilestoneReleased { job_id, index }.publish(&env);
        Ok(())
    }

    // ------------------------------------------------------------------
    // Day 2: auto-release
    // ------------------------------------------------------------------

    /// Anyone can call this once the review window has run out with no approval or dispute.
    /// The money goes to the freelancer, so no signature is needed.
    pub fn claim_after_window(env: Env, job_id: u32, index: u32) -> Result<(), Error> {
        let mut job = Self::load(&env, job_id)?;
        let m = Self::submitted(&job, index)?;
        let now = env.ledger().timestamp();
        if now < m.submitted_at.saturating_add(job.terms.review_window) {
            return Err(Error::WindowOpen);
        }
        Self::pay_freelancer(&env, &job, m.amount);
        Self::finish(&mut job, index, MilestoneStatus::Released, now);
        Self::save(&env, job_id, &job);
        AutoReleased { job_id, index }.publish(&env);
        Ok(())
    }

    // ------------------------------------------------------------------
    // Day 2: disputes
    // ------------------------------------------------------------------

    /// Client disputes a submitted milestone inside the review window. Freezes that milestone.
    pub fn raise_dispute(
        env: Env,
        job_id: u32,
        index: u32,
        evidence: BytesN<32>,
    ) -> Result<(), Error> {
        let mut job = Self::load(&env, job_id)?;
        job.client.require_auth();
        let mut m = Self::submitted(&job, index)?;
        let now = env.ledger().timestamp();
        if now >= m.submitted_at.saturating_add(job.terms.review_window) {
            return Err(Error::WindowClosed);
        }
        m.status = MilestoneStatus::Disputed;
        job.milestones.set(index, m);
        job.last_activity = now;

        let mut client_evidence: Vec<BytesN<32>> = Vec::new(&env);
        client_evidence.push_back(evidence);
        let dispute = Dispute {
            raised_at: now,
            client_evidence,
            freelancer_evidence: Vec::new(&env),
            ruling_hash: None,
        };
        Self::save_dispute(&env, job_id, index, &dispute);
        Self::save(&env, job_id, &job);
        DisputeRaised { job_id, index }.publish(&env);
        Ok(())
    }

    /// Client or freelancer adds an evidence fingerprint while the evidence window is open.
    pub fn add_evidence(
        env: Env,
        job_id: u32,
        index: u32,
        party: Address,
        evidence: BytesN<32>,
    ) -> Result<(), Error> {
        party.require_auth();
        let job = Self::load(&env, job_id)?;
        let is_client = party == job.client;
        if !is_client && party != job.freelancer {
            return Err(Error::NotAParty);
        }
        let m = job.milestones.get(index).ok_or(Error::NoSuchMilestone)?;
        if m.status != MilestoneStatus::Disputed {
            return Err(Error::WrongState);
        }
        let mut dispute = Self::load_dispute(&env, job_id, index)?;
        let now = env.ledger().timestamp();
        if now >= dispute.raised_at.saturating_add(job.terms.dispute_window) {
            return Err(Error::WindowClosed);
        }
        if is_client {
            if dispute.client_evidence.len() >= MAX_EVIDENCE {
                return Err(Error::TooMuchEvidence);
            }
            dispute.client_evidence.push_back(evidence);
        } else {
            if dispute.freelancer_evidence.len() >= MAX_EVIDENCE {
                return Err(Error::TooMuchEvidence);
            }
            dispute.freelancer_evidence.push_back(evidence);
        }
        Self::save_dispute(&env, job_id, index, &dispute);
        EvidenceAdded { job_id, index, by: party }.publish(&env);
        Ok(())
    }

    /// The arbiter rules on a disputed milestone. The arbiter fee is paid first, the rest of the
    /// milestone is divided by the ruling. Platform fee applies only to what the freelancer gets.
    pub fn resolve_dispute(
        env: Env,
        job_id: u32,
        index: u32,
        ruling: Ruling,
        ruling_hash: BytesN<32>,
    ) -> Result<(), Error> {
        let mut job = Self::load(&env, job_id)?;
        job.arbiter.require_auth();
        let m = job.milestones.get(index).ok_or(Error::NoSuchMilestone)?;
        if job.status != JobStatus::Active || m.status != MilestoneStatus::Disputed {
            return Err(Error::WrongState);
        }
        if let Ruling::Split(bps) = ruling {
            if bps == 0 || bps >= 10_000 {
                return Err(Error::BadRuling);
            }
        }

        let fee = job.terms.arbiter_fee;
        let pool = m.amount - fee;
        Self::pay(&env, &job, &job.arbiter, fee);

        let status = match ruling {
            Ruling::ToFreelancer => {
                Self::pay_freelancer(&env, &job, pool);
                MilestoneStatus::Released
            }
            Ruling::ToClient => {
                Self::pay(&env, &job, &job.client, pool);
                MilestoneStatus::Refunded
            }
            Ruling::Split(bps) => {
                let to_freelancer = pool * (bps as i128) / BPS;
                Self::pay_freelancer(&env, &job, to_freelancer);
                Self::pay(&env, &job, &job.client, pool - to_freelancer);
                MilestoneStatus::Split
            }
        };

        let mut dispute = Self::load_dispute(&env, job_id, index)?;
        dispute.ruling_hash = Some(ruling_hash);
        Self::save_dispute(&env, job_id, index, &dispute);

        Self::finish(&mut job, index, status, env.ledger().timestamp());
        Self::save(&env, job_id, &job);
        DisputeResolved { job_id, index, ruling }.publish(&env);
        Ok(())
    }

    /// If the arbiter has not ruled by the end of the arbiter window, anyone can trigger the
    /// default outcome: a 50/50 split of the full milestone. No arbiter fee is paid.
    pub fn arbiter_timeout(env: Env, job_id: u32, index: u32) -> Result<(), Error> {
        let mut job = Self::load(&env, job_id)?;
        let m = job.milestones.get(index).ok_or(Error::NoSuchMilestone)?;
        if job.status != JobStatus::Active || m.status != MilestoneStatus::Disputed {
            return Err(Error::WrongState);
        }
        let dispute = Self::load_dispute(&env, job_id, index)?;
        let now = env.ledger().timestamp();
        let deadline = dispute
            .raised_at
            .saturating_add(job.terms.dispute_window)
            .saturating_add(job.terms.arbiter_window);
        if now < deadline {
            return Err(Error::WindowOpen);
        }

        let to_freelancer = m.amount / 2;
        Self::pay_freelancer(&env, &job, to_freelancer);
        Self::pay(&env, &job, &job.client, m.amount - to_freelancer);

        Self::finish(&mut job, index, MilestoneStatus::Split, now);
        Self::save(&env, job_id, &job);
        ArbiterTimedOut { job_id, index }.publish(&env);
        Ok(())
    }

    // ------------------------------------------------------------------
    // Day 2: ways out, so funds are never stuck
    // ------------------------------------------------------------------

    /// Client cancels a job the freelancer has not accepted yet. Full refund.
    pub fn cancel_before_start(env: Env, job_id: u32) -> Result<(), Error> {
        let mut job = Self::load(&env, job_id)?;
        job.client.require_auth();
        if job.status != JobStatus::AwaitingAcceptance {
            return Err(Error::WrongState);
        }
        Self::pay(&env, &job, &job.client, job.total);
        job.status = JobStatus::Cancelled;
        Self::save(&env, job_id, &job);
        JobCancelled { job_id, refunded: job.total }.publish(&env);
        Ok(())
    }

    /// If the freelancer goes quiet, the client reclaims every milestone that was never submitted,
    /// once the inactivity window has run out. Only allowed when nothing is submitted or disputed.
    pub fn reclaim_inactive(env: Env, job_id: u32) -> Result<(), Error> {
        let mut job = Self::load(&env, job_id)?;
        job.client.require_auth();
        if job.status != JobStatus::Active {
            return Err(Error::WrongState);
        }
        let now = env.ledger().timestamp();
        if now < job.last_activity.saturating_add(job.terms.inactivity_window) {
            return Err(Error::WindowOpen);
        }
        let mut refund: i128 = 0;
        for (i, m) in job.milestones.iter().enumerate() {
            match m.status {
                MilestoneStatus::Submitted | MilestoneStatus::Disputed => {
                    return Err(Error::WrongState)
                }
                MilestoneStatus::Pending => {
                    refund += m.amount;
                    let mut m2 = m.clone();
                    m2.status = MilestoneStatus::Refunded;
                    job.milestones.set(i as u32, m2);
                }
                _ => {}
            }
        }
        Self::pay(&env, &job, &job.client, refund);
        job.status = JobStatus::Cancelled;
        job.last_activity = now;
        Self::save(&env, job_id, &job);
        JobCancelled { job_id, refunded: refund }.publish(&env);
        Ok(())
    }

    // ------------------------------------------------------------------
    // Views
    // ------------------------------------------------------------------

    pub fn get_job(env: Env, job_id: u32) -> Result<Job, Error> {
        Self::load(&env, job_id)
    }

    pub fn get_dispute(env: Env, job_id: u32, index: u32) -> Result<Dispute, Error> {
        Self::load_dispute(&env, job_id, index)
    }

    // ------------------------------------------------------------------
    // Internal helpers
    // ------------------------------------------------------------------

    fn load(env: &Env, job_id: u32) -> Result<Job, Error> {
        let key = DataKey::Job(job_id);
        let job = env.storage().persistent().get(&key).ok_or(Error::NoSuchJob)?;
        env.storage().persistent().extend_ttl(&key, BUMP_THRESHOLD, BUMP_TO);
        Ok(job)
    }

    fn save(env: &Env, job_id: u32, job: &Job) {
        let key = DataKey::Job(job_id);
        env.storage().persistent().set(&key, job);
        env.storage().persistent().extend_ttl(&key, BUMP_THRESHOLD, BUMP_TO);
    }

    fn load_dispute(env: &Env, job_id: u32, index: u32) -> Result<Dispute, Error> {
        let key = DataKey::Dispute(job_id, index);
        let d = env.storage().persistent().get(&key).ok_or(Error::WrongState)?;
        env.storage().persistent().extend_ttl(&key, BUMP_THRESHOLD, BUMP_TO);
        Ok(d)
    }

    fn save_dispute(env: &Env, job_id: u32, index: u32, d: &Dispute) {
        let key = DataKey::Dispute(job_id, index);
        env.storage().persistent().set(&key, d);
        env.storage().persistent().extend_ttl(&key, BUMP_THRESHOLD, BUMP_TO);
    }

    /// The milestone at `index`, which must be Submitted on an Active job.
    fn submitted(job: &Job, index: u32) -> Result<Milestone, Error> {
        if job.status != JobStatus::Active {
            return Err(Error::WrongState);
        }
        let m = job.milestones.get(index).ok_or(Error::NoSuchMilestone)?;
        if m.status != MilestoneStatus::Submitted {
            return Err(Error::WrongState);
        }
        Ok(m)
    }

    /// Mark a milestone settled; complete the job when every milestone is settled.
    fn finish(job: &mut Job, index: u32, status: MilestoneStatus, now: u64) {
        let mut m = job.milestones.get(index).unwrap();
        m.status = status;
        job.milestones.set(index, m);
        job.last_activity = now;
        if job.milestones.iter().all(|x| is_terminal(x.status)) {
            job.status = JobStatus::Completed;
        }
    }

    fn pay(env: &Env, job: &Job, to: &Address, amount: i128) {
        if amount > 0 {
            token::Client::new(env, &job.token).transfer(
                &env.current_contract_address(),
                to,
                &amount,
            );
        }
    }

    /// Pay the freelancer `gross`, minus the platform fee which goes to the platform.
    fn pay_freelancer(env: &Env, job: &Job, gross: i128) {
        let fee_bps: u32 = env.storage().instance().get(&DataKey::FeeBps).unwrap_or(0);
        let fee_recipient: Address = env.storage().instance().get(&DataKey::FeeRecipient).unwrap();
        let fee = gross * (fee_bps as i128) / BPS;
        Self::pay(env, job, &fee_recipient, fee);
        Self::pay(env, job, &job.freelancer, gross - fee);
    }
}

fn is_terminal(s: MilestoneStatus) -> bool {
    matches!(s, MilestoneStatus::Released | MilestoneStatus::Refunded | MilestoneStatus::Split)
}

#[cfg(test)]
mod test;
