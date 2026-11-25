#![no_std]
//! Rung escrow, Day 1: create and fund a job, accept, submit a milestone, approve and release.
//! Day 2 adds: auto-release after the review window, dispute freeze, arbiter ruling.

use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, token, Address, BytesN,
    Env, Vec,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    /// Client, freelancer and arbiter must be three different addresses.
    SameParty = 1,
    /// Milestone list is empty, or an amount is not positive.
    BadAmounts = 2,
    NoSuchJob = 3,
    /// The job is not in the right state for this action.
    WrongState = 4,
    NoSuchMilestone = 5,
    /// Milestones must be submitted and released in order.
    OutOfOrder = 6,
    BadWindow = 7,
    BadFee = 8,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JobStatus {
    AwaitingAcceptance,
    Active,
    Completed,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MilestoneStatus {
    Pending,
    Submitted,
    Released,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Milestone {
    pub amount: i128,
    pub status: MilestoneStatus,
    /// SHA-256 fingerprint of the evidence files (zeros until submitted).
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
    /// Seconds the client has to respond after a submission. Set per job from the
    /// freelancer's trust tier by the app, and accepted by the freelancer in accept_job.
    pub review_window: u64,
    pub status: JobStatus,
    pub milestones: Vec<Milestone>,
}

#[contracttype]
enum DataKey {
    FeeRecipient,
    FeeBps,
    NextId,
    Job(u32),
}

// Events the app listens to.
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

    /// Client creates a job and locks the full amount in the contract.
    pub fn create_job(
        env: Env,
        client: Address,
        freelancer: Address,
        arbiter: Address,
        token: Address,
        milestone_amounts: Vec<i128>,
        review_window: u64,
    ) -> Result<u32, Error> {
        client.require_auth();

        if client == freelancer || client == arbiter || freelancer == arbiter {
            return Err(Error::SameParty);
        }
        if milestone_amounts.is_empty() || review_window == 0 {
            return Err(if review_window == 0 { Error::BadWindow } else { Error::BadAmounts });
        }

        let mut total: i128 = 0;
        let mut milestones: Vec<Milestone> = Vec::new(&env);
        for amount in milestone_amounts.iter() {
            if amount <= 0 {
                return Err(Error::BadAmounts);
            }
            total = total.checked_add(amount).ok_or(Error::BadAmounts)?;
            milestones.push_back(Milestone {
                amount,
                status: MilestoneStatus::Pending,
                evidence: BytesN::from_array(&env, &[0u8; 32]),
                submitted_at: 0,
            });
        }

        // Lock the full amount from the start.
        token::Client::new(&env, &token).transfer(&client, &env.current_contract_address(), &total);

        let id: u32 = env.storage().instance().get(&DataKey::NextId).unwrap_or(1);
        env.storage().instance().set(&DataKey::NextId, &(id + 1));

        let job = Job {
            client,
            freelancer,
            arbiter,
            token,
            total,
            review_window,
            status: JobStatus::AwaitingAcceptance,
            milestones,
        };
        env.storage().persistent().set(&DataKey::Job(id), &job);
        JobCreated { job_id: id, total }.publish(&env);
        Ok(id)
    }

    /// Freelancer accepts the terms, including the review window. Job becomes Active.
    pub fn accept_job(env: Env, job_id: u32) -> Result<(), Error> {
        let mut job = Self::load(&env, job_id)?;
        job.freelancer.require_auth();
        if job.status != JobStatus::AwaitingAcceptance {
            return Err(Error::WrongState);
        }
        job.status = JobStatus::Active;
        env.storage().persistent().set(&DataKey::Job(job_id), &job);
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
            if prev.status != MilestoneStatus::Released {
                return Err(Error::OutOfOrder);
            }
        }
        m.status = MilestoneStatus::Submitted;
        m.evidence = evidence;
        m.submitted_at = env.ledger().timestamp();
        job.milestones.set(index, m);
        env.storage().persistent().set(&DataKey::Job(job_id), &job);
        MilestoneSubmitted { job_id, index }.publish(&env);
        Ok(())
    }

    /// Client approves a submitted milestone. Releases it to the freelancer minus the platform fee.
    pub fn approve_milestone(env: Env, job_id: u32, index: u32) -> Result<(), Error> {
        let mut job = Self::load(&env, job_id)?;
        job.client.require_auth();
        if job.status != JobStatus::Active {
            return Err(Error::WrongState);
        }
        let mut m = job.milestones.get(index).ok_or(Error::NoSuchMilestone)?;
        if m.status != MilestoneStatus::Submitted {
            return Err(Error::WrongState);
        }

        Self::release(&env, &job, m.amount);

        m.status = MilestoneStatus::Released;
        job.milestones.set(index, m);
        if job.milestones.iter().all(|x| x.status == MilestoneStatus::Released) {
            job.status = JobStatus::Completed;
        }
        env.storage().persistent().set(&DataKey::Job(job_id), &job);
        MilestoneReleased { job_id, index }.publish(&env);
        Ok(())
    }

    pub fn get_job(env: Env, job_id: u32) -> Result<Job, Error> {
        Self::load(&env, job_id)
    }

    // ---------- internal ----------

    fn load(env: &Env, job_id: u32) -> Result<Job, Error> {
        env.storage().persistent().get(&DataKey::Job(job_id)).ok_or(Error::NoSuchJob)
    }

    /// Pay a milestone out of the contract: freelancer gets amount minus fee, platform gets fee.
    fn release(env: &Env, job: &Job, amount: i128) {
        let fee_bps: u32 = env.storage().instance().get(&DataKey::FeeBps).unwrap_or(0);
        let fee_recipient: Address = env.storage().instance().get(&DataKey::FeeRecipient).unwrap();
        let fee = amount * (fee_bps as i128) / 10_000;
        let t = token::Client::new(env, &job.token);
        let me = env.current_contract_address();
        if fee > 0 {
            t.transfer(&me, &fee_recipient, &fee);
        }
        t.transfer(&me, &job.freelancer, &(amount - fee));
    }
}

#[cfg(test)]
mod test;
