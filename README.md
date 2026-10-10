# hat-execution-evidence

Record an admitted HAT execution result with exact references that other components can verify.

## What you can do

- Retain signed admission and bounded result evidence.
- Look up conflicts without mutating the evidence stream.

## Current scope

The host supplies worker authentication and signing custody. A crash between an external effect and durable evidence remains uncertain; exactly-once execution is not promised.

This reusable library is packaged independently for crates.io. Its declared library dependencies are available from the public registry; no private index or sibling source checkout is required.

## Getting started

Install Rust 1.97 or newer and make the declared dependencies available. All declared library dependencies resolve from crates.io. Run from this repository:

```sh
cargo test --locked
```

## Documentation and source

[Usage guide](docs/getting-started.md)

[Implementation and public interfaces](src) · [Verification cases](tests) · [Contributing](CONTRIBUTING.md) · [Security reporting](SECURITY.md) · [License](LICENSE) · [Attribution notices](NOTICE)

## Independent Cargo consumer

Add `hat-execution-evidence = "0.10.0"` to a Rust 1.97 application. Select only the documented features required by the caller. Storage locations, network authority and runtime orchestration are supplied explicitly by the application.
