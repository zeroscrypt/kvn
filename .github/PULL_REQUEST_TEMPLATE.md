<!-- Title must be a Conventional Commit: feat|fix|docs|…(scope): summary -->

## What and why

<!-- Brief description of the change and the motivation. -->

## How it was tested

<!-- Commands run, platforms touched, live checks performed. -->

## Checklist

- [ ] PR title is a Conventional Commit (CI enforces it)
- [ ] Commits are signed off (`git commit -s`, DCO)
- [ ] `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace` and `pnpm build` pass locally
- [ ] User-visible strings exist in both RU and EN (`apps/desktop/src/i18n.ts`)
- [ ] Docs updated when behaviour changed (README / docs / ADR)
