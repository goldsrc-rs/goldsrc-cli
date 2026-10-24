# GoldSrc Developer CLI (`grs`)

Official command-line developer tooling for the GoldSrc.rs WebAssembly plugin platform.

## Features

- **`grs new`**: Interactive or automated scaffolding for new GoldSrc plugins.
- **`grs build`**: Compile plugins to `wasm32-unknown-unknown` with size-optimization and strip stages.
- **`grs check`**: Contract and schema validation of `package.metadata.goldsrc` manifests.
- **`grs deploy`**: Hot-deploy binaries, configurations, assets, and language dictionaries to local HLDS installations.
- **`grs setup`**: Automated verification and environment setup for GoldSrc developers.

## Installation

```bash
cargo install --git https://github.com/goldsrc-rs/goldsrc-cli.git --branch dev
```

## License

Licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))
