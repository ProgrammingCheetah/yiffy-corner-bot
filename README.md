# Yiffy Corner Bot

A single-tenant Telegram curator for furry art: users submit by source URL
(e621, FurAffinity, Twitter/X, BlueSky, DeviantArt, t.me forwards),
moderators curate into one ordered feed, and per-channel Posters consume it
BSky-style with their own tag subscriptions and cursors. Hexagonal + DDD;
`design/domain.md` is the authoritative domain document.

Quick start: `just setup` (interactive secrets) then `just start`.

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
(see `just setup`).

```sh
# from the repo root
YCB_ENV=development cargo run -p telegram_bot
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

## The feed model

All curated posts live in ONE ordered feed (BSky-style). Approving a
submission — or saving from `/browse` — assigns it the next feed position.
Every Poster (consumer) stores its tag subscription and a cursor: each fire
scans forward from the cursor and posts the first entry matching its tags,
advancing the cursor to the match (or to the pre-scan feed end on a miss —
appends during a scan are never skipped). Consume-once: when a consumer
reaches the feed end it stays quiet until new content is curated.

Every feed entry is tagged: e621 tags come from the API; other sources
require submitter tags — inline (`/suggest <url> wolf male`) or via the
ask-and-wait dialogue (the bot prompts, your next message is the tags).
Channel forwards always go through the tag dialogue.

## First-run flow

1. `/start` the bot as the Owner.
2. `/newposter 15 @yourchannel wolf -gore` — creates Poster #1 bound to the
   channel (bot must be a channel admin), live within a minute.
3. Fill the feed: `/browse wolf` + Send buttons, or `/suggest <url> [tags…]`
   from anyone + Moderator approval via the DM buttons.
4. Optional: `/announcements 24` for the recurring channel directory,
   `/spoilertag`//`/listtags` for content policy, `/postinfo <id>` to audit.

## Production deployment

On the target machine:

```sh
git clone https://github.com/ProgrammingCheetah/yiffy-corner-bot.git
cd yiffy-corner-bot

# Provision the vault (NOT in git) — create these files:
#   config/vault/production/token.txt        Telegram bot token
#   config/vault/production/e621_login.txt   e621 username        (optional)
#   config/vault/production/e621_key.txt     e621 API key         (optional)
#   config/vault/production/cookie_a.txt     FA session cookie a  (optional)
#   config/vault/production/cookie_b.txt     FA session cookie b  (optional)
mkdir -p config/vault/production config/vault/storage

docker compose up -d --build bot-rust
```

That single `docker compose up -d --build bot-rust` is the production
command: it builds the image, runs migrations against
`config/vault/storage/rust-bot.sqlite` (persisted via the volume mount),
and starts polling. `docker logs -f yiffy_corner_bot_rust` streams the
JSON log. Update = `git pull && docker compose up -d --build bot-rust`.

State (users, feed, cursors, posters) lives in the mounted sqlite file —
back up `config/vault/` and you have everything.

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


## Mini App (Telegram WebApp)

Everything the commands do, in an app — including Tinder-style moderation
(swipe right = approve, left = reject, with reason / extra-tags buttons).

Setup, once:

1. **Tunnel**: Cloudflare Zero Trust → Networks → Tunnels → create one, add a
   public hostname (e.g. `app.got-paws.net`) pointing at
   `http://bot-rust:3000`, and copy the tunnel token.
2. **Env**: in the repo root create `.env`:

   ```
   TUNNEL_TOKEN=eyJ…
   YCB_WEBAPP_URL=https://app.got-paws.net
   ```

3. **Run**: `just start-tunnel` (instead of `just start`). The bot registers
   its own menu button, so the app appears in every private chat with it.

The SvelteKit bundle in `webapp/build/` is committed — production needs no
node. After editing `webapp/src`, rebuild it with `just webapp` and commit.

## Desktop userscript

`tools/ycb-submit.user.js` (Tampermonkey) adds a 🐾 Submit button on
e621/e926 posts, FurAffinity views, Twitter/X statuses and BlueSky posts.
e621 submits straight away (tags come from the API); the others prompt for
tags (`artist:<name>` credits the artist). Auth: run `/apitoken` in the bot,
then userscript menu → *Set API token*.
