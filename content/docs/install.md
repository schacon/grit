---
title: Install grit
summary: Install the grit CLI on your machine, try nightly builds, and keep your copy up to date.
---

## Stable release (recommended)

On macOS and Linux:

```bash
curl -fsSL https://grit-scm.com/install | sh
```

On Windows, run this in PowerShell:

```bash
irm https://grit-scm.com/install.ps1 | iex
```

The script downloads the latest release from GitHub, installs the `grit` binary (by default into `~/.local/bin` on Unix), and prints the version it installed.

## Linux GNU loader compatibility

Linux x86_64 releases provide two GNU assets. Both preserve PIE and ASLR:

| Asset | Loader support |
| --- | --- |
| `grit-x86_64-unknown-linux-gnu.tar.gz` | Default and fallback; glibc 2.34 or newer, standard relocations |
| `grit-x86_64-unknown-linux-gnu-relr.tar.gz` | Optional faster startup; glibc 2.36 or newer, packed relative relocations (RELR) |

The stable and nightly installers select RELR only when `getconf GNU_LIBC_VERSION`
(or, if inconclusive, the first line of `ldd --version`) confidently identifies
glibc 2.36 or newer. Older, unknown or malformed versions use the default asset.
If the optional asset is unavailable, the installer retries the default asset;
this also supports releases from before RELR was added. Manual downloads keep
the existing GNU asset as the default. Its weak glibc 2.39 references do not
require a newer loader.

The RELR link improved small-command startup by **5–8%** in the initial
shipping `dist` measurement, with **4–5%** for branch/status/resolve in a fresh
Xeon check; see [benchmarks](benchmarks.md#pie-startup-relocation-probe)
for timings and memory limits. GNU aarch64, static musl, macOS and Windows keep
their existing artifacts and selection. Static musl artifacts and source builds
remain alternatives for other systems.

## Shell completions

Tab completion helps you discover `grit`'s subcommands and flags. Generate a script with [`grit completions`](../completions/) and load it from your shell config, or copy the pre-generated files from a release tarball's `completions/` directory.

**bash** — add to `~/.bashrc`:

```bash
eval "$(grit completions bash)"
```

**zsh** — add to `~/.zshrc`:

```bash
eval "$(grit completions zsh)"
```

Or install once under a site-functions directory:

```bash
grit completions zsh > ~/.local/share/zsh/site-functions/_grit
```

**fish** — write a completion file:

```bash
mkdir -p ~/.config/fish/completions
grit completions fish > ~/.config/fish/completions/grit.fish
```

After [`grit update`](../update/), regenerate completions so new commands appear.

## Nightly builds

CI publishes a rolling **nightly** prerelease when the release workflow is triggered manually. To install that build instead of the latest stable release:

```bash
curl -fsSL https://grit-scm.com/install-nightly | sh
```

On Windows:

```bash
irm https://grit-scm.com/install-nightly.ps1 | iex
```

Nightlies track recent `main`; use them when you want to test fixes before the next tagged release.

## Install with Cargo

If you already have a Rust toolchain:

```bash
cargo install grit-cli
```

This builds and installs the `grit` executable into Cargo's bin directory (usually `~/.cargo/bin`). Make sure that directory is on your `PATH`.

## Keep grit up to date

After a script install, run [`grit update`](../update/) to re-run the same installer and replace the binary you're running. If you installed with Cargo, update with:

```bash
cargo install grit-cli
```

## Build from source

Clone [the repository](https://github.com/gitbutlerapp/grit) and build the CLI crate:

```bash
cargo build --release -p grit-cli
```

The binary is `target/release/grit`. You can copy it anywhere on your `PATH`, or run it from the build tree.

### Toolchain

The workspace pins a Rust version in [`rust-toolchain.toml`](https://github.com/gitbutlerapp/grit/blob/main/rust-toolchain.toml) (currently **1.99.0** with `rustfmt` and `clippy`). `rustup` picks this up automatically when you build in the repo.

To use `grit-lib` in your own crate, add it from [crates.io](https://crates.io/crates/grit-lib) — see the [library quick start](../library-quickstart/).
