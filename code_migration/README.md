# yiffy-corner-bot (Rust)

The Rust implementation of the Yiffy Corner curator bot. Hexagonal + DDD;
`design/domain.md` (repo root) is the authoritative domain document.

## Crates

| crate | role |
|---|---|
| `domain` | entities, value objects, and every port (repositories + outbound gateways) |
| `application` | use cases (`/suggest`, moderation, …), the queue-first selector, the minute-tick scheduler |
| `persistence` | SQLite (sqlx, embedded migrations) and in-memory adapters |
| `infra-e621` | rate-limited e621 client (2 req/s): fetch, tag search, media resolution |
| `infra-fixup` | Twitter/X via the FixupX API, BlueSky via fxbsky embeds, DeviantArt/t.me link embeds |
| `infra-furaffinity` | FA page scrape (optional `a`/`b` session cookies for Mature/Adult) |
| `telegram_bot` | the binary: teloxide command surface, publisher, boot wiring |

## Running

Secrets live in the legacy vault layout: `config/vault/<env>/token.txt`
(FurAffinity cookies optional as `cookie_a.txt` / `cookie_b.txt`).

```sh
# from the repo root
YCB_ENV=development YCB_VAULT_DIR=config/vault cargo run -p telegram_bot --manifest-path code_migration/Cargo.toml
# or in a container
docker compose up bot-rust
```

Environment (all optional):

| var | default |
|---|---|
| `YCB_ENV` | `development` (vault subfolder) |
| `YCB_VAULT_DIR` | `config/vault` |
| `YCB_DATABASE_URL` | `sqlite:<vault>/storage/rust-bot.sqlite` |
| `YCB_OWNER_ID` | `1402476143` |
| `YCB_HEALTH_ADDR` | `0.0.0.0:3000` |
| `YCB_REPOST_COOLDOWN_DAYS` | `7` |

## First-run flow

1. `/start` the bot as the Owner.
2. `/newposter 15 wolf -gore` → creates Poster #1 (fires every 15 min).
3. `/setchannel 1 @yourchannel` (bot must be an admin of the channel).
4. Restart the bot — Poster runtimes are loaded at boot.
5. Anyone can `/suggest <url>`; Moderators approve via the DM buttons;
   `/browse wolf` + `/save <e621-url>` fills the tag-based pool.

## Logging

Logs are **JSON lines** on stdout (set `YCB_LOG_FORMAT=pretty` for the human
format; level via `RUST_LOG`, default `info,teloxide=warn`). Every line
carries an `event` field drawn from the closed vocabulary in
`crates/telemetry` — filter with `jq`:

```sh
docker logs yiffy_corner_bot_rust | jq 'select(.event == "published")'
docker logs yiffy_corner_bot_rust | jq 'select(.event == "submission_rejected") | {reason, user_id}'
```

Level convention: `error` = operation failed; `warn` = degraded/suspicious
(auth denial, fallback, empty pool); `info` = domain event (submission,
decision, publish, role change); `debug` = plumbing (upstream requests,
selector candidate walk, DM fan-out). Secondary classifications are enums
too: `reason` (`RejectReason`/`SkipReason`) and `upstream`
(e621/fxtwitter/fxbsky/furaffinity/telegram).

## Tests

```sh
cargo test --workspace                  # unit + adapter suites (offline)
cargo test --workspace -- --ignored     # live checks against e621/FixUp/FA
```
