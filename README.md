# Yiffy Corner Bot

A single-tenant Telegram curator for furry art: users submit by source URL
(e621, FurAffinity, Twitter/X, BlueSky, DeviantArt, t.me forwards),
moderators curate into one ordered feed, and per-channel Posters consume it
BSky-style with their own tag subscriptions and cursors.

Rust workspace in [`code_migration/`](code_migration/) — see
[its README](code_migration/README.md) for architecture, commands, logging,
and the production runbook. `design/domain.md` is the authoritative domain
document.

Quick start on a fresh machine:

```sh
just setup   # provision secrets interactively
just start   # build + run (docker compose, detached)
```
