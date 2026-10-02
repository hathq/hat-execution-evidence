# Using hat-execution-evidence

Record an admitted HAT execution result with exact references that other components can verify.

## Before you start

The host supplies worker authentication and signing custody. A crash between an external effect and durable evidence remains uncertain; exactly-once execution is not promised.

## First steps

Run from the repository root:

```sh
cargo test --locked
```

## How to assess the result

- Retain signed admission and bounded result evidence.
- Look up conflicts without mutating the evidence stream.

A passing source-level check establishes only what that check observes. Keep missing configuration, unavailable services and unverified deployment paths visible.

## Continue reading

[Repository overview](../README.md)
