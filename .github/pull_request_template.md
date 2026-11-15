## What and why

<!-- What this changes, and the issue it closes (Closes #...). -->

## How it was tested

<!-- What you ran, and what you could not run. -->

## Checklist

- [ ] The change is narrow: it fixes what the issue describes, nothing unrelated.
- [ ] A fix comes with a test that fails without it (a crash fix with a test that panicked before).
- [ ] No `unwrap`, `expect`, `panic!` or `unsafe` in non-test code.
- [ ] Nothing copied from another product; new dependencies and assets are licensed per AGENTS.md, assets listed in ATTRIBUTION.md.
- [ ] `cargo xtask ci` passes, or I said above what I could not run.
- [ ] The docs that this change affects are updated.
