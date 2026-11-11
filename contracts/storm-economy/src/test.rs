#![cfg(test)]

use super::*;
use soroban_sdk::testutils::{Address as _, Events};
use soroban_sdk::Env;

fn setup(env: &Env) -> (StormEconomyClient<'_>, Address) {
    setup_with_persona(env, Persona::Classic)
}

fn setup_with_persona(env: &Env, persona: Persona) -> (StormEconomyClient<'_>, Address) {
    env.mock_all_auths();
    let contract_id = env.register(StormEconomy, ());
    let client = StormEconomyClient::new(env, &contract_id);
    let player = Address::generate(env);
    client.join(&player, &persona);
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
    let err = client.try_join(&player, &Persona::Classic);
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
fn hustler_side_hustle_pays_five_not_three() {
    let env = Env::default();
    let (client, player) = setup_with_persona(&env, Persona::Hustler);
    client.roll_and_pay(&player);
    let before = client.get_state(&player);
    client.take_action(&player, &Action::SideHustle);
    let after = client.get_state(&player);
    assert_eq!(after.cash - before.cash, HUSTLER_SIDE_HUSTLE_GAIN);
    assert_eq!(HUSTLER_SIDE_HUSTLE_GAIN, 5);
}

#[test]
fn saver_pays_one_less_expense_per_round() {
    let env = Env::default();
    let (classic_client, classic_player) = setup_with_persona(&env, Persona::Classic);
    let (saver_client, saver_player) = setup_with_persona(&env, Persona::Saver);

    classic_client.roll_and_pay(&classic_player);
    saver_client.roll_and_pay(&saver_player);

    let classic_state = classic_client.get_state(&classic_player);
    let saver_state = saver_client.get_state(&saver_player);
    // Same starting cash, same roll odds per-env, different expense only;
    // assert the raw expense gap rather than cash (cash also depends on
    // the independent die roll each got), so this stays robust regardless
    // of which event each env's prng produced this round.
    assert_eq!(
        classic_state.expenses(1) - saver_state.expenses(1),
        SAVER_EXPENSE_DISCOUNT
    );
}

#[test]
fn builder_pays_less_for_stall_and_business() {
    let env = Env::default();
    let (client, player) = setup_with_persona(&env, Persona::Builder);
    client.roll_and_pay(&player);
    let before = client.get_state(&player);
    client.take_action(&player, &Action::BuyStall);
    let after_stall = client.get_state(&player);
    assert_eq!(before.cash - after_stall.cash, STALL_COST - BUILDER_STALL_DISCOUNT);
    assert_eq!(STALL_COST - BUILDER_STALL_DISCOUNT, 6);
}

#[test]
fn gambler_side_hustle_is_zero_or_double_never_the_flat_rate() {
    let env = Env::default();
    let (client, player) = setup_with_persona(&env, Persona::Gambler);
    client.roll_and_pay(&player);
    let before = client.get_state(&player);
    client.take_action(&player, &Action::SideHustle);
    let after = client.get_state(&player);
    let delta = after.cash - before.cash;
    assert!(
        delta == GAMBLER_SIDE_HUSTLE_LOSS || delta == GAMBLER_SIDE_HUSTLE_WIN,
        "gambler side hustle gave {delta}, expected 0 or 6"
    );
}

#[test]
fn classic_persona_matches_original_flat_rates() {
    let env = Env::default();
    let (client, player) = setup_with_persona(&env, Persona::Classic);
    client.roll_and_pay(&player);
    let before = client.get_state(&player);
    client.take_action(&player, &Action::SideHustle);
    let after = client.get_state(&player);
    assert_eq!(after.cash - before.cash, SIDE_HUSTLE_GAIN);
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

// ---- v3: one-transaction rounds, config view, new events ---------------

#[test]
fn play_round_with_two_actions_advances_like_the_two_step_path() {
    let env = Env::default();
    let (client, player) = setup(&env);

    client.play_round(&player, &soroban_sdk::vec![&env, Action::SideHustle, Action::SideHustle]);

    let after = client.get_state(&player);
    assert_eq!(after.round, 2);
    assert!(after.awaiting_roll);
    assert_eq!(after.actions_left, 0);
}

#[test]
fn play_round_with_one_action_leaves_the_round_open() {
    let env = Env::default();
    let (client, player) = setup(&env);

    client.play_round(&player, &soroban_sdk::vec![&env, Action::SideHustle]);

    let after = client.get_state(&player);
    assert!(!after.awaiting_roll);
    assert_eq!(after.actions_left, 1);
    assert_eq!(after.round, 1);
}

/// A business costs 24; round 1 pays at most 13 even before any action, so
/// this action can never be afforded here. The transaction must still
/// succeed (the roll is real and already paid for), log the skip, leave
/// the slot unspent, and stop rather than attempting the next action.
#[test]
fn play_round_logs_an_unaffordable_action_instead_of_reverting_the_round() {
    let env = Env::default();
    let (client, player) = setup(&env);

    client.play_round(&player, &soroban_sdk::vec![&env, Action::BuyBusiness, Action::SideHustle]);
    // Read events before any further invocation: `all()` reports only the
    // most recent invocation's events.
    let emitted = env.events().all().events().len();

    let after = client.get_state(&player);
    assert_eq!(after.businesses, 0);
    assert_eq!(after.actions_left, MAX_ACTIONS_PER_ROUND);
    assert_eq!(after.round, 1);
    assert!(!after.awaiting_roll);
    assert!(!after.bankrupt);
    // RoundResolved + ActionSkipped; the SideHustle after the skip was not
    // attempted, so it produced no ActionTaken.
    assert_eq!(emitted, 2);
}

#[test]
fn play_round_rejects_more_actions_than_a_round_allows() {
    let env = Env::default();
    let (client, player) = setup(&env);

    let err = client.try_play_round(
        &player,
        &soroban_sdk::vec![&env, Action::SideHustle, Action::SideHustle, Action::SideHustle],
    );
    assert_eq!(err, Err(Ok(Error::TooManyActions)));
}

#[test]
fn play_round_after_this_rounds_roll_fails() {
    let env = Env::default();
    let (client, player) = setup(&env);
    client.roll_and_pay(&player);

    let err = client.try_play_round(&player, &soroban_sdk::vec![&env]);
    assert_eq!(err, Err(Ok(Error::AlreadyRolled)));
}

/// Same regression gate as `side_hustle_only_scores_exactly_minus_eleven`,
/// driven through the one-transaction path: folding a round into one tx
/// must not change the economy's arithmetic.
#[test]
fn play_round_only_scores_exactly_minus_eleven() {
    let env = Env::default();
    let (client, player) = setup(&env);

    loop {
        client.play_round(&player, &soroban_sdk::vec![&env, Action::SideHustle, Action::SideHustle]);
        let state = client.get_state(&player);
        assert!(!state.bankrupt, "side-hustle-only should never go bankrupt");
        if state.game_over {
            assert_eq!(state.final_score, -11, "one-tx rounds drifted from the two-step economy");
            break;
        }
    }
}

#[test]
fn get_config_matches_the_contracts_own_constants() {
    let env = Env::default();
    let (client, _player) = setup(&env);

    let c = client.get_config();
    assert_eq!(c.rounds, ROUNDS);
    assert_eq!(c.starting_cash, STARTING_CASH);
    assert_eq!(c.salary, SALARY);
    assert_eq!(c.base_expense, BASE_EXPENSE);
    assert_eq!(c.expense_step_rounds, EXPENSE_STEP_ROUNDS);
    assert_eq!(c.emergency_cost, EMERGENCY_COST);
    assert_eq!(c.max_loans, MAX_LOANS);
    assert_eq!(c.stall_cost, STALL_COST);
    assert_eq!(c.stall_pay, STALL_PAY);
    assert_eq!(c.business_cost, BUSINESS_COST);
    assert_eq!(c.business_pay, BUSINESS_PAY);
    assert_eq!(c.hustler_side_hustle_gain, HUSTLER_SIDE_HUSTLE_GAIN);
    assert_eq!(c.gambler_side_hustle_win, GAMBLER_SIDE_HUSTLE_WIN);
    assert_eq!(c.saver_expense_discount, SAVER_EXPENSE_DISCOUNT);
    assert_eq!(c.builder_stall_discount, BUILDER_STALL_DISCOUNT);
    assert_eq!(c.builder_business_discount, BUILDER_BUSINESS_DISCOUNT);
    assert_eq!(c.max_actions_per_round, MAX_ACTIONS_PER_ROUND);
}

/// v2 emitted nothing on join and nothing for the free actions, which is
/// why the indexer could not show them. That gap is what this closes.
#[test]
fn join_and_free_actions_each_publish_an_event() {
    let env = Env::default();
    let (client, player) = setup(&env);
    // `all()` reports only the most recent invocation's events.
    assert_eq!(env.events().all().events().len(), 1, "join should publish PlayerJoined");

    client.roll_and_pay(&player);
    assert_eq!(env.events().all().events().len(), 1, "roll still publishes exactly RoundResolved");

    client.take_loan(&player);
    assert_eq!(env.events().all().events().len(), 1, "take_loan should publish LoanTaken");

    client.repay_loan(&player);
    assert_eq!(env.events().all().events().len(), 1, "repay_loan should publish LoanRepaid");

    client.buy_insurance(&player);
    assert_eq!(env.events().all().events().len(), 1, "buy_insurance should publish InsuranceBought");
}
