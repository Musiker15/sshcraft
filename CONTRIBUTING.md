# Contributing to SSHCraft

Thanks for helping. SSHCraft is an SSH client that holds the keys to every server its user has,
so a few rules are stricter here than in most projects. The full set is in
[AGENTS.md](AGENTS.md); it applies to people and to coding agents alike.

## Before you start

1. Read [ROADMAP.md](ROADMAP.md) and [docs/gaps.md](docs/gaps.md) to see what is planned and
   what is open.
2. Look for an issue. Read its comments, search the pull requests for its number and keywords,
   and check the recent commits on `main`, so two people don't fix the same thing.
3. Say on the issue that you take it. If you stop, say so too.

A security problem never goes into a public issue: see [SECURITY.md](SECURITY.md).

## Making the change

- **Keep it narrow.** Fix what the issue describes, where the bug is. Nothing unrelated in the
  same pull request.
- **Test it.** Every fix comes with a test that fails without the fix. A crash fix comes with a
  test that panicked before.
- **Never crash.** No `unwrap()`, `expect()`, `panic!` or `unsafe` in non-test code (the
  exceptions are listed in AGENTS.md). Treat every input as hostile: import files, server
  responses, terminal output.
- **Everything is a command.** User-visible behaviour is a command in `crates/engine/src/cmd/`;
  the window, the CLI, the control channel and MCP all go through it.
- **Layers.** A crate depends only on lower layers (`cargo xtask layers` checks it).
- **Clean room.** Nothing copied from Termius or any other product: no code, icons, artwork,
  fonts, themes or wording. No GPL, AGPL or LGPL code.
- **Dependencies and assets** need a licence from the allowlist in AGENTS.md. Every asset gets a
  row in [ATTRIBUTION.md](ATTRIBUTION.md) in the same change. Font files never go into this
  repository.
- **CI stays shared.** Don't change the workflows in a feature or fix pull request.

## Checking it

```bash
cargo xtask ci
```

That runs formatting, clippy with `-D warnings`, the tests, the layer and asset checks,
`cargo deny` and the wasm check. While you work, scope it to your crate:
`cargo clippy -p <crate> --all-targets -- -D warnings` and `cargo test -p <crate>`.

## The pull request

- `main` changes only through a pull request; nobody pushes to it.
- Review your own diff first: the names you use exist, the test fails without the fix, nothing
  unrelated changed, and the description is true.
- Say what you tested and what you could not run.
- If another pull request fixes the same thing as well or better, close yours and say why.
- Answer review feedback within 7 days, or say when you will.
- A pull request does not say that an agent wrote it. Whoever opens it answers for it.

## Being credited

`contributors/people.toml` holds the names people chose for themselves. Add only your own entry,
if you want more than your GitHub username shown. Never edit `contributors/contributors.json`;
it is generated.

## Licence

By contributing you agree that your contribution is licensed under either of
[MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at the user's option, like the rest of
SSHCraft.

Please also follow the [code of conduct](CODE_OF_CONDUCT.md).
