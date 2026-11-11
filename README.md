# Storm Money on Stellar

The economy as on-chain state instead of a JS object only the host's
browser can see. Same round order and numbers as `sim.py` /
`prototype.html` (post-rental-cut: `stall` 8→+1/rd, `business` 24→+4/rd),
reimplemented as a Soroban contract in `contracts/storm-economy`.

## Deployed on testnet

```
Contract ID: CBO4POPU3FQXXYE353LKO6KNHT2QJRZGOFDTQEKELS3H2ZZBNEPF3SOE
Wasm hash:   8a3f34f05e886e3cb26a49343f0a9f7dacd7fd1571b468cea60530b96fab6c32
Explorer:    https://lab.stellar.org/r/testnet/contract/CBO4POPU3FQXXYE353LKO6KNHT2QJRZGOFDTQEKELS3H2ZZBNEPF3SOE
```

**This is the second contract generation (2026-11-05)**, deployed to add
personas (see below) — a one-way door, since Soroban contracts here have
no upgrade function, so adding a `join()` parameter means a new contract
ID, not an in-place update. The first generation stays live and
unmodified at `CABDVHFVUJSNTWLHYWWSOWKIZS6OYFOSRLQNXPXI2OICFSHZCNMG4RAT`
for anyone who already has a game on it; it just isn't the one this repo
(web client, backend indexer) points at anymore.

Verified live, not just locally: `join`, `roll_and_pay`, `take_action`,
`take_loan`/`repay_loan` all round-tripped against real testnet
transactions (not just `cargo test`), including a real `Emergency` roll
whose math checked out exactly (10 start + 8 salary − 5 expenses − 12
emergency = 1), and a real rejected transaction (`take_action` called
before that round's roll, correctly failing simulation with
`Error::MustRollFirst` before any fee was spent). The persona system on
this second generation was verified the same way: real `join()` calls
with each of the five `Persona` values, then a real `roll_and_pay`/
`take_action`, confirming on real testnet transactions that Hustler's
Side Hustle actually paid 5 (not 3), Builder's stall actually cost 6
(not 8), Saver's expenses were actually 1 lower, and Gambler's Side
Hustle actually landed the 0-payout branch at least once — not just
asserted by `cargo test` (21 passing unit tests on this branch, including
one gambler run and one classic-baseline run per persona).

### Third generation (v3) — deployed, not yet the default

```
Contract ID: CDRJL2JJEFNL6DZSA5HROPVSOIMLUI2374ZZSY7LATLG5YM7DBH4BUJ5
Wasm hash:   6358f3c88a74e70e551aef7ff063bc15204232a60bc3aa4d88d42091eb3f2a33
Explorer:    https://lab.stellar.org/r/testnet/contract/CDRJL2JJEFNL6DZSA5HROPVSOIMLUI2374ZZSY7LATLG5YM7DBH4BUJ5
```

Adds `play_round` (a whole round — roll plus up to two actions — in one
signed transaction), `get_config`, and the events the indexer was
missing: `PlayerJoined`, `LoanTaken`, `LoanRepaid`, `InsuranceBought`,
`ActionSkipped`. The v2 entry points are unchanged, so this is additive.

Deployed and exercised with real testnet transactions (not only
`cargo test`): a Hustler's `play_round(["SideHustle","SideHustle"])`
resolved a Calm roll (10 + 8 − 5 = 13) and paid 5 per hustle to 23, then
advanced to round 2; a Builder's `play_round(["BuyBusiness",
"SideHustle"])` published `RoundResolved` + `ActionSkipped(error_code: 8)`
in one successful transaction, left `actions_left` at 2 and the round
open, and did not attempt the hustle — the documented stop-on-rejection
behaviour, without throwing away the paid-for roll. `take_loan` also
emitted `LoanTaken` live.

**It is not what the clients point at yet.** `storm-money-backend` and
`storm-money-web` still use the second generation above. A v3 event that
the indexer doesn't decode would be worse than not indexing it, so the
order is: backend learns the new events → point the backend at the new
ID → point the web client at it. Until then, v3 is a verified deployment,
not the live game.

## Personas (second generation only)

Chosen once at `join(player, persona)`, fixed for the life of the game —
a real lever on the contract's own numbers, enforced on-chain, not
client-side flavor. A player's name/avatar is cosmetic and lives in the
browser client (`storm-money-web`'s `Operator` storage), not on the
contract, since it has no gameplay effect and doesn't need to cost
contract storage.

| Persona | Effect |
|---|---|
| `Classic` | No bonuses, no penalties — the original economy. |
| `Hustler` | Side Hustle pays 5 instead of 3. |
| `Saver` | Expenses are 1 lower every round. |
| `Builder` | Stall costs 6 instead of 8; Business costs 18 instead of 24. |
| `Gambler` | Side Hustle is a coin flip: 0 or 6 (same average as the flat 3, far more variance). |

## Round lifecycle

There are two paths, and both stay available on purpose.

**One transaction per round (v3).**

1. `join(player, persona)` — once, registers a fresh game for that
   address with the chosen persona locked in.
2. `play_round(player, actions)` — roll, payday, bankruptcy, then up to
   two actions, all in one signed transaction. If the roll makes a chosen
   action unaffordable, the transaction still succeeds, publishes
   `ActionSkipped`, leaves that slot unspent, and stops the list there.

**One call per step (v2, still supported).**

1. `join(player, persona)`.
2. `roll_and_pay(player)` — event die, then payday, then bankruptcy
   liquidation. Must be called once at the start of every round.
3. `take_action(player, action)` — `BuyStall` / `BuyBusiness` /
   `SideHustle`, called up to twice. The second call that empties
   `actions_left` advances the round (or, on round 20, finalizes the
   score) automatically.

Both paths share the same free actions and reads:

4. `take_loan` / `repay_loan` / `buy_insurance(player)` — free actions,
   callable any time between the roll and the end of that round.
5. `get_state(player)` — read-only view of the full player struct
   (includes `persona`).
6. `get_config()` (v3) — the economy's numbers from the contract itself,
   so a client or indexer doesn't hardcode a copy that drifts on redeploy.

Every state change publishes an event — `PlayerJoined`, `RoundResolved`,
`ActionTaken`, `ActionSkipped`, `LoanTaken`, `LoanRepaid`,
`InsuranceBought`, `GameFinished` — so a player can reconstruct their
whole game from the event log instead of trusting a UI. That is the point
of putting this on-chain at all for a crypto-curious audience. (The first
two generations published nothing on `join`, and nothing at all for
loans/insurance; v3 closes that gap.)

## Try it

```bash
stellar keys generate me --network testnet --fund
ME=$(stellar keys address me)

stellar contract invoke --id CBO4POPU3FQXXYE353LKO6KNHT2QJRZGOFDTQEKELS3H2ZZBNEPF3SOE --source-account me --network testnet -- join --player "$ME" --persona Hustler
stellar contract invoke --id CBO4POPU3FQXXYE353LKO6KNHT2QJRZGOFDTQEKELS3H2ZZBNEPF3SOE --source-account me --network testnet -- roll_and_pay --player "$ME"
stellar contract invoke --id CBO4POPU3FQXXYE353LKO6KNHT2QJRZGOFDTQEKELS3H2ZZBNEPF3SOE --source-account me --network testnet -- take_action --player "$ME" --action SideHustle
stellar contract invoke --id CBO4POPU3FQXXYE353LKO6KNHT2QJRZGOFDTQEKELS3H2ZZBNEPF3SOE --source-account me --network testnet -- take_action --player "$ME" --action SideHustle
stellar contract invoke --id CBO4POPU3FQXXYE353LKO6KNHT2QJRZGOFDTQEKELS3H2ZZBNEPF3SOE --source-account me --network testnet --send=no -- get_state --player "$ME"
```

(`--persona` accepts `Classic` / `Hustler` / `Saver` / `Builder` /
`Gambler`. Add `--network testnet` identities with `stellar keys
generate`.)

On the v3 deployment, the same round is one call:

```bash
stellar contract invoke --id CDRJL2JJEFNL6DZSA5HROPVSOIMLUI2374ZZSY7LATLG5YM7DBH4BUJ5 --source-account me --network testnet -- play_round --player "$ME" --actions '["SideHustle","SideHustle"]'
stellar contract invoke --id CDRJL2JJEFNL6DZSA5HROPVSOIMLUI2374ZZSY7LATLG5YM7DBH4BUJ5 --source-account me --network testnet --send=no -- get_config
```

## Honest limitations

- **Randomness is predictable.** `env.prng()` is seeded from ledger
  state — deterministic and gameable, not a VRF. Left in deliberately:
  for this audience, "why is on-chain randomness hard" is a real lesson,
  not a bug to hide. Don't point this contract at anything where a
  predictable roll has value attached. (Gambler's Side Hustle coin flip
  uses the same predictable source — same caveat applies to it.)
- **A full 20-round game is still 21+ signed transactions on v3** (one
  `play_round` per round, plus any loans/insurance) and 60+ on the
  generation the clients currently use (1 roll + 2 actions × 20). That's
  a real, felt cost compared to the JS prototype's instant taps — which
  is itself the pedagogical contrast with Bitcoin's much slower, much
  more expensive settlement. Folding a round into one transaction
  reduces the click count; it doesn't remove the lesson that every step
  is a real, fee-paying transaction.
- **No persona "respec."** The choice is locked in at `join()` with no
  way to change it mid-game, on purpose — same spirit as the economy
  numbers themselves being fixed for the life of a game.
- **The browser client lives in a separate repo**,
  `Storm-Money/storm-money-web` (real direct signing, no wallet
  extension, with a persona/name/avatar creation step ahead of `join()`)
  — see that repo's README for its own verification trail. The indexer
  is its own repo too, `Storm-Money/storm-money-backend`.
- **Rebuilding/redeploying** after an economy.json-driven number change
  requires manually updating the Rust constants at the top of `lib.rs`
  — they're not (yet) read from `economy.json` at build time the way
  `sim.py` and `prototype.html` are. A build script that codegens the
  constants from `economy.json` would close that gap; not done here.
