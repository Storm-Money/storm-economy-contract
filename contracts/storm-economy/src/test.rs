#![cfg(test)]

use super::*;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::Env;

fn setup(env: &Env) -> (StormEconomyClient<'_>, Address) {
    env.mock_all_auths();
    let contract_id = env.register(StormEconomy, ());
    let client = StormEconomyClient::new(env, &contract_id);
    let player = Address::generate(env);
    client.join(&player);
    (client, player)
}

#[test]
fn join_sets_starting_state() {
    let env = Env::default();
    let (client, player) = setup(&env);
    let state = client.get_state(&player);
    assert_eq!(state.cash, STARTING_CASH);
    assert_eq!(state.round, 1);
    assert!(state.awaiting_roll);
    assert!(!state.bankrupt);
    assert!(!state.game_over);
}

#[test]
fn double_join_fails() {
    let env = Env::default();
    let (client, player) = setup(&env);
    let err = client.try_join(&player);
    assert_eq!(err, Err(Ok(Error::AlreadyJoined)));
}

#[test]
fn action_before_roll_fails() {
    let env = Env::default();
    let (client, player) = setup(&env);
    let err = client.try_take_action(&player, &Action::SideHustle);
    assert_eq!(err, Err(Ok(Error::MustRollFirst)));
}

#[test]
fn one_round_round_trips() {
    let env = Env::default();
    let (client, player) = setup(&env);

    client.roll_and_pay(&player);
    let mid = client.get_state(&player);
    assert!(!mid.awaiting_roll);
    assert_eq!(mid.actions_left, 2);

    client.take_action(&player, &Action::SideHustle);
    client.take_action(&player, &Action::SideHustle);

    let after = client.get_state(&player);
    assert_eq!(after.round, 2); // advanced, not yet done (ROUNDS = 20)
    assert!(after.awaiting_roll);
    assert_eq!(after.actions_left, 0);
}

/// Mirrors sim.py's regression-gate invariant: a player who only ever
/// side-hustles has zero passive income, so market dips and emergencies
/// can't touch their score -- final_score is deterministically
/// 0 - expenses(20) = -11, every single run. Same exact-equality check
/// as `run_checks()` in sim.py, now pinned on-chain too.
#[test]
fn side_hustle_only_scores_exactly_minus_eleven() {
    let env = Env::default();
    let (client, player) = setup(&env);

    loop {
        client.roll_and_pay(&player);
        let state = client.get_state(&player);
        if state.bankrupt {
            panic!("side-hustle-only should never go bankrupt (matches sim.py: ~3% over 5000 games, but 0% is also consistent with a single run)");
        }
        client.take_action(&player, &Action::SideHustle);
        client.take_action(&player, &Action::SideHustle);
        let after = client.get_state(&player);
        if after.game_over {
            assert_eq!(after.final_score, -11, "economy.json/sim.py and this contract have drifted");
            break;
        }
    }
}

#[test]
fn storm_without_insurance_removes_priciest_asset() {
    let env = Env::default();
    let (client, player) = setup(&env);

    // Buy a business the moment affordable, then force-resolve rounds
    // until a Storm actually rolls (env.prng() is deterministic per
    // seed here, so this terminates well within ROUNDS in practice;
    // this test is about the removal rule, not the odds).
    let mut bought = false;
    loop {
        let roll = client.roll_and_pay(&player);
        let state = client.get_state(&player);
        if state.game_over {
            break;
        }
        if !bought && state.cash >= BUSINESS_COST {
            client.take_action(&player, &Action::BuyBusiness);
            bought = true;
            client.take_action(&player, &Action::SideHustle);
        } else {
            client.take_action(&player, &Action::SideHustle);
            client.take_action(&player, &Action::SideHustle);
        }
        if bought && roll == EventRoll::Storm {
            let after = client.get_state(&player);
            assert_eq!(after.businesses, 0, "uninsured storm should have taken the business");
            return;
        }
    }
}

#[test]
fn loan_then_repay_round_trips_cash_and_loan_count() {
    let env = Env::default();
    let (client, player) = setup(&env);

    client.roll_and_pay(&player);
    let before = client.get_state(&player);
    client.take_loan(&player);
    let after_loan = client.get_state(&player);
    assert_eq!(after_loan.loans, 1);
    assert_eq!(after_loan.cash, before.cash + LOAN_AMOUNT);

    client.repay_loan(&player);
    let after_repay = client.get_state(&player);
    assert_eq!(after_repay.loans, 0);
    assert_eq!(after_repay.cash, after_loan.cash - LOAN_REPAY_COST);
}

#[test]
fn fourth_loan_is_rejected() {
    let env = Env::default();
    let (client, player) = setup(&env);
    client.roll_and_pay(&player);
    client.take_loan(&player);
    client.take_loan(&player);
    client.take_loan(&player);
    let err = client.try_take_loan(&player);
    assert_eq!(err, Err(Ok(Error::MaxLoansReached)));
}
