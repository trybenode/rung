#![cfg(test)]
use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token, vec, Address, BytesN, Env,
};

const FEE_BPS: u32 = 200; // 2%

struct Ctx {
    env: Env,
    escrow: RungEscrowClient<'static>,
    token: Address,
    buyer: Address,
    freelancer: Address,
    arbiter: Address,
    platform: Address,
}

fn setup() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();
    let platform = Address::generate(&env);
    let contract_id = env.register(RungEscrow, (platform.clone(), FEE_BPS));
    let escrow = RungEscrowClient::new(&env, &contract_id);
    let token = env.register_stellar_asset_contract_v2(Address::generate(&env)).address();
    let buyer = Address::generate(&env);
    token::StellarAssetClient::new(&env, &token).mint(&buyer, &1_000);
    Ctx {
        freelancer: Address::generate(&env),
        arbiter: Address::generate(&env),
        env,
        escrow,
        token,
        buyer,
        platform,
    }
}

fn hash(env: &Env, b: u8) -> BytesN<32> {
    BytesN::from_array(env, &[b; 32])
}

fn new_job(c: &Ctx) -> u32 {
    c.escrow.create_job(
        &c.buyer,
        &c.freelancer,
        &c.arbiter,
        &c.token,
        &vec![&c.env, 100i128, 200, 300],
        &3600,
    )
}

#[test]
fn happy_path_three_milestones() {
    let c = setup();
    let t = token::Client::new(&c.env, &c.token);
    let id = new_job(&c);

    // The full 600 is locked from the start.
    assert_eq!(t.balance(&c.buyer), 400);
    assert_eq!(t.balance(&c.escrow.address), 600);
    assert_eq!(c.escrow.get_job(&id).status, JobStatus::AwaitingAcceptance);

    c.escrow.accept_job(&id);
    assert_eq!(c.escrow.get_job(&id).status, JobStatus::Active);

    for i in 0..3u32 {
        c.escrow.submit_milestone(&id, &i, &hash(&c.env, i as u8 + 1));
        c.escrow.approve_milestone(&id, &i);
    }

    // 2% fee on 600 = 12.
    assert_eq!(t.balance(&c.freelancer), 588);
    assert_eq!(t.balance(&c.platform), 12);
    assert_eq!(t.balance(&c.escrow.address), 0);
    assert_eq!(c.escrow.get_job(&id).status, JobStatus::Completed);
}

#[test]
fn funds_stay_locked_until_approval() {
    let c = setup();
    let t = token::Client::new(&c.env, &c.token);
    let id = new_job(&c);
    c.escrow.accept_job(&id);
    c.escrow.submit_milestone(&id, &0, &hash(&c.env, 9));
    // Submitted but not approved: freelancer has not been paid.
    assert_eq!(t.balance(&c.freelancer), 0);
    assert_eq!(t.balance(&c.escrow.address), 600);
}

#[test]
fn submission_records_evidence_and_time() {
    let c = setup();
    let id = new_job(&c);
    c.escrow.accept_job(&id);
    c.env.ledger().set_timestamp(1_000);
    c.escrow.submit_milestone(&id, &0, &hash(&c.env, 7));
    let m = c.escrow.get_job(&id).milestones.get(0).unwrap();
    assert_eq!(m.status, MilestoneStatus::Submitted);
    assert_eq!(m.evidence, hash(&c.env, 7));
    assert_eq!(m.submitted_at, 1_000);
}

#[test]
fn three_parties_must_be_different() {
    let c = setup();
    let t = token::Client::new(&c.env, &c.token);
    let amounts = vec![&c.env, 100i128];
    let r = c.escrow.try_create_job(&c.buyer, &c.buyer, &c.arbiter, &c.token, &amounts, &3600);
    assert_eq!(r, Err(Ok(Error::SameParty)));
    let r = c.escrow.try_create_job(&c.buyer, &c.freelancer, &c.buyer, &c.token, &amounts, &3600);
    assert_eq!(r, Err(Ok(Error::SameParty)));
    let r = c.escrow.try_create_job(&c.buyer, &c.freelancer, &c.freelancer, &c.token, &amounts, &3600);
    assert_eq!(r, Err(Ok(Error::SameParty)));
    // Nothing was locked.
    assert_eq!(t.balance(&c.buyer), 1_000);
}

#[test]
fn bad_amounts_and_window_rejected() {
    let c = setup();
    let go = |amounts: Vec<i128>, window: u64| {
        c.escrow.try_create_job(&c.buyer, &c.freelancer, &c.arbiter, &c.token, &amounts, &window)
    };
    assert_eq!(go(vec![&c.env], 3600), Err(Ok(Error::BadAmounts)));
    assert_eq!(go(vec![&c.env, 0i128], 3600), Err(Ok(Error::BadAmounts)));
    assert_eq!(go(vec![&c.env, 100i128, -5], 3600), Err(Ok(Error::BadAmounts)));
    assert_eq!(go(vec![&c.env, 100i128], 0), Err(Ok(Error::BadWindow)));
}

#[test]
fn cannot_submit_before_accept() {
    let c = setup();
    let id = new_job(&c);
    let r = c.escrow.try_submit_milestone(&id, &0, &hash(&c.env, 1));
    assert_eq!(r, Err(Ok(Error::WrongState)));
}

#[test]
fn cannot_approve_before_submit() {
    let c = setup();
    let id = new_job(&c);
    c.escrow.accept_job(&id);
    assert_eq!(c.escrow.try_approve_milestone(&id, &0), Err(Ok(Error::WrongState)));
}

#[test]
fn cannot_accept_twice() {
    let c = setup();
    let id = new_job(&c);
    c.escrow.accept_job(&id);
    assert_eq!(c.escrow.try_accept_job(&id), Err(Ok(Error::WrongState)));
}

#[test]
fn milestones_must_go_in_order() {
    let c = setup();
    let id = new_job(&c);
    c.escrow.accept_job(&id);
    // Cannot skip ahead.
    assert_eq!(
        c.escrow.try_submit_milestone(&id, &1, &hash(&c.env, 1)),
        Err(Ok(Error::OutOfOrder))
    );
    // Cannot submit 1 while 0 is only submitted, not released.
    c.escrow.submit_milestone(&id, &0, &hash(&c.env, 1));
    assert_eq!(
        c.escrow.try_submit_milestone(&id, &1, &hash(&c.env, 2)),
        Err(Ok(Error::OutOfOrder))
    );
    // After release of 0, milestone 1 is allowed.
    c.escrow.approve_milestone(&id, &0);
    c.escrow.submit_milestone(&id, &1, &hash(&c.env, 2));
}

#[test]
fn cannot_release_twice() {
    let c = setup();
    let t = token::Client::new(&c.env, &c.token);
    let id = new_job(&c);
    c.escrow.accept_job(&id);
    c.escrow.submit_milestone(&id, &0, &hash(&c.env, 1));
    c.escrow.approve_milestone(&id, &0);
    assert_eq!(c.escrow.try_approve_milestone(&id, &0), Err(Ok(Error::WrongState)));
    // Only milestone 0 (100, minus 2% fee) has been paid.
    assert_eq!(t.balance(&c.freelancer), 98);
    assert_eq!(t.balance(&c.escrow.address), 500);
}

#[test]
fn unknown_job_and_milestone() {
    let c = setup();
    assert_eq!(c.escrow.try_get_job(&99), Err(Ok(Error::NoSuchJob)));
    let id = new_job(&c);
    c.escrow.accept_job(&id);
    assert_eq!(
        c.escrow.try_submit_milestone(&id, &9, &hash(&c.env, 1)),
        Err(Ok(Error::NoSuchMilestone))
    );
}

#[test]
fn each_action_requires_the_right_signer() {
    let c = setup();
    let id = new_job(&c);
    assert_eq!(c.env.auths()[0].0, c.buyer); // create_job: buyer

    c.escrow.accept_job(&id);
    assert_eq!(c.env.auths()[0].0, c.freelancer); // accept_job: freelancer

    c.escrow.submit_milestone(&id, &0, &hash(&c.env, 1));
    assert_eq!(c.env.auths()[0].0, c.freelancer); // submit: freelancer

    c.escrow.approve_milestone(&id, &0);
    assert_eq!(c.env.auths()[0].0, c.buyer); // approve: buyer
}

#[test]
#[should_panic]
fn platform_fee_is_capped() {
    let env = Env::default();
    env.mock_all_auths();
    env.register(RungEscrow, (Address::generate(&env), 5_000u32));
}
