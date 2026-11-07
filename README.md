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
asserted by `cargo test` (13 passing unit tests, including one gambler
run and one classic-baseline run per persona).

## Personas (second generation only)

Chosen once at `join(player, persona)`, fixed for the life of the game —
a real lever on the contract's own numbers, enforced on-chain, not
client-side flavor. A player's name/avatar is cosmetic and lives in the
browser client (`web/index.html`'s `Operator` storage), not on the
contract, since it has no gameplay effect and doesn't need to cost
contract storage.

| Persona | Effect |
|---|---|
| `Classic` | No bonuses, no penalties — the original economy. |
| `Hustler` | Side Hustle pays 5 instead of 3. |
| `Saver` | Expenses are 1 lower every round. |
| `Builder` | Stall costs 6 instead of 8; Business costs 18 instead of 24. |
| `Gambler` | Side Hustle is a coin flip: 0 or 6 (same average as the flat 3, far more variance). |

## Round lifecycle (one call per step, each a real signed transaction)

1. `join(player, persona)` — once, registers a fresh game for that
   address with the chosen persona locked in.
2. `roll_and_pay(player)` — event die, then payday, then bankruptcy
   liquidation. Must be called once at the start of every round.
3. `take_action(player, action)` — `BuyStall` / `BuyBusiness` /
   `SideHustle`, called up to twice. The second call that empties
   `actions_left` advances the round (or, on round 20, finalizes the
   score) automatically.
4. `take_loan` / `repay_loan` / `buy_insurance(player)` — free actions,
   callable any time between step 2 and the end of that round.
5. `get_state(player)` — read-only view of the full player struct
   (includes `persona`).

Every state change publishes an event (`RoundResolved`, `ActionTaken`,
`GameFinished`) so a player can reconstruct their whole game from the
event log instead of trusting a UI — the point of putting this on-chain
at all for a crypto-curious audience. (`join()` itself still publishes no
event — same gap as the first generation, see `backend/README.md`.)

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

## Honest limitations

- **Randomness is predictable.** `env.prng()` is seeded from ledger
  state — deterministic and gameable, not a VRF. Left in deliberately:
  for this audience, "why is on-chain randomness hard" is a real lesson,
  not a bug to hide. Don't point this contract at anything where a
  predictable roll has value attached. (Gambler's Side Hustle coin flip
  uses the same predictable source — same caveat applies to it.)
- **A full 20-round game is 60+ signed transactions** (1 roll + 2
  actions × 20, plus any loans/insurance). That's a real, felt cost
  compared to the JS prototype's instant taps — which is itself the
  pedagogical contrast with Bitcoin's much slower, much more expensive
  settlement (see `../bitcoin/README.md` once that lands).
- **No persona "respec."** The choice is locked in at `join()` with no
  way to change it mid-game, on purpose — same spirit as the economy
  numbers themselves being fixed for the life of a game.
- **A live web client now exists** at `web/` (real Freighter-free direct
  signing, per the `dapp` skill, with a persona/name/avatar creation step
  ahead of `join()`) — see `web/README.md` for its own verification
  trail.
- **Rebuilding/redeploying** after an economy.json-driven number change
  requires manually updating the Rust constants at the top of `lib.rs`
  — they're not (yet) read from `economy.json` at build time the way
  `sim.py` and `prototype.html` are. A build script that codegens the
  constants from `economy.json` would close that gap; not done here.
