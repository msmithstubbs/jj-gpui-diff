# AGENTS.md

## Project purpose

`jj-gpui-diff` is a deliberately small, read-only macOS app. It launches a native GPUI window and renders the Jujutsu working-copy changeset (`@` versus `@-`) as an inline text diff.

## Important behavior

- The repository is the process **current working directory**, not the source or executable directory.
- Do not add a Git backend: Jujutsu is the sole source of repository data.
- Jujutsu commands are invoked in `src/main.rs` with `std::process::Command`:
  - `jj root`
  - `jj diff -r @ --name-only`
  - `jj file show -r @- -- <path>` and `jj file show -r @ -- <path>`
- The app is read-only. Do not run mutating `jj` commands.
- File content is diffed locally with `imara-diff` using `Algorithm::Histogram` followed by `postprocess_lines()`, matching Zed's line-diff direction.
- Binary/non-UTF-8 files must remain safely non-rendered; the UI should show a marker instead of attempting a text diff.

## Development

The local default Cargo toolchain may be old. Use stable explicitly:

```sh
cargo +stable check
cargo +stable test
cargo +stable run --manifest-path "$PWD/Cargo.toml"
```

The final command only uses the desired Jujutsu checkout when it is invoked *from that checkout*. For example:

```sh
cd /path/to/jj-checkout
cargo +stable run --manifest-path /path/to/jj-gpui-diff/Cargo.toml
```

GPUI uses Metal on macOS. If the build reports that `metal` is unavailable, install Xcode's Metal component:

```sh
xcodebuild -downloadComponent MetalToolchain
```

## UI guidance

- Keep the interface dark, compact, and code-focused.
- Preserve GPUI's native scroll rendering; do not add a webview.
- Keep line-number alignment and the green/red added/deleted semantics.
- Long unchanged sections are intentionally summarized with three context lines at each side (`CONTEXT_LINES`).
- A refresh may currently perform synchronous Jujutsu calls. If the app grows, move repository work off the UI thread while keeping state updates on GPUI's app context.

## Validation

At minimum run:

```sh
cargo +stable check
cargo +stable test
```
