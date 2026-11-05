# Storm Money on Stellar

The economy as on-chain state instead of a JS object only the host's
browser can see. Same round order and numbers as `sim.py` /
`prototype.html` (post-rental-cut: `stall` 8→+1/rd, `business` 24→+4/rd),
reimplemented as a Soroban contract in `contracts/storm-economy`.

## Deployed on testnet

```
Contract ID: CABDVHFVUJSNTWLHYWWSOWKIZS6OYFOSRLQNXPXI2OICFSHZCNMG4RAT
Wasm hash:   ff1ae8d4ca9f1c1487677a182b65553d1cd8179ca177ef6a366f6ffcb1bce2cd
Explorer:    https://lab.stellar.org/r/testnet/contract/CABDVHFVUJSNTWLHYWWSOWKIZS6OYFOSRLQNXPXI2OICFSHZCNMG4RAT
```

Verified live, not just locally: `join`, `roll_and_pay`, `take_action`,
`take_loan`/`repay_loan` all round-tripped against real testnet
transactions (not just `cargo test`), including a real `Emergency` roll
whose math checked out exactly (10 start + 8 salary − 5 expenses − 12
emergency = 1), and a real rejected transaction (`take_action` called
before that round's roll, correctly failing simulation with
`Error::MustRollFirst` before any fee was spent).

## Round lifecycle (one call per step, each a real signed transaction)

1. `join(player)` — once, registers a fresh game for that address.
2. `roll_and_pay(player)` — event die, then payday, then bankruptcy
   liquidation. Must be called once at the start of every round.
3. `take_action(player, action)` — `BuyStall` / `BuyBusiness` /
   `SideHustle`, called up to twice. The second call that empties
   `actions_left` advances the round (or, on round 20, finalizes the
   score) automatically.
4. `take_loan` / `repay_loan` / `buy_insurance(player)` — free actions,
   callable any time between step 2 and the end of that round.
5. `get_state(player)` — read-only view of the full player struct.

Every state change publishes an event (`RoundResolved`, `ActionTaken`,
`GameFinished`) so a player can reconstruct their whole game from the
event log instead of trusting a UI — the point of putting this on-chain
at all for a crypto-curious audience.

## Try it

```bash
stellar keys generate me --network testnet --fund
ME=$(stellar keys address me)

stellar contract invoke --id storm-economy --source-account me --network testnet -- join --player "$ME"
stellar contract invoke --id storm-economy --source-account me --network testnet -- roll_and_pay --player "$ME"
stellar contract invoke --id storm-economy --source-account me --network testnet -- take_action --player "$ME" --action SideHustle
stellar contract invoke --id storm-economy --source-account me --network testnet -- take_action --player "$ME" --action SideHustle
stellar contract invoke --id storm-economy --source-account me --network testnet --send=no -- get_state --player "$ME"
```

(`storm-economy` resolves via the local alias saved at deploy time; add
`--network testnet` identities with `stellar keys generate`.)

## Honest limitations

- **Randomness is predictable.** `env.prng()` is seeded from ledger
  state — deterministic and gameable, not a VRF. Left in deliberately:
  for this audience, "why is on-chain randomness hard" is a real lesson,
  not a bug to hide. Don't point this contract at anything where a
  predictable roll has value attached.
- **A full 20-round game is 60+ signed transactions** (1 roll + 2
  actions × 20, plus any loans/insurance). That's a real, felt cost
  compared to the JS prototype's instant taps — which is itself the
  pedagogical contrast with Bitcoin's much slower, much more expensive
  settlement (see `../bitcoin/README.md` once that lands).
- **No UI yet.** This is the contract layer only; a frontend would use
  Freighter/Wallet Kit instead of CLI identities, per the `dapp` skill.
- **Rebuilding/redeploying** after an economy.json-driven number change
  requires manually updating the Rust constants at the top of `lib.rs`
  — they're not (yet) read from `economy.json` at build time the way
  `sim.py` and `prototype.html` are. A build script that codegens the
  constants from `economy.json` would close that gap; not done here.
