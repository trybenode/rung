#![cfg(test)]
use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token, vec, Address, BytesN, Env,
};

const FEE_BPS: u32 = 200; // 2% platform fee
const ARBITER_FEE: i128 = 5;

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

fn terms() -> Terms {
    Terms {
        review_window: 100,
        dispute_window: 200,
        arbiter_window: 300,
        inactivity_window: 1_000,
        arbiter_fee: ARBITER_FEE,
    }
}

fn hash(env: &Env, b: u8) -> BytesN<32> {
    BytesN::from_array(env, &[b; 32])
}

fn at(c: &Ctx, t: u64) {
    c.env.ledger().set_timestamp(t);
}

fn bal(c: &Ctx, who: &Address) -> i128 {
    token::Client::new(&c.env, &c.token).balance(who)
}

/// Job with milestones 100, 200, 300, created at time 1000, accepted.
fn active_job(c: &Ctx) -> u32 {
    at(c, 1_000);
    let id = new_job(c);
    c.escrow.accept_job(&id);
    id
}

fn new_job(c: &Ctx) -> u32 {
    c.escrow.create_job(
        &c.buyer,
        &c.freelancer,
        &c.arbiter,
        &c.token,
        &vec![&c.env, 100i128, 200, 300],
        &terms(),
    )
}

/// Active job with milestone 0 submitted at t=1000 and disputed at t=1050.
fn disputed_job(c: &Ctx) -> u32 {
    let id = active_job(c);
    c.escrow.submit_milestone(&id, &0, &hash(&c.env, 1));
    at(c, 1_050);
    c.escrow.raise_dispute(&id, &0, &hash(&c.env, 2));
    id
}

// ---------------------------------------------------------------- Day 1 behaviour

#[test]
fn happy_path_three_milestones() {
    let c = setup();
    let id = new_job(&c);
    assert_eq!(bal(&c, &c.buyer), 400); // 600 locked up front
    assert_eq!(bal(&c, &c.escrow.address), 600);
    c.escrow.accept_job(&id);
    for i in 0..3u32 {
        c.escrow.submit_milestone(&id, &i, &hash(&c.env, i as u8 + 1));
        c.escrow.approve_milestone(&id, &i);
    }
    assert_eq!(bal(&c, &c.freelancer), 588); // 600 minus 2%
    assert_eq!(bal(&c, &c.platform), 12);
    assert_eq!(bal(&c, &c.escrow.address), 0);
    assert_eq!(c.escrow.get_job(&id).status, JobStatus::Completed);
}

#[test]
fn funds_stay_locked_until_approval() {
    let c = setup();
    let id = active_job(&c);
    c.escrow.submit_milestone(&id, &0, &hash(&c.env, 9));
    assert_eq!(bal(&c, &c.freelancer), 0);
    assert_eq!(bal(&c, &c.escrow.address), 600);
}

#[test]
fn submission_records_evidence_and_time() {
    let c = setup();
    let id = active_job(&c);
    at(&c, 1_234);
    c.escrow.submit_milestone(&id, &0, &hash(&c.env, 7));
    let m = c.escrow.get_job(&id).milestones.get(0).unwrap();
    assert_eq!(m.status, MilestoneStatus::Submitted);
    assert_eq!(m.evidence, hash(&c.env, 7));
    assert_eq!(m.submitted_at, 1_234);
}

#[test]
fn three_parties_must_be_different() {
    let c = setup();
    let amounts = vec![&c.env, 100i128];
    let r = c.escrow.try_create_job(&c.buyer, &c.buyer, &c.arbiter, &c.token, &amounts, &terms());
    assert_eq!(r, Err(Ok(Error::SameParty)));
    let r = c.escrow.try_create_job(&c.buyer, &c.freelancer, &c.buyer, &c.token, &amounts, &terms());
    assert_eq!(r, Err(Ok(Error::SameParty)));
    let r = c.escrow.try_create_job(&c.buyer, &c.freelancer, &c.freelancer, &c.token, &amounts, &terms());
    assert_eq!(r, Err(Ok(Error::SameParty)));
    assert_eq!(bal(&c, &c.buyer), 1_000); // nothing locked
}

#[test]
fn bad_amounts_rejected() {
    let c = setup();
    let go = |amounts: Vec<i128>| {
        c.escrow.try_create_job(&c.buyer, &c.freelancer, &c.arbiter, &c.token, &amounts, &terms())
    };
    assert_eq!(go(vec![&c.env]), Err(Ok(Error::BadAmounts)));
    assert_eq!(go(vec![&c.env, 0i128]), Err(Ok(Error::BadAmounts)));
    assert_eq!(go(vec![&c.env, 100i128, -5]), Err(Ok(Error::BadAmounts)));
}

#[test]
fn bad_terms_rejected() {
    let c = setup();
    let amounts = vec![&c.env, 100i128, 200];
    let go = |t: Terms| {
        c.escrow.try_create_job(&c.buyer, &c.freelancer, &c.arbiter, &c.token, &amounts, &t)
    };
    assert_eq!(go(Terms { review_window: 0, ..terms() }), Err(Ok(Error::BadWindow)));
    assert_eq!(go(Terms { dispute_window: 0, ..terms() }), Err(Ok(Error::BadWindow)));
    assert_eq!(go(Terms { arbiter_window: 0, ..terms() }), Err(Ok(Error::BadWindow)));
    assert_eq!(go(Terms { inactivity_window: 0, ..terms() }), Err(Ok(Error::BadWindow)));
    assert_eq!(go(Terms { arbiter_fee: -1, ..terms() }), Err(Ok(Error::BadArbiterFee)));
    // Fee must be smaller than the smallest milestone (100).
    assert_eq!(go(Terms { arbiter_fee: 100, ..terms() }), Err(Ok(Error::BadArbiterFee)));
    assert!(go(Terms { arbiter_fee: 99, ..terms() }).is_ok());
    assert!(go(Terms { arbiter_fee: 0, ..terms() }).is_ok());
}

#[test]
fn state_guards() {
    let c = setup();
    let id = new_job(&c);
    // Not accepted yet.
    assert_eq!(c.escrow.try_submit_milestone(&id, &0, &hash(&c.env, 1)), Err(Ok(Error::WrongState)));
    c.escrow.accept_job(&id);
    assert_eq!(c.escrow.try_accept_job(&id), Err(Ok(Error::WrongState)));
    // Nothing submitted yet.
    assert_eq!(c.escrow.try_approve_milestone(&id, &0), Err(Ok(Error::WrongState)));
    assert_eq!(c.escrow.try_claim_after_window(&id, &0), Err(Ok(Error::WrongState)));
    assert_eq!(c.escrow.try_raise_dispute(&id, &0, &hash(&c.env, 1)), Err(Ok(Error::WrongState)));
    // Unknown things.
    assert_eq!(c.escrow.try_get_job(&99), Err(Ok(Error::NoSuchJob)));
    assert_eq!(c.escrow.try_submit_milestone(&id, &9, &hash(&c.env, 1)), Err(Ok(Error::NoSuchMilestone)));
}

#[test]
fn milestones_must_go_in_order() {
    let c = setup();
    let id = active_job(&c);
    assert_eq!(c.escrow.try_submit_milestone(&id, &1, &hash(&c.env, 1)), Err(Ok(Error::OutOfOrder)));
    c.escrow.submit_milestone(&id, &0, &hash(&c.env, 1));
    assert_eq!(c.escrow.try_submit_milestone(&id, &1, &hash(&c.env, 2)), Err(Ok(Error::OutOfOrder)));
    c.escrow.approve_milestone(&id, &0);
    c.escrow.submit_milestone(&id, &1, &hash(&c.env, 2));
}

#[test]
fn cannot_release_twice() {
    let c = setup();
    let id = active_job(&c);
    c.escrow.submit_milestone(&id, &0, &hash(&c.env, 1));
    c.escrow.approve_milestone(&id, &0);
    assert_eq!(c.escrow.try_approve_milestone(&id, &0), Err(Ok(Error::WrongState)));
    assert_eq!(bal(&c, &c.freelancer), 98);
    assert_eq!(bal(&c, &c.escrow.address), 500);
}

#[test]
#[should_panic]
fn platform_fee_is_capped() {
    let env = Env::default();
    env.mock_all_auths();
    env.register(RungEscrow, (Address::generate(&env), 5_000u32));
}

// ---------------------------------------------------------------- auto-release

#[test]
fn auto_release_after_window() {
    let c = setup();
    let id = active_job(&c);
    c.escrow.submit_milestone(&id, &0, &hash(&c.env, 1)); // t=1000, window 100
    at(&c, 1_099);
    assert_eq!(c.escrow.try_claim_after_window(&id, &0), Err(Ok(Error::WindowOpen)));
    assert_eq!(bal(&c, &c.freelancer), 0);
    at(&c, 1_100);
    c.escrow.claim_after_window(&id, &0);
    assert_eq!(bal(&c, &c.freelancer), 98);
    assert_eq!(bal(&c, &c.platform), 2);
    let m = c.escrow.get_job(&id).milestones.get(0).unwrap();
    assert_eq!(m.status, MilestoneStatus::Released);
    // Cannot claim twice.
    assert_eq!(c.escrow.try_claim_after_window(&id, &0), Err(Ok(Error::WrongState)));
}

// ---------------------------------------------------------------- disputes

#[test]
fn dispute_freezes_the_milestone() {
    let c = setup();
    let id = disputed_job(&c);
    let m = c.escrow.get_job(&id).milestones.get(0).unwrap();
    assert_eq!(m.status, MilestoneStatus::Disputed);
    // Neither approve nor auto-release can move the money now, even long after the window.
    at(&c, 99_999);
    assert_eq!(c.escrow.try_approve_milestone(&id, &0), Err(Ok(Error::WrongState)));
    assert_eq!(c.escrow.try_claim_after_window(&id, &0), Err(Ok(Error::WrongState)));
    assert_eq!(bal(&c, &c.escrow.address), 600);
    let d = c.escrow.get_dispute(&id, &0);
    assert_eq!(d.raised_at, 1_050);
    assert_eq!(d.client_evidence.len(), 1);
    assert_eq!(d.freelancer_evidence.len(), 0);
}

#[test]
fn cannot_dispute_after_review_window() {
    let c = setup();
    let id = active_job(&c);
    c.escrow.submit_milestone(&id, &0, &hash(&c.env, 1)); // t=1000
    at(&c, 1_100);
    assert_eq!(c.escrow.try_raise_dispute(&id, &0, &hash(&c.env, 2)), Err(Ok(Error::WindowClosed)));
}

#[test]
fn ruling_to_freelancer() {
    let c = setup();
    let id = disputed_job(&c);
    c.escrow.resolve_dispute(&id, &0, &Ruling::ToFreelancer, &hash(&c.env, 3));
    // Arbiter 5; pool 95; platform fee 2% of 95 = 1; freelancer 94.
    assert_eq!(bal(&c, &c.arbiter), 5);
    assert_eq!(bal(&c, &c.platform), 1);
    assert_eq!(bal(&c, &c.freelancer), 94);
    assert_eq!(bal(&c, &c.escrow.address), 500);
    let job = c.escrow.get_job(&id);
    assert_eq!(job.milestones.get(0).unwrap().status, MilestoneStatus::Released);
    assert_eq!(c.escrow.get_dispute(&id, &0).ruling_hash, Some(hash(&c.env, 3)));
}

#[test]
fn ruling_to_client() {
    let c = setup();
    let id = disputed_job(&c);
    c.escrow.resolve_dispute(&id, &0, &Ruling::ToClient, &hash(&c.env, 3));
    assert_eq!(bal(&c, &c.arbiter), 5);
    assert_eq!(bal(&c, &c.freelancer), 0);
    assert_eq!(bal(&c, &c.platform), 0); // no platform fee on money returned to the client
    assert_eq!(bal(&c, &c.buyer), 400 + 95);
    assert_eq!(c.escrow.get_job(&id).milestones.get(0).unwrap().status, MilestoneStatus::Refunded);
}

#[test]
fn ruling_split() {
    let c = setup();
    let id = disputed_job(&c);
    c.escrow.resolve_dispute(&id, &0, &Ruling::Split(6_000), &hash(&c.env, 3));
    // Pool 95: freelancer share 57, client 38. Fee on 57 = 1. Freelancer nets 56.
    assert_eq!(bal(&c, &c.arbiter), 5);
    assert_eq!(bal(&c, &c.freelancer), 56);
    assert_eq!(bal(&c, &c.platform), 1);
    assert_eq!(bal(&c, &c.buyer), 400 + 38);
    assert_eq!(c.escrow.get_job(&id).milestones.get(0).unwrap().status, MilestoneStatus::Split);
}

#[test]
fn bad_split_rulings_rejected() {
    let c = setup();
    let id = disputed_job(&c);
    for bps in [0u32, 10_000, 20_000] {
        assert_eq!(
            c.escrow.try_resolve_dispute(&id, &0, &Ruling::Split(bps), &hash(&c.env, 3)),
            Err(Ok(Error::BadRuling))
        );
    }
    assert_eq!(bal(&c, &c.escrow.address), 600); // nothing moved
}

#[test]
fn arbiter_fee_is_the_same_whichever_side_wins() {
    for ruling in [Ruling::ToFreelancer, Ruling::ToClient, Ruling::Split(5_000)] {
        let c = setup();
        let id = disputed_job(&c);
        c.escrow.resolve_dispute(&id, &0, &ruling, &hash(&c.env, 3));
        assert_eq!(bal(&c, &c.arbiter), ARBITER_FEE);
    }
}

#[test]
fn money_is_conserved_in_every_ruling() {
    for ruling in [Ruling::ToFreelancer, Ruling::ToClient, Ruling::Split(3_333), Ruling::Split(9_999)] {
        let c = setup();
        let id = disputed_job(&c);
        c.escrow.resolve_dispute(&id, &0, &ruling, &hash(&c.env, 3));
        let paid_out = bal(&c, &c.buyer) - 400
            + bal(&c, &c.freelancer)
            + bal(&c, &c.platform)
            + bal(&c, &c.arbiter);
        assert_eq!(paid_out, 100); // exactly the disputed milestone
        assert_eq!(bal(&c, &c.escrow.address), 500);
    }
}

#[test]
fn resolve_needs_a_dispute() {
    let c = setup();
    let id = active_job(&c);
    c.escrow.submit_milestone(&id, &0, &hash(&c.env, 1));
    assert_eq!(
        c.escrow.try_resolve_dispute(&id, &0, &Ruling::ToClient, &hash(&c.env, 3)),
        Err(Ok(Error::WrongState))
    );
    // And a settled dispute cannot be ruled on again.
    let c2 = setup();
    let id2 = disputed_job(&c2);
    c2.escrow.resolve_dispute(&id2, &0, &Ruling::ToClient, &hash(&c2.env, 3));
    assert_eq!(
        c2.escrow.try_resolve_dispute(&id2, &0, &Ruling::ToFreelancer, &hash(&c2.env, 4)),
        Err(Ok(Error::WrongState))
    );
}

#[test]
fn evidence_rules() {
    let c = setup();
    let id = disputed_job(&c); // raised at 1050, evidence window 200 -> closes at 1250
    // Strangers cannot add evidence.
    let stranger = Address::generate(&c.env);
    assert_eq!(
        c.escrow.try_add_evidence(&id, &0, &stranger, &hash(&c.env, 5)),
        Err(Ok(Error::NotAParty))
    );
    // Both sides can.
    c.escrow.add_evidence(&id, &0, &c.freelancer, &hash(&c.env, 6));
    c.escrow.add_evidence(&id, &0, &c.buyer, &hash(&c.env, 7));
    let d = c.escrow.get_dispute(&id, &0);
    assert_eq!(d.client_evidence.len(), 2);
    assert_eq!(d.freelancer_evidence.len(), 1);
    // Cap of 10 per side.
    for i in 0..9u8 {
        c.escrow.add_evidence(&id, &0, &c.freelancer, &hash(&c.env, 10 + i));
    }
    assert_eq!(
        c.escrow.try_add_evidence(&id, &0, &c.freelancer, &hash(&c.env, 99)),
        Err(Ok(Error::TooMuchEvidence))
    );
    // Window closes.
    at(&c, 1_250);
    assert_eq!(
        c.escrow.try_add_evidence(&id, &0, &c.buyer, &hash(&c.env, 8)),
        Err(Ok(Error::WindowClosed))
    );
}

#[test]
fn arbiter_timeout_gives_a_default_split() {
    let c = setup();
    let id = disputed_job(&c); // raised 1050 + dispute 200 + arbiter 300 = 1550
    at(&c, 1_549);
    assert_eq!(c.escrow.try_arbiter_timeout(&id, &0), Err(Ok(Error::WindowOpen)));
    at(&c, 1_550);
    c.escrow.arbiter_timeout(&id, &0);
    // 50/50 of the full 100, no arbiter fee. Freelancer half 50, fee 1, nets 49.
    assert_eq!(bal(&c, &c.arbiter), 0);
    assert_eq!(bal(&c, &c.freelancer), 49);
    assert_eq!(bal(&c, &c.platform), 1);
    assert_eq!(bal(&c, &c.buyer), 400 + 50);
    assert_eq!(c.escrow.get_job(&id).milestones.get(0).unwrap().status, MilestoneStatus::Split);
    // The arbiter cannot rule after the default has happened.
    assert_eq!(
        c.escrow.try_resolve_dispute(&id, &0, &Ruling::ToClient, &hash(&c.env, 3)),
        Err(Ok(Error::WrongState))
    );
}

#[test]
fn job_continues_after_a_ruling_and_completes() {
    let c = setup();
    let id = disputed_job(&c);
    c.escrow.resolve_dispute(&id, &0, &Ruling::ToClient, &hash(&c.env, 3));
    // Milestone 1: approved. Milestone 2: auto-released.
    c.escrow.submit_milestone(&id, &1, &hash(&c.env, 4));
    c.escrow.approve_milestone(&id, &1);
    c.escrow.submit_milestone(&id, &2, &hash(&c.env, 5));
    at(&c, 5_000);
    c.escrow.claim_after_window(&id, &2);
    assert_eq!(c.escrow.get_job(&id).status, JobStatus::Completed);
    assert_eq!(bal(&c, &c.escrow.address), 0);
}

// ---------------------------------------------------------------- ways out

#[test]
fn client_can_cancel_before_the_freelancer_accepts() {
    let c = setup();
    let id = new_job(&c);
    c.escrow.cancel_before_start(&id);
    assert_eq!(bal(&c, &c.buyer), 1_000);
    assert_eq!(bal(&c, &c.escrow.address), 0);
    assert_eq!(c.escrow.get_job(&id).status, JobStatus::Cancelled);
    assert_eq!(c.escrow.try_accept_job(&id), Err(Ok(Error::WrongState)));
    // Not after acceptance.
    let c2 = setup();
    let id2 = active_job(&c2);
    assert_eq!(c2.escrow.try_cancel_before_start(&id2), Err(Ok(Error::WrongState)));
}

#[test]
fn client_reclaims_when_the_freelancer_goes_quiet() {
    let c = setup();
    let id = active_job(&c); // last activity 1000, inactivity window 1000
    at(&c, 1_999);
    assert_eq!(c.escrow.try_reclaim_inactive(&id), Err(Ok(Error::WindowOpen)));
    at(&c, 2_000);
    c.escrow.reclaim_inactive(&id);
    assert_eq!(bal(&c, &c.buyer), 1_000);
    assert_eq!(bal(&c, &c.escrow.address), 0);
    assert_eq!(c.escrow.get_job(&id).status, JobStatus::Cancelled);
}

#[test]
fn reclaim_returns_only_unspent_milestones() {
    let c = setup();
    let id = active_job(&c);
    c.escrow.submit_milestone(&id, &0, &hash(&c.env, 1));
    at(&c, 1_050);
    c.escrow.approve_milestone(&id, &0); // activity at 1050
    at(&c, 2_049);
    assert_eq!(c.escrow.try_reclaim_inactive(&id), Err(Ok(Error::WindowOpen)));
    at(&c, 2_050);
    c.escrow.reclaim_inactive(&id);
    assert_eq!(bal(&c, &c.buyer), 400 + 500); // milestones 1 and 2 returned
    assert_eq!(bal(&c, &c.freelancer), 98); // milestone 0 stays paid
    let job = c.escrow.get_job(&id);
    assert_eq!(job.milestones.get(0).unwrap().status, MilestoneStatus::Released);
    assert_eq!(job.milestones.get(1).unwrap().status, MilestoneStatus::Refunded);
    assert_eq!(job.milestones.get(2).unwrap().status, MilestoneStatus::Refunded);
}

#[test]
fn cannot_reclaim_while_work_is_under_review_or_dispute() {
    let c = setup();
    let id = active_job(&c);
    c.escrow.submit_milestone(&id, &0, &hash(&c.env, 1));
    at(&c, 99_999);
    assert_eq!(c.escrow.try_reclaim_inactive(&id), Err(Ok(Error::WrongState)));
}

// ---------------------------------------------------------------- who signs

#[test]
fn each_action_requires_the_right_signer() {
    let c = setup();
    let id = new_job(&c);
    assert_eq!(c.env.auths()[0].0, c.buyer);
    c.escrow.accept_job(&id);
    assert_eq!(c.env.auths()[0].0, c.freelancer);
    c.escrow.submit_milestone(&id, &0, &hash(&c.env, 1));
    assert_eq!(c.env.auths()[0].0, c.freelancer);
    c.escrow.raise_dispute(&id, &0, &hash(&c.env, 2));
    assert_eq!(c.env.auths()[0].0, c.buyer);
    c.escrow.add_evidence(&id, &0, &c.freelancer, &hash(&c.env, 3));
    assert_eq!(c.env.auths()[0].0, c.freelancer);
    c.escrow.resolve_dispute(&id, &0, &Ruling::ToClient, &hash(&c.env, 4));
    assert_eq!(c.env.auths()[0].0, c.arbiter);
    c.escrow.submit_milestone(&id, &1, &hash(&c.env, 5));
    c.escrow.approve_milestone(&id, &1);
    assert_eq!(c.env.auths()[0].0, c.buyer);
}

#[test]
fn cancel_and_reclaim_need_the_client() {
    let c = setup();
    let id = new_job(&c);
    c.escrow.cancel_before_start(&id);
    assert_eq!(c.env.auths()[0].0, c.buyer);
    let c2 = setup();
    let id2 = active_job(&c2);
    at(&c2, 5_000);
    c2.escrow.reclaim_inactive(&id2);
    assert_eq!(c2.env.auths()[0].0, c2.buyer);
}
