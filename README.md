# jj-gpui-diff

A small native macOS diff viewer built with Zed's public **GPUI** crate. It reads the Jujutsu working-copy changeset (`@` compared with its parent), then computes each text-file diff locally with `imara-diff`'s `Histogram` algorithm and line post-processing—the same diff engine and processing direction used by Zed.

The display is a scrollable unified or split diff: deleted lines are red, added lines are green, and long unchanged spans are collapsed to three lines of context on each side. **Refresh** reruns Jujutsu and recomputes the view. The interface uses the [GitHub Dark palette from the GitHub Zed theme](https://github.com/PyaeSoneAungRgn/github-zed-theme).

## Requirements

- macOS with Xcode and the Metal toolchain
- `jj` available on `PATH`
- current stable Rust (the included `rust-toolchain.toml` selects it)

If GPUI reports that `metal` is unavailable, install the Xcode component once:

```sh
xcodebuild -downloadComponent MetalToolchain
```

## Build and run

Build from this directory:

```sh
cargo +stable build --release
```

The executable deliberately uses its **current working directory**, not the directory that contains the executable. Run it from a Jujutsu checkout:

```sh
cd /path/to/jj-checkout
/path/to/2026-08-14-stratton/target/release/jj-gpui-diff
```

During development, retain the Jujutsu checkout as the shell's working directory:

```sh
cd /path/to/jj-checkout
cargo +stable run --manifest-path /path/to/2026-08-14-stratton/Cargo.toml
```

## How it gets the diff

1. `jj root` resolves the repository from the launch directory.
2. `jj diff -r @ --name-only` gets modified paths.
3. For each path, `jj file show -r @-` and `jj file show -r @` load the parent and current contents.
4. `imara-diff` calculates and post-processes line hunks. GPUI renders the hunks natively with Metal on macOS.

It is intentionally read-only. Binary or non-UTF-8 file contents are marked, but are not rendered as text.

## Project layout

- `src/main.rs` — Jujutsu process calls, imara-diff hunk-to-row conversion, and the GPUI view.
- `Cargo.toml` — native GPUI and imara-diff dependencies.
- `AGENTS.md` — implementation constraints and future-session notes.

## Verify

```sh
cargo +stable check
cargo +stable test
```
