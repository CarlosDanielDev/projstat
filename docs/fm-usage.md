# fm usage — getting right Rust out of a weak model

`fm` here = `system` (Apple AFM-3 Core, on-device, ~3B). `pcc` is unavailable
in this context. It is **weak at Rust and does not know ratatui 0.30's API** —
it will hallucinate old widget APIs. Treat it like an untrusted junior who only
knows standard-library Rust.

## What fm may write (and nothing else)

Pure, `std`-only functions. No `ratatui`, no `tokio`, no `serde`, no `gh`
shelling, no async, no I/O. Good targets in projstat:

- momentum math: `fn momentum(closed_7d, opened_7d) -> i32`
- pick score + the edge-case normalization (clamp negatives, div-by-zero guard)
- duration humanizer: seconds/age → `"2h"`, `"1d"`, `"23d"`
- progress-bar string: `(pct, width) -> "██████░░░░"`
- sort comparators / key extraction
- number/count humanizers

Everything touching ratatui, tokio, serde, or `gh` → the main agent writes it.

## The rule for every fm call

1. **One function per call.** Never a module, never "wire these together".
2. Give the **exact signature** it must use, 2–3 input→output examples, and the
   behavior in one line.
3. End with: *"Return ONLY the function in one ```rust block. std only. No
   prose, no comments, no tests."*
4. **Verify before trusting:** `cargo check` + run a unit test the main agent
   wrote (not fm). fm never writes its own test.
5. **Two strikes:** wrong twice → main agent writes it. No retry loops.

## Template

```
fm respond 'You are writing one Rust function. Rust 2024 edition, std only, no
external crates.
Signature (use exactly):
    fn <name>(<args>) -> <ret>
Behavior: <one line>.
Examples: <call>=<result>; <call>=<result>.
Return ONLY the function in one ```rust block. No prose, no comments, no tests.'
```

The unit test is the contract. If fm output passes the main-agent-written test
after `cargo check`, keep it; else discard and write by hand.
