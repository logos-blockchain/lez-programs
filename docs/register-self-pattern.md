# `register_self`: chained-call deployer authorization

A pattern for programs that need a *deployer-verified* registry entry —
on-chain proof that an entry was written by the program it describes,
not by a third party claiming the name.

## The problem

`#[account(signer)]` can express "this user authorized this", but it
cannot express "the program did this". Program deployment is an unsigned
transaction and a program id has no keypair, so there is no signature to
check. The only on-chain attestation available is `caller_program_id`,
which the state machine sets to the *calling* program inside a chained
call (and to all-zeros, `DEFAULT_PROGRAM_ID`, for a top-level user call).

## The pattern

```
user tx ──▶ program.register_self ──chained call──▶ registry.register_deployer
                                                     (caller == program → accept)
```

1. **The callee rejects anything that is not a self-chained call.**
   `register_deployer` checks `ctx.caller_program_id` twice: it must not
   be `DEFAULT_PROGRAM_ID` (which would mean a direct top-level call),
   and it must equal the program id being registered — so a chained call
   naming a *different* program is rejected too.

2. **The program exposes a `register_self` hook** that emits a
   `ChainedCall` back into the registry carrying `register_deployer` for
   itself. The hook guards on `ctx.self_program_id == program` and
   recomputes the expected entry PDA so the call can only ever create
   *this* program's deployer entry.

Because `register_deployer` has no signer requirement and refuses direct
calls, the deployer entry can only ever come into existence via the
registered program's own execution.

## SPEL gotchas

Three non-obvious constraints we hit implementing this:

- **A foreign entry PDA must be a plain mention, not `#[account(init)]`.**
  `#[account(init, pda = ...)]` auto-generates the init claim in *this*
  instruction — but the init must be claimed by the callee
  (`register_deployer`), not the caller (`register_self`). Declare the
  entry account as a plain `AccountWithMetadata` in the hook and put the
  `#[account(init, pda)]` only on the callee.

- **`post_states.len() == pre_states.len()`.** The sequencer requires the
  post-state list to be the same length as the pre-state list, so the
  entry account declared in `register_self` must also appear in its
  `SpelOutput::execute` post-states — passed through unchanged. The real
  write happens in the callee.

- **`caller_program_id` is all-zeros for top-level calls.** Check
  `caller_program_id != DEFAULT_PROGRAM_ID` *and* `caller_program_id ==
  program` — either check alone is insufficient (the first alone admits
  chained calls naming a foreign program; the second alone admits
  nothing, since a direct call is all-zeros, not the program).

## Reference implementation

[`TerexitariusStomp/widespread-logos`](https://github.com/TerexitariusStomp/widespread-logos)
ships the pattern end-to-end:

- `programs/registry` — the registry program (`register_deployer` with
  the double caller check, `register_self`, `update_deployer_entry`,
  `register_third_party` for unattributed entries)
- `programs/testimonial`, `programs/pointer` — consumer programs with
  their own `register_self` hooks
- `programs/ARTIFACTS.md` — live testnet verification, including the
  negative test: a direct `register_deployer` call naming a foreign
  program is rejected by the caller check
