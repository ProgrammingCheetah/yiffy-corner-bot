# yiffy-corner-bot — Rewrite Plan

Iterative implementation plan for the rewrite described in `DESIGN.md`. This file captures **decisions** about what the new implementation must do; the "how" lives alongside the code once written.

Each item should be specific enough that someone reading just this file (and `DESIGN.md`) could implement it without further clarification. Reference DESIGN.md sections (e.g. "see §5.2") wherever relevant rather than restating.

Status legend: `[ ]` planned · `[~]` in progress · `[x]` done · `[-]` dropped (with reason). Progress is tracked in this file.

---

## Additions (Features)

_New features, or completions of features that were unfinished in the original. Each item: what the feature is, who triggers it, what it does, and any acceptance criteria._

- [ ] **Hexagonal Design (Ports & Adapters + DDD).** Cargo workspace where the domain crate is pure (no I/O), the application crate defines port traits and use cases, and infra crates implement adapters. Aggregates are data-only structs; cross-aggregate interactions go through domain services. Repositories act on a single aggregate root. Transactions wrap multi-step state changes. Workspace map in **Notes → Workspace Layout**.

- [ ] **Comprehensive Testing.** Unit tests for every domain service and use case (using in-memory port fakes). Integration tests for adapters against real test dependencies (test SQLite file, mock HTTP server for e621). E2E tests for critical flows: e621 → curate → publish, multi-bot spawn, role-gated commands. Coverage target: 80% lines overall, 100% on domain services.

- [ ] **Telegram WebApp UI.** Replaces the bot's text/button interface entirely. Built in **Svelte** (small runtime, mature ecosystem). Authenticated via Telegram WebApp `initData` (HMAC-verified server-side). Only logged-in roles (Mod/Admin/Owner) can access — anyone else gets denied at the API gate.
  Capabilities:
    - **Smash-or-Pass curation** for e621 candidates: browse fresh API results one at a time; "Smash" enqueues, "Pass" skips. No animation — just two buttons.
    - **URL paste-and-confirm** for non-e621 sources (Twitter via fxtwitter, Bluesky, FurAffinity): operator pastes a URL, the bot fetches metadata + media, shows a preview confirmation card, and queues only after explicit approval.
    - View current queue, remove items, **reorder** items (drag-and-drop), trigger immediate publish.
    - Clear instructions and feedback for every action (loading states, success/error toasts).

- [ ] **Multi-Bot Spawning.** Owner-only `/add_bot` command (and matching WebApp form) that registers a new bot. Each registered bot runs as its own `tokio` task within the same process, sharing the domain/application layer. Bot configs live in **per-bot TOML files** under `config/bots/<name>.toml` (chosen over DB storage for inspectability and recoverability). A bot config holds: token (from env var ref), channel handle, default tags, forbidden tags, post cadence, repost window override, caption template. Loaded at startup; new bots start their task on `/add_bot`.

- [ ] **Per-Channel Tag Configuration.** Each spawned bot has its own default and forbidden tag sets, plus a **globally-banned** tag set that no channel can override (operator-level safety net). The forbidden-tag check at publish time applies the union of global + per-channel.

- [ ] **Multi-Source Ingestion.** Sources: e621 (browse via API), FurAffinity (URL paste; cookie-auth required), Twitter via fxtwitter (URL paste), Bluesky (URL paste). Non-e621 sources are **single-post only** — they don't carry rich tags so they never re-circulate. Only e621 posts have tag metadata stored and are eligible for re-circulation. All sources are perceptually hashed at ingest (see Repost Resistance).

- [ ] **Repost-Resistance Algorithm.** Two layers, both must pass:
    1. **Perceptual hash check.** Every published image gets a 64-bit pHash (or dHash) stored in DB. Before publishing, hash the candidate and reject if Hamming distance to any stored hash is below threshold (initial tune: 6 bits for 64-bit hash). No ML — the `image-hasher` / `img_hash` crate is sufficient.
    2. **Time-window check.** Only e621 posts re-circulate, and only after `repost_window_days` has elapsed since their `last_published_at`. Default 30 days. Configurable via owner-only `/setrepostwindow [days]`.

  Non-e621 posts are still hashed (so the same image reposted from Twitter then e621 is caught) but their post records carry a `recirculate_eligible = false` flag.

- [ ] **Caption / Layout Parser.** Template parser with placeholders (`{title}`, `{description}`, `{artists}`, `{tags}`, `{sources}`, `{post_id}`, `{channel}`). Templates stored per-channel so each spawned bot can have its own caption style. Default templates per source type (e621 vs URL-paste). MarkdownV2 escaping is applied to substituted values automatically.

- [ ] **Redis for Cross-Cutting State.** Use Redis for: rate-limiting (per-user UI calls, per-bot Telegram-API quota), short-lived caches (e621 search results, fxtwitter responses), and ephemeral locks (single-flight publish per channel). Persistent state stays in SQL.

- [ ] **Roles & Permissions (Mod / Admin / Owner).** Roles are attached to **Telegram numeric user IDs**.
    - **Moderator**: delete from queue, request channel-message delete, trigger publish.
    - **Admin**: everything Mod can do, plus reorder queue, plus add/remove Mods.
    - **Owner**: everything Admin can do, plus add/remove Admins, plus add bots, plus configure per-channel settings, plus view blame log.

  All three roles can use the WebApp and add new content. Anyone without a role cannot access the WebApp at all.

- [ ] **Blame (Audit Log).** Append-only log of role-gated actions only:
    - Adding a post to the queue (who, when, source, perceptual hash, external id).
    - Adding/removing a member with a role (who added whom, what role, when).

  No retention policy needed at this scale. Other state changes (tag list edits, reorders, deletes) are not logged — they're recoverable from the queue/channel state.

- [ ] **Defensive Programming.** Validate all external inputs (Telegram updates, e621 API responses, user-pasted URLs). Use Rust's type system: newtypes for all IDs (`TelegramUserId`, `E621PostId`, `ChannelHandle`, etc.), no `String`-typed enums. Every fallible operation returns `Result<T, BotError>`; no `unwrap`/`expect` on production paths. Errors at API/UI boundaries return user-facing messages without leaking internals.

- [ ] **`/getlogs` Command.** Owner-only. Returns recent log files as Telegram document attachments. (Operator is not always at a workstation, so SSH is not always available.) Reads from the unified log directory (see Changes → Logging Sink Unification).

- [ ] **`/setrepostwindow [days]` Command.** Owner-only. Updates the global repost-resistance window (the default applied to all bots; per-bot overrides remain in their TOML configs). Persists to DB-backed runtime config.

- [ ] **FurAffinity Cookie Configuration.** FA still requires cookie auth to fetch posts. Cookies (`cookie_a`, `cookie_b`) are configurable via env vars (`FURAFFINITY_COOKIE_A`, `FURAFFINITY_COOKIE_B`) at startup, and updatable at runtime via owner-only command. The FA client re-logs in on cookie change.

- [ ] **Migration Script.** Standalone binary `bin-migrate/` that reads the original project's SQLite (`config/vault/storage/db.sqlite`) and writes its `post`, `queue`, `tag`, and `admin` rows into the new schema. Idempotent — safe to re-run. Run once at cutover. Maps `admin` rows to Owner's initial mod list, leaves role-binding to operator post-cutover.

---

## Removals

_Features, code paths, or behaviors from the original that will NOT be carried over. Each item: what's being removed, and why._

- [ ] **Old Project Layout.** The original's hard-coded service controllers (`CConfiguration`, `CDatabase`, `CFileSystem`, `CLogger`, `Ce621API`, `CFuraffinity`) and the monolithic `Server.ts`. Replaced by the hexagonal Rust workspace. DDD with data-only aggregates; cross-aggregate logic in domain services; one-aggregate-per-repository; transactions for multi-step changes.

- [ ] **Old UI (Bot Text-and-Buttons).** Removed wholesale. Replaced by the Telegram WebApp UI in Additions. Reason: poor UX, hard to evolve, can't express richer interactions like queue reordering.

- [ ] **`/prob`, `/setMins`, `ProbabilityModel`.** Reason: dead code in the original — `shouldSend` was never consulted by the cron (DESIGN §4.3). Removed rather than completed. Cron publishes deterministically every N minutes, configurable per-bot.

- [ ] **Channel Sourcing from Other Telegram Channels.** Reason: out of scope. Bot accounts can't read message history; userbots are explicitly out of scope (require phone + personal account). If members of other channels want content forwarded, they should message the channel directly.

- [ ] **Encrypted Vault.** Reason: overkill for this deployment model. Secrets live in environment variables. The operator can layer their own secrets management on top (systemd `LoadCredential`, Docker secrets, sops, etc.) without the bot needing to know.

---

## Changes

_Behaviors that exist in the original and will be modified. Each item: original → new behavior, plus reason._

- [ ] **Programming Language and Architecture.** Original: TypeScript, single Node project, ad-hoc layering. New: Rust, Cargo workspace with multiple crates, strict ports-and-adapters separation. The WebApp UI is its own non-cargo project. The UI ↔ backend boundary is itself a port/adapter pair (HTTP API in axum).

- [ ] **UI Surface.** Original: bot text/button interface. New: Telegram WebApp (Svelte). See the Telegram WebApp UI item in Additions.

- [ ] **Configuration Source.** Original: a `config/` directory with per-env TS files plus a `vault/` directory of plaintext files (DESIGN §8). New:
    - **Env vars** for global secrets (DB URL, Redis URL, FA cookies, the operator's "primary" bot token).
    - **Per-bot TOML files** under `config/bots/<name>.toml` for each spawned bot's tags, cadence, channel, repost window, caption template. Each references its token by env-var name.
    - **DB-backed runtime state** for things changed via commands (e.g. `/setrepostwindow`, role assignments).

  The original's runtime "write to vault" path (`/setcookiefa`) is replaced by env-var rotation + a runtime command that updates in-memory state.

- [ ] **Logging Sink Unification.** Original had two log directories — logger wrote to `./logs/` while `/getlogs` read from `config.logs.dir` (DESIGN §14.3). New: one configurable directory used for both. Default `./logs/`. Logging via the `tracing` crate ecosystem with structured spans and per-update correlation IDs.

- [ ] **Schema and Storage.** Original: SQLite via Sequelize ORM, no migrations, generated models. New: SQLite (single-instance for now; SQL written so a Postgres swap is mechanical — no SQLite-isms in repository code). Hand-written schema with explicit migrations via `sqlx migrate`. Constraints fixed (uniqueness on `tag(name, type)`, `admin(username)`, `post.external_id`; `last_updated` updated on every publish — DESIGN §20 bugs 2, 3, 8).

- [ ] **Media Routing.** Original: `webm`/`mp4` go through `sendPhoto` (broken). New: `gif` → `sendAnimation`, `mp4` → `sendAnimation`, `webm` → `sendVideo`, photos → `sendPhoto`. (DESIGN §4.3, §20 bug 1.)

- [ ] **Cron Resilience.** Original: unbounded retry loop on failure, no reentrancy guard (DESIGN §20 bugs 4, 5). New: bounded retries (3 attempts with exponential backoff) per tick, then give up until next tick. Single-flight execution per channel via Redis lock.

- [ ] **Callback Query Acknowledgement.** Original: never calls `answerCallbackQuery` (DESIGN §20 bug 6). New: always called for whatever residual bot interactions remain after the WebApp migration.

- [ ] **Admin Auth Failure Mode.** Original: admin checkpoint silently drops unauthorised users with no reply (DESIGN §13). New: explicit `Forbidden` response *and* a logged audit entry. User identity is now Telegram numeric ID, not username — survives username changes.

- [ ] **Re-Circulation Tracking.** Original: never updates `last_updated` after re-publishing a recirculated post (DESIGN §20 bug 3). New: `last_published_at` updated on every successful publish, regardless of whether it came from queue or recirculation.

---

## Notes

_Cross-cutting decisions, constraints, conventions, and reminders that apply across multiple items above._

- **On Forbidden Content.** Discovering forbidden content at any stage (curation, pre-publish recheck, retroactive tag-list update) hard-deletes ALL stored data for that post — DB rows, perceptual hash entry, queue references, blame references. No soft-delete or archive.

- **On Reposts.** Two layers: perceptual hash check (Hamming-distance threshold) and time-window check. Only e621 sources are eligible for re-circulation. Default window 30 days, configurable via `/setrepostwindow`. Non-e621 sources are hashed at publish so the same image arriving later from a different source is still rejected.

- **On Infrastructure (Single Instance).** Designed for single-instance deployment. SQLite is the chosen DB engine; the SQL layer is written so a Postgres swap is mechanical (no SQLite-only features in repository code; `DATABASE_URL` is the only switch). State must NOT live in process memory if it must survive a restart — push to SQLite or Redis. In-memory state is for transient request-scoped data only.

- **On Testing.** Unit tests for domain layer (no fakes needed — pure code). Use case tests with in-memory port fakes. Adapter integration tests against real test instances (test SQLite, mock HTTP server, dockerised Redis). CI must run all three tiers. Coverage target: 80% lines overall, 100% on domain services.

- **On Logging.** Use the `tracing` ecosystem (structured, span-based). Per-update correlation IDs (UUID v4) attached to spans. Log sinks: stdout (JSON in prod, pretty in dev) plus a rotating file in the unified log directory. Never log secrets — use newtypes that opt into `Debug` only with redaction (`#[derive(Debug)]` is forbidden on token/cookie types).

- **On Secrets.** Environment variables only. Conventions:
    - `TELEGRAM_BOT_TOKEN_<NAME>` — token for bot `<NAME>` (referenced from its TOML config).
    - `DATABASE_URL` — SQLite connection string.
    - `REDIS_URL` — Redis connection string.
    - `FURAFFINITY_COOKIE_A`, `FURAFFINITY_COOKIE_B` — FA session cookies.

  No encrypted vault; rely on the deployment environment for secret management.

- **On Reverse Proxy.** **Caddy** sits in front of the Rust binary. Single domain (e.g. `bot.example.com`). Routes:
    - `/api/*` → Rust HTTP server (WebApp backend).
    - `/webhook/telegram/*` → Rust (only if/when we move from long-polling to webhook mode).
    - `/*` → static Svelte build directory.

  Caddy handles TLS via Let's Encrypt automatically, gzip/brotli compression, static asset caching, and connection buffering. Rust binary listens on `127.0.0.1:<port>` only — never directly exposed.

- **On Tooling/Language Choices.**
    - Language: **Rust** (stable channel).
    - Async runtime: **`tokio`**.
    - Telegram lib: **`teloxide`**.
    - HTTP server: **`axum`** (for the WebApp API).
    - HTTP client: **`reqwest`**.
    - DB driver: **`sqlx`** with compile-time-checked queries against SQLite. **No ORM.**
    - Migrations: **`sqlx migrate`**.
    - Config loading: **`figment`** (env + TOML layered).
    - Logging: **`tracing`** + **`tracing-subscriber`**.
    - Image hashing: **`image-hasher`** (no ML).
    - Errors: **`thiserror`** for library/crate errors, **`anyhow`** only at the binary boundary.

- **On Module Convention.** **No `mod.rs` files.** Use the `<name>.rs` + `<name>/` sibling layout. Each crate has a `lib.rs` (or `main.rs` for binaries). Example: `domain/src/post.rs` for the module file, `domain/src/post/` for its submodules.

- **On Workspace Layout.**
  ```
  crates/
    domain/              # entities, value objects, domain services (pure)
    application/         # use cases, port traits
    infra-telegram/      # teloxide adapter
    infra-persistence/   # sqlx repositories + migrations
    infra-imageboard/    # e621, FurAffinity, fxtwitter, Bluesky clients
    infra-config/        # env-var + TOML loader (figment)
    infra-redis/         # redis adapter (caching, rate-limit, locks)
    infra-blame/         # audit log adapter
    infra-http/          # axum server + WebApp API handlers
    bin-bot/             # main binary: composition root
    bin-migrate/         # one-shot migrator from old SQLite to new schema
  webapp/                # Svelte project (separate, not in cargo workspace)
  config/
    bots/                # per-bot TOML files (one per spawned bot)
  ```

- **On Single-Process Model.** One Rust binary hosts:
    - One **`axum` HTTP server** on a single port (serves the WebApp API; reverse-proxied by Caddy).
    - **N `teloxide` bot dispatchers**, one per spawned bot, each on its own `tokio` task.
    - **One scheduler task per bot** for the publish cron.

  All share the same domain/application crates. Composition root in `bin-bot/src/main.rs`.

---

## Next Steps

- [ ] Scaffold the Cargo workspace per the layout above.
- [ ] Create the **domain** crate (entities, value objects, domain services — no I/O).
- [ ] Create the **application** crate (port traits and use cases; in-memory fakes for tests).
- [ ] Create infra adapters (`infra-persistence`, `infra-telegram`, `infra-imageboard`, `infra-redis`, `infra-blame`, `infra-http`, `infra-config`).
- [ ] Create the migration binary (`bin-migrate`: old SQLite → new schema).
- [ ] Create the Svelte WebApp project; wire `initData` HMAC verification.
- [ ] Wire `bin-bot` composition root: load env + TOML configs, spawn N bot tasks, start axum.
- [ ] Configure Caddy reverse proxy for prod.
- [ ] Write tests at all three tiers; integrate with CI.
- [ ] Cutover: run `bin-migrate`, switch DNS / Telegram bot tokens to the new binary.

---

## Your Questions

[All resolved — answers folded into the sections above. Kept here as an audit trail.]

### Formatting / housekeeping
1. **Bullet style.** Items currently use `--- [ ]` (three dashes). Markdown renders this as a horizontal rule plus a stray `[ ]`. Standard would be `- [ ]`. Want me to convert all of them?

   > Yes

2. **Status legend usage.** The legend lists `[~]` in progress and `[-]` dropped, but no items use either yet. Are we going to track progress in this file as work happens, or in a separate tracker?

   > I am fine with you choosing this one. Whichever is easier for LLMs.

### Architecture & language (blocks any code work)
3. **Telegram bot library in Rust.** `teloxide` is the de-facto choice; `frankenstein` and `grammers` exist as alternatives. Any preference, or pick `teloxide`?

   > I like Teloxide best.

4. **Async runtime.** `tokio` (assumed) or `async-std`?

   > Tokio for the win.

5. **Database driver.** `sqlx` (compile-time-checked SQL, async, supports SQLite + Postgres), `sea-orm` (active record), or `diesel` (sync, mature)? My default for the layout you describe would be `sqlx`.

   > Yup. SQLX. Do NOT want ORMs.

6. **Database engine.** You mentioned "SQL databases" plural and Redis. Sticking with SQLite (current), or moving to Postgres for multi-instance support? Multi-instance with SQLite is unsafe (DESIGN §18); the "Multi-Channel" + "shared state across instances" goals effectively require Postgres or MySQL.

   > We want to use PosgreSQL. This will only be one instance, so maybe we can go with SQLite.

7. **Workspace layout.** Rough sketch I'd propose, want to confirm before scaffolding:
   ```
   crates/
     domain/              # entities, value objects, domain services (pure)
     application/         # use cases, port traits
     infra-telegram/      # teloxide adapter
     infra-persistence/   # sqlx repositories
     infra-imageboard/    # e621 + furaffinity + twitter (fxtwitter) + bsky clients
     infra-vault/         # encrypted secret store
     infra-redis/         # queue adapter
     infra-blame/         # audit log adapter
     bin-bot/             # the actual binary (composition root)
     bin-api/             # HTTP API for the WebApp UI (if separate process)
   webapp/                # Svelte project (separate, not in cargo workspace)
   ```

   > Looks good to me! Please use the `lib.rs` layout and named modules for crates. It should be... For example, inside domain, if we have a dir, it should be... `element.rs` and `element/`, for example. Do not use `mod.rs` files.

8. **Bot ↔ UI process model.** Your "Next Steps" #4 asks whether the Rust bot should be one process exposing an HTTP API consumed by the Svelte WebApp, or two processes. My recommendation: **one process** — the Telegram bot client and the HTTP server live in the same binary, sharing the same domain/application crates. Telegram WebApps are just regular web pages opened from a Telegram button — the user's browser hits your HTTP endpoint directly, with the Telegram-provided `initData` for authentication. Confirm?

   > Confirmed.

### Domain decisions
9. **Vault encryption.** What threat model? "Encrypted at rest" can mean (a) AES-GCM with a key from an env var, (b) age/sops-style with a recipient key, (c) integration with a real KMS (HashiCorp Vault, AWS KMS, etc). The current "vault" is just plaintext files in a directory. What's the practical bar?

   > To be honest, this is overkill. We can just skip this. We are going to be using environment variables for the secrets, so we can just skip this. We can add it later if we want to.

10. **Hashing algorithm for content.** Perceptual hash (pHash/dHash, robust to re-encoding/cropping — needed if you want to catch the same image reposted from a different source) or cryptographic hash (SHA-256 of the file bytes, exact match only)? Perceptual matters more for the multi-source goal because the same image often appears on Twitter and e621 with different file bytes.

    > Perceptual hash is the way to go. We want to be able to catch the same image reposted from a different source, so we need something that is robust to re-encoding/cropping. Do NOT use ML-based image similarity — too much infrastructure for this use case.

11. **Multi-source: how do non-e621 posts enter the queue?** For e621 the operator browses and approves via the UI. For Twitter/Bsky/FA — does the operator paste a URL? Does the bot watch specific accounts? Watching accounts is a much bigger feature than ingesting a URL.

    > We are going to fetch the e621 API for new posts and allow a selection of those posts to be added to the queue. We can make it kind of like a Tinder Smash or Pass (without the animation. Smash or Pass buttons are fine.) For Twitter/Bsky/FA, the operator can paste a URL. The bot will fetch the content, show content confirmation (making sure we are grabbing the correct things) and then queue it if the operator approves.

12. **"Channel sourcing" tag mechanic.** Re-read item: bot reads from *other Telegram channels* and reposts content tagged a certain way. Does the bot need to be a member of those channels? Userbot vs bot account? (Bot accounts cannot read message history of channels they're admin of — they only see new updates from the moment they're added. Userbots can, but require a phone number and personal account.)

    > We are not going to be using Userbots. We can skip this one. Members of channels that want something to be sent to the channel should just message the channel, not the bot. Out of scope.

13. **Roles vs Telegram identity.** Are roles attached to Telegram numeric user IDs, or to a separate account in your system that's *linked* to a Telegram identity? The latter scales better (e.g. role survives a Telegram username change) but is more work.

    > Roles are attached to User IDs.

14. **"Blame" scope.** Every state-changing action, or only role-gated actions (delete, move, publish)? Storing every action is fine but grows fast — do we need retention/rotation?

    > Only role-gated actions. Adding new members as roles is also in this scope. Admins can add mods. Owner can add admins. Mods can't add anybody. We only care to know who added a post or a member.

15. **Repost-resistance window.** N days from the example — what's the actual policy? Per-source (e621 reposts after X days, Twitter never)? Global?

    > Should be configurable through a bot command, such as `/setrepostwindow [days]`. Default should be 30 days. Only e621 can be reposted.

### Migration
16. **Existing data.** The current SQLite has live `post`, `queue`, `tag`, `admin` rows. Do we need a migration script, or is the new system starting empty?

    > We do!

17. **Operator's existing channel.** Same channel handle, same bot token? Or new bot? (Same token is fine — Telegram bots are not bound to an implementation language.)

    > All through env. Channels should now be saved along with their configurations in the database, so we can add new channels without redeploying the bot. We can just point the new bot to the same channel handle and token. We should be able to spawn new bots from this program by passing a token, specifying "default" tags, "forbidden" tags, how often to post, what channel to post to, etc. through bot commands (Such as `/add_bot`). Only the owner should be able to add new bots. If we want, we can make configuration files for each bot, rather than keeping them in the database, so that they are just spawned into a new tokio thread. I prefer this.

### Out-of-scope confirmation
18. **`/prob`, `/setMins`, `shouldSend`.** Should I add these to **Removals** explicitly per the DESIGN.md punch list, or are they implicitly dropped by removing the old UI?

    > Yup.

19. **FurAffinity `Login` scaffolding.** Same — explicit removal or implicitly dropped?

    > We are going to need the login for FurAffinity if we want to fetch the content from there. It should be configurable from cookies, though.

20. **`/getlogs` command.** With proper logging infra (file + maybe a log aggregator), do we still need a Telegram command to retrieve logs? Default would be: drop it, ssh into the host instead.

    > I say we keep it because I am not in front of my computer all the time.
