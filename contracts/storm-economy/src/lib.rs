//! Storm Money's economy as on-chain state, instead of a JS object only
//! the host's browser can see. Mirrors economy.json / sim.py's round
//! order exactly: event roll -> payday -> bankruptcy check -> up to two
//! buy/hustle actions -> free actions (loan/repay/insurance), any time
//! mid-round. Numbers are the post-rental-cut 2-tier economy (see
//! ../../README.md in this repo for why rental was removed).
//!
//! Randomness: `env.prng()` is seeded from ledger state and
//! predictable (documented in soroban-sdk). That's used here
//! deliberately, not hidden: for a crypto-curious audience, "the storm
//! die is gameable because on-chain randomness without a commit-reveal
//! scheme or VRF is predictable" is itself part of the lesson. Don't
//! reuse this contract anywhere a predictable roll has real value on
//! the line.
#![no_std]

use soroban_sdk::{contract, contracterror, contractevent, contractimpl, contracttype, Address, Env};

// ---- Economy numbers (must match economy.json) ----------------------
const ROUNDS: u32 = 20;
const STARTING_CASH: i32 = 10;
const SALARY: i32 = 8;
const BASE_EXPENSE: i32 = 5;
const EXPENSE_STEP: i32 = 1;
const EXPENSE_STEP_ROUNDS: u32 = 3;
const EMERGENCY_COST: i32 = 12;
const SIDE_HUSTLE_GAIN: i32 = 3;
const LOAN_AMOUNT: i32 = 10;
const LOAN_REPAY_COST: i32 = 10;
const LOAN_EXPENSE: i32 = 1;
const MAX_LOANS: u32 = 3;
const INSURANCE_EXPENSE: i32 = 1;
const STALL_COST: i32 = 8;
const STALL_PAY: i32 = 1;
const BUSINESS_COST: i32 = 24;
const BUSINESS_PAY: i32 = 4;

fn base_expense(round: u32) -> i32 {
    BASE_EXPENSE + EXPENSE_STEP * ((round - 1) / EXPENSE_STEP_ROUNDS) as i32
}

#[contracttype]
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EventRoll {
    MarketDip,
    Emergency,
    Calm,
    Storm,
}

fn roll_event(env: &Env) -> EventRoll {
    match env.prng().gen_range(1u64..=6u64) {
        1 => EventRoll::MarketDip,
        2 => EventRoll::Emergency,
        6 => EventRoll::Storm,
        _ => EventRoll::Calm,
    }
}

#[contracttype]
#[derive(Clone)]
pub struct Player {
    pub cash: i32,
    pub stalls: u32,
    pub businesses: u32,
    pub loans: u32,
    pub insured: bool,
    pub round: u32,        // round this player is about to play (1..=ROUNDS, or ROUNDS+1 once done)
    pub actions_left: u32, // 0 while awaiting the next roll, else 2/1/0 mid-round
    pub awaiting_roll: bool,
    pub bankrupt: bool,
    pub bankrupt_round: u32, // 0 = not bankrupt
    pub free_round: u32,     // 0 = hasn't hit the "Free" milestone yet
    pub final_score: i32,    // only meaningful once round > ROUNDS or bankrupt
    pub game_over: bool,
}

impl Player {
    fn passive_income(&self) -> i32 {
        self.stalls as i32 * STALL_PAY + self.businesses as i32 * BUSINESS_PAY
    }

    fn expenses(&self, round: u32) -> i32 {
        base_expense(round) + self.loans as i32 * LOAN_EXPENSE + if self.insured { INSURANCE_EXPENSE } else { 0 }
    }
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Player(Address),
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    AlreadyJoined = 1,
    NotJoined = 2,
    GameOver = 3,
    Bankrupt = 4,
    MustRollFirst = 5, // called an action before this round's roll_and_pay
    AlreadyRolled = 6, // called roll_and_pay again before using up this round's actions
    NoActionsLeft = 7,
    CantAfford = 8,
    MaxLoansReached = 9,
    NoLoanToRepay = 10,
    AlreadyInsured = 11,
}

// Emitted after every state change so a crypto-curious player can read
// their whole game back from the event log instead of trusting a UI.
#[contractevent]
pub struct RoundResolved {
    #[topic]
    pub player: Address,
    pub round: u32,
    pub roll: EventRoll,
    pub cash_after_payday: i32,
    pub bankrupt: bool,
    pub hit_free_milestone: bool,
}

#[contractevent]
pub struct ActionTaken {
    #[topic]
    pub player: Address,
    pub round: u32,
    pub action: Action,
    pub cash_after: i32,
}

#[contractevent]
pub struct GameFinished {
    #[topic]
    pub player: Address,
    pub final_score: i32,
    pub final_cash: i32,
    pub bankrupt: bool,
}

#[contracttype]
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    BuyStall,
    BuyBusiness,
    SideHustle,
}

#[contract]
pub struct StormEconomy;

#[contractimpl]
impl StormEconomy {
    /// Register a fresh game for `player`. One contract instance can
    /// hold many independent players, each keyed by their own address.
    pub fn join(env: Env, player: Address) -> Result<(), Error> {
        player.require_auth();
        let key = DataKey::Player(player.clone());
        if env.storage().persistent().has(&key) {
            return Err(Error::AlreadyJoined);
        }
        let state = Player {
            cash: STARTING_CASH,
            stalls: 0,
            businesses: 0,
            loans: 0,
            insured: false,
            round: 1,
            actions_left: 0,
            awaiting_roll: true,
            bankrupt: false,
            bankrupt_round: 0,
            free_round: 0,
            final_score: 0,
            game_over: false,
        };
        env.storage().persistent().set(&key, &state);
        extend_ttl(&env, &key);
        Ok(())
    }

    /// Event roll, then payday, then bankruptcy liquidation -- exactly
    /// sim.py's round order. Must be called once at the start of every
    /// round before any buy/hustle/loan/insurance call.
    pub fn roll_and_pay(env: Env, player: Address) -> Result<EventRoll, Error> {
        player.require_auth();
        let key = DataKey::Player(player.clone());
        let mut p: Player = env.storage().persistent().get(&key).ok_or(Error::NotJoined)?;
        if p.game_over {
            return Err(Error::GameOver);
        }
        if p.bankrupt {
            return Err(Error::Bankrupt);
        }
        if !p.awaiting_roll {
            return Err(Error::AlreadyRolled);
        }

        let roll = roll_event(&env);
        if roll == EventRoll::Storm && !p.insured {
            // lose the priciest uninsured asset
            if p.businesses > 0 {
                p.businesses -= 1;
            } else if p.stalls > 0 {
                p.stalls -= 1;
            }
        }
        if roll == EventRoll::Emergency {
            p.cash -= EMERGENCY_COST;
        }

        let exp = p.expenses(p.round);
        let nominal_passive = p.passive_income();
        let used_passive = if roll == EventRoll::MarketDip { nominal_passive / 2 } else { nominal_passive };
        p.cash += SALARY + used_passive - exp;

        let mut hit_free = false;
        if p.free_round == 0 && nominal_passive >= exp {
            p.free_round = p.round;
            hit_free = true;
        }

        // bankruptcy check: sell at half price, businesses first (pricier)
        while p.cash < 0 && (p.businesses > 0 || p.stalls > 0) {
            if p.businesses > 0 {
                p.businesses -= 1;
                p.cash += BUSINESS_COST / 2;
            } else {
                p.stalls -= 1;
                p.cash += STALL_COST / 2;
            }
        }
        if p.cash < 0 {
            p.bankrupt = true;
            p.bankrupt_round = p.round;
            p.game_over = true;
            p.final_score = -999;
        }

        p.awaiting_roll = false;
        p.actions_left = 2;

        RoundResolved {
            player: player.clone(),
            round: p.round,
            roll,
            cash_after_payday: p.cash,
            bankrupt: p.bankrupt,
            hit_free_milestone: hit_free,
        }
        .publish(&env);

        if p.bankrupt {
            GameFinished { player: player.clone(), final_score: p.final_score, final_cash: p.cash, bankrupt: true }
                .publish(&env);
        }

        env.storage().persistent().set(&key, &p);
        extend_ttl(&env, &key);
        Ok(roll)
    }

    /// Spend one of this round's two actions on an asset or a side hustle.
    pub fn take_action(env: Env, player: Address, action: Action) -> Result<(), Error> {
        player.require_auth();
        let key = DataKey::Player(player.clone());
        let mut p: Player = env.storage().persistent().get(&key).ok_or(Error::NotJoined)?;
        if p.game_over {
            return Err(Error::GameOver);
        }
        if p.awaiting_roll {
            return Err(Error::MustRollFirst);
        }
        if p.actions_left == 0 {
            return Err(Error::NoActionsLeft);
        }

        match action {
            Action::BuyStall => {
                if p.cash < STALL_COST {
                    return Err(Error::CantAfford);
                }
                p.cash -= STALL_COST;
                p.stalls += 1;
            }
            Action::BuyBusiness => {
                if p.cash < BUSINESS_COST {
                    return Err(Error::CantAfford);
                }
                p.cash -= BUSINESS_COST;
                p.businesses += 1;
            }
            Action::SideHustle => {
                p.cash += SIDE_HUSTLE_GAIN;
            }
        }
        p.actions_left -= 1;

        ActionTaken { player: player.clone(), round: p.round, action, cash_after: p.cash }.publish(&env);

        if p.actions_left == 0 {
            if p.round >= ROUNDS {
                p.final_score = p.passive_income() - p.expenses(ROUNDS);
                p.game_over = true;
                GameFinished {
                    player: player.clone(),
                    final_score: p.final_score,
                    final_cash: p.cash,
                    bankrupt: false,
                }
                .publish(&env);
            } else {
                p.round += 1;
                p.awaiting_roll = true;
            }
        }

        env.storage().persistent().set(&key, &p);
        extend_ttl(&env, &key);
        Ok(())
    }

    /// Free action: available any time mid-round (after this round's
    /// roll, before the game ends), doesn't consume an action slot.
    pub fn take_loan(env: Env, player: Address) -> Result<(), Error> {
        player.require_auth();
        let key = DataKey::Player(player.clone());
        let mut p: Player = env.storage().persistent().get(&key).ok_or(Error::NotJoined)?;
        require_mid_round(&p)?;
        if p.loans >= MAX_LOANS {
            return Err(Error::MaxLoansReached);
        }
        p.loans += 1;
        p.cash += LOAN_AMOUNT;
        env.storage().persistent().set(&key, &p);
        extend_ttl(&env, &key);
        Ok(())
    }

    pub fn repay_loan(env: Env, player: Address) -> Result<(), Error> {
        player.require_auth();
        let key = DataKey::Player(player.clone());
        let mut p: Player = env.storage().persistent().get(&key).ok_or(Error::NotJoined)?;
        require_mid_round(&p)?;
        if p.loans == 0 {
            return Err(Error::NoLoanToRepay);
        }
        if p.cash < LOAN_REPAY_COST {
            return Err(Error::CantAfford);
        }
        p.loans -= 1;
        p.cash -= LOAN_REPAY_COST;
        env.storage().persistent().set(&key, &p);
        extend_ttl(&env, &key);
        Ok(())
    }

    pub fn buy_insurance(env: Env, player: Address) -> Result<(), Error> {
        player.require_auth();
        let key = DataKey::Player(player.clone());
        let mut p: Player = env.storage().persistent().get(&key).ok_or(Error::NotJoined)?;
        require_mid_round(&p)?;
        if p.insured {
            return Err(Error::AlreadyInsured);
        }
        p.insured = true;
        env.storage().persistent().set(&key, &p);
        extend_ttl(&env, &key);
        Ok(())
    }

    pub fn get_state(env: Env, player: Address) -> Result<Player, Error> {
        env.storage().persistent().get(&DataKey::Player(player)).ok_or(Error::NotJoined)
    }
}

fn require_mid_round(p: &Player) -> Result<(), Error> {
    if p.game_over {
        return Err(Error::GameOver);
    }
    if p.awaiting_roll {
        return Err(Error::MustRollFirst);
    }
    Ok(())
}

fn extend_ttl(env: &Env, key: &DataKey) {
    env.storage().persistent().extend_ttl(key, 120 * 17280, 180 * 17280);
}

mod test;
