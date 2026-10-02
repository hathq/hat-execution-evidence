# hat-execution-evidence

Record an admitted HAT execution result with exact references that other components can verify.

## What you can do

- Retain signed admission and bounded result evidence.
- Look up conflicts without mutating the evidence stream.

## Current scope

The host supplies worker authentication and signing custody. A crash between an external effect and durable evidence remains uncertain; exactly-once execution is not promised.

Package distribution is not activated by this documentation. Use the checked-in source and the declared dependency versions; published availability must be verified separately.

## Getting started

Install Rust 1.97 or newer and make the declared dependencies available. Use the configured private registry when a dependency is not distributed publicly. Run from this repository:

```sh
cargo test --locked
```

## Documentation and source

[Usage guide](docs/getting-started.md)

[Implementation and public interfaces](src) · [Verification cases](tests) · [Contributing](CONTRIBUTING.md) · [Security reporting](SECURITY.md) · [License](LICENSE) · [Attribution notices](NOTICE)
