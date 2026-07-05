# yiffy-corner-bot — Functional Design Specification

This document is a language-agnostic specification of the existing TypeScript implementation, written as a rewrite brief targeting **Hexagonal Architecture** (a.k.a. Ports & Adapters). It captures *what* the system does and *why*, not *how* the current code achieves it. The goal is that someone with this document and no access to the existing source could produce a behaviourally equivalent implementation in any language.

Conventions used below:
- "**Operator**" = the human Telegram user who is the bot's owner.
- "**Admin**" = a Telegram user whose username has been added to the admin allow-list.
- "**Channel**" = the Telegram channel into which posts are published.
- All times are interpreted in offset **UTC−06:00** unless stated otherwise.

---

## 1. Goal & Purpose

A self-hosted, single-tenant Telegram bot that:

1. **Curates** image and animation posts from the e621 image board, applying a configurable allow-list ("default tags") and block-list ("forbidden tags").
2. Lets the operator **review** candidate posts in a private chat via inline keyboards and explicitly approve them into a publishing queue.
3. **Publishes** posts to a target Telegram channel on a fixed cadence, drawing first from the operator-curated queue and falling back to a re-circulation of previously published posts after a cool-down.
4. Provides **administrative** commands for managing tags, admins, cookies, logs, and queue state.

The system is operated by exactly one human (the *owner*); a small number of additional users may be granted *admin* status to use a subset of commands. There is no multi-tenant or self-service concept.

---

## 2. High-Level System Overview

The bot has **three concurrent input channels**:

| Channel | Trigger | Effect |
|---|---|---|
| Telegram **commands** (private chat) | A user sends a message starting with `/` | Looked up against the command catalog (§10), gated by middleware (§13), executes a use case |
| Telegram **callback queries** | A user taps an inline-keyboard button under a previously sent message | Looked up against the callback catalog (§11), executes a curation action |
| **Scheduler** | A cron tick every 5 minutes | Attempts to publish the next post to the channel (§12) |

There is **one output channel**: the Telegram Bot API, used both for replies in private chat and for posting to the channel.

There are **three external dependencies** beyond Telegram:
- **e621.net** HTTP API — read-only, used to search for posts and to fetch a single post's full metadata.
- **furaffinity-api** session login — currently scaffolded but not actually exercised in any user-facing flow; cookies are persisted and a login is attempted on startup and on `/setcookiefa`.
- **Local filesystem** — for the SQLite database, the secrets vault, and log files.

---

## 3. Hexagonal Architecture Overview

The rewrite should organise code into the following concentric rings. **Nothing in an inner ring may import from an outer ring.**

```
                    ┌─────────────────────────────────────────────┐
                    │  Driving adapters (inbound)                 │
                    │  - Telegram command router                  │
                    │  - Telegram callback router                 │
                    │  - Cron scheduler                           │
                    │  - Process lifecycle (signals, startup)     │
                    └──────────────────┬──────────────────────────┘
                                       │ calls driving ports
                                       ▼
                    ┌─────────────────────────────────────────────┐
                    │  Application layer (use cases)              │
                    │  Orchestrates the domain and driven ports.  │
                    │  No I/O of its own; no framework code.      │
                    └──────────────────┬──────────────────────────┘
                                       │ uses domain + driven ports
                                       ▼
                    ┌─────────────────────────────────────────────┐
                    │  Domain (pure)                              │
                    │  Entities, value objects, domain services.  │
                    │  No I/O. Deterministic given inputs.        │
                    └─────────────────────────────────────────────┘
                                       ▲
                                       │ implements driven ports
                    ┌──────────────────┴──────────────────────────┐
                    │  Driven adapters (outbound)                 │
                    │  - SQLite repositories                      │
                    │  - e621 HTTP client                         │
                    │  - Telegram Bot API client                  │
                    │  - Vault (filesystem secrets)               │
                    │  - Config loader                            │
                    │  - Logger sinks                             │
                    │  - Clock, RNG, UUID, FurAffinity login      │
                    └─────────────────────────────────────────────┘
```

The current TypeScript code does **not** follow hexagonal architecture: `Server.ts` mixes Telegram framework code, business logic, persistence calls, HTTP requests, formatting, and scheduling in one ~700-line file. The rewrite is an opportunity to separate these concerns cleanly.

---

## 4. Domain Model

### 4.1 Entities

#### `Post`
Represents a post the operator has approved at some point in the past. Persistent.

| Field | Type | Notes |
|---|---|---|
| `id` | integer, surrogate PK | Internal database identity |
| `externalId` | string | The e621 post ID (stored as text in current schema, used as numeric in the e621 API) |
| `lastUpdated` | date (YYYY-MM-DD) or null | The day this post was last published to the channel; used for re-circulation cool-down |

A `Post` is created when the operator taps **Send** on a candidate (a `send` callback). It is destroyed when (a) the operator taps **Delete** via `/removeFromSaved`, or (b) it is found to contain forbidden tags at publish time.

#### `QueueEntry`
A pointer to a `Post` that is awaiting publication. Persistent. FIFO-ish (current code calls `findOne` without an explicit `ORDER BY`, so order is implementation-defined; the rewrite should explicitly order by insertion time).

| Field | Type | Notes |
|---|---|---|
| `id` | integer, surrogate PK | |
| `postId` | integer FK → `Post.id` | |

A `QueueEntry` is created at the same time as its `Post` (on `send` callback). It is destroyed (a) when its post is published, (b) when the operator deletes the post via `/removeFromSaved`, or (c) when the post is rejected for forbidden tags at publish time.

#### `Tag`
A tag in either the default (allow) list or the forbidden (block) list. Persistent.

| Field | Type | Notes |
|---|---|---|
| `id` | integer, surrogate PK | |
| `name` | string | Raw e621 tag name (no `#`, not URL-encoded at storage time) |
| `type` | enum: `'D'` (default) \| `'F'` (forbidden) | Schema default is `'F'` |

#### `Admin`
A Telegram **username** (not numeric ID) that is permitted to use admin-gated commands.

| Field | Type | Notes |
|---|---|---|
| `id` | integer, surrogate PK | |
| `username` | string | Case-sensitive in current code — see §13 quirks |

### 4.2 Value Objects

These are not persisted but are central to the domain logic and should be modeled as immutable types:

- **`TagSet`** — the pair `(default: Set<string>, forbidden: Set<string>)` used as input to e621 searches. Knows how to render itself into the e621 query string format (§7.1).
- **`E621Post`** — the de-serialised representation of a post returned by e621 (id, file ext + url, score, sources[], tags grouped by category: artist, character, copyright, general, meta, species, lore, invalid). The domain treats this as a frozen snapshot.
- **`PostCaption`** — the rendered MarkdownV2 caption sent with the media to the channel (§15).
- **`CurationKeyboard`** — the inline keyboard offered to the operator under a candidate post (Send / Erase / Check src), and the confirmation keyboard for `/removeFromSaved` (Delete / View / Cancel).
- **`CallbackPayload`** — `{ id: <e621 post id>, type: 'send' | 'erase' | 'destroy' }`, serialised as JSON in the `callback_data` field (Telegram limits this to 64 bytes — current code is well under).

### 4.3 Domain Services (pure functions)

These belong in the domain ring and have no I/O dependencies. They are the heart of the rewrite — every one of them should have unit tests.

#### `TagQuery.build(default, forbidden) → string`
Builds the e621 `tags=` query string fragment.
- If both lists are empty, return empty string.
- **Cap:** while `default.length + forbidden.length > 40`, pop from the end of `forbidden`. (The default list is never truncated; in practice the operator controls the count via the database and command-line overrides.) The number 40 is e621's per-search tag limit for unauthenticated requests.
- Render `default` as items joined by `+`.
- Render `forbidden` as items joined by `+-`, i.e. each forbidden term gets its own `-` prefix once the strings are joined.
- If both are non-empty, concatenate `<default-rendered>+-<forbidden-rendered>`. If only one is non-empty, use that one.
- Return `?tags=<rendered>`.

Example: default `["score:>50", "rating:e"]`, forbidden `["young", "loli"]` → `?tags=score:>50+rating:e+-young+-loli`.

#### `PostFilter.dropPostsWithForbiddenTags(posts, forbidden) → posts`
Returns posts whose union of all per-category tag values does not intersect `forbidden`. Comparison is **case-insensitive** (current code uppercases both sides).

#### `PostFilter.containsForbiddenTags(post, forbidden) → bool`
Same as above but for a single post. Used at publish time to defensively re-check (because the tag set on e621 may have been edited between approval and publication).

#### `PostFilter.dedupe(existing, incoming) → posts`
Returns the union of two post lists deduplicated by `post.id` (e621 id, **not** internal id), preserving order and existing entries.

#### `SourcePicker.preferred(sources) → string | null`
Given the array of post sources, return the first one whose URL matches one of these regexes, in priority order:

1. `https://www.twitter.com`
2. `https://www.furaffinity.com`
3. `https://www.tumblr.com`
4. `https://www.deviantart.com`
5. `https://www.pixiv.net`

Falls back to `sources[0]` if none match. Returns `null` only if `sources` is empty.

> **Note:** the regexes anchor on `https://www.<domain>` and so will miss naked-domain or `https://<domain>` variants common in e621's source field (e.g. `https://twitter.com/...` won't match). The rewrite should consider broadening these to `https?://(www\.)?<domain>` while preserving priority ordering. Document this as a deliberate change.

#### `MarkdownV2.escape(text) → string`
Escape every occurrence of these characters with a leading backslash:

```
_  *  [  ]  (  )  ~  `  >  #  +  -  =  |  {  }  .  !
```

Empty input returns empty string. Idempotency is **not** preserved — re-escaping doubles the backslashes; callers must escape exactly once.

#### `Caption.build(post, channelHandle) → MarkdownV2-string`
Construct the caption that is sent with the media to the channel:

```
\[<post_id>\]

Artists: #artist1 #artist2 ...
Meta: #meta1 #meta2 1920x1080 16:9 ...

[e621 Source](https://e621.net/post/show/<id>) \| [Source 1](<src1>) \| [Source 2](<src2>) \|

<channel-handle-from-vault>
```

Specifics:
- The `[` `]` around `<post_id>` are *escaped* (`\[`, `\]`) so MarkdownV2 keeps them literal.
- The `Artists:` line is **omitted** if there are no artists after filtering.
- The `Meta:` line is **omitted** if `post.tags.meta` is empty.
- Artist filtering: drop entries equal to any of `sound_warning`, `conditional_dnp`, `unknown_artist`, `anonymous_artist`.
- Artist transformation: remove the characters `( ) . -` from the name, then prefix with `#`. Then run the whole space-joined string through `MarkdownV2.escape`.
- Meta transformation: keep entries that match `^\d+$` (a count) or `^\d+:\d+$` (an aspect ratio) verbatim; otherwise remove `( ) . -` and prefix with `#`. Then escape.
- Sources: always include `[e621 Source](https://e621.net/post/show/<id>)` first, then each `post.sources[i]` as `[Source <i+1>](<url>)`. Joined by ` \| ` (escaped pipe with a trailing space). The last entry has a trailing space but no separator after it.
- Channel handle: read from the vault file pointed to by `channel.at` config (e.g. `@my_channel`). Appended verbatim (already MarkdownV2-safe by virtue of being a username).

#### `MediaKind.fromExtension(ext) → 'photo' | 'animation'`
Telegram requires different API methods depending on media type:
- `'gif'` → `animation`
- everything else (including `'webm'`, `'mp4'`, `'png'`, `'jpg'`, `'jpeg'`) → `photo`

> **Known issue:** the current implementation labels `webm`/`mp4` as "animated" internally but still routes them to `sendPhoto`, which Telegram rejects. The rewrite should send `webm`/`mp4` via `sendVideo` (or `sendAnimation` for `mp4` if silent loops are desired). Decide and document.

#### `RestartGuard.shouldRefuse(now, offset='-06:00') → bool`
True iff `now + 5 minutes` is after end-of-day in offset `-06:00`. Used to block command processing in the last 5 minutes of the local day. The bot itself does **not** restart — this guard relies on an *external* process killing the container at midnight, with `restart: always` in `docker-compose.yml` bringing it back up. The rewrite should preserve this contract or replace it with an explicit shutdown signal.

#### `ProbabilityModel.shouldSend(minutesSinceLastSent, rng) → bool`
- Sample `r` uniformly from `[1, 99]` (inclusive integer).
- Compute `p = 1e-6 * ((minutesSinceLastSent / 60) * 100)^4`.
- Return `p > r`.

> **Note:** in the current code this is exposed via `/prob` and `/setMins` only — the cron does **not** consult it. `minutesSinceLastSent` is never auto-incremented; the operator sets it manually for testing. This is effectively an unfinished feature. The rewrite should either complete it (gate the cron on this and increment a clock-driven counter) or remove it. **Decide before starting work.**

### 4.4 Authorization Rules (domain)

Two pure rules:

- **Owner check:** the message's Telegram-numeric-`from.id` must equal the configured `owner.id`. Failure raises a domain error with `botMessage = "Only the owner can do this!"`.
- **Admin check:** the message's `from.username` (string) must exist in the `Admin` repository. Failure raises a domain error with `botMessage = "Forbidden!"`.

Both are pure functions of an input identity and a lookup result; the lookup itself is a port (§6.2).

---

## 5. Application Layer (Use Cases)

Each use case is a single function or class with one public entry-point. Inputs and outputs are domain values; side effects go through driven ports.

### 5.1 Curation use cases

#### `FetchCandidatePosts(overrides?: { default?, forbidden? }) → E621Post[]`
The core retrieval routine. Returns up to 75 posts that satisfy the (default, forbidden) filter.

Algorithm:
1. Resolve the effective `default` and `forbidden` tag lists:
   - If `overrides.default` is provided, use it. Otherwise, load all `Tag`s of type `D` from the repo and **URL-encode each name** before use.
   - If `overrides.forbidden` is provided, *append* it to the DB-loaded `F` list (the override is additive). Otherwise, just use the DB list. Forbidden DB names are also URL-encoded.
2. Initialize `found = []`, `tries = 3`, `previousLength = -1`.
3. Loop:
   - Call e621 with the current tag set.
   - If the response is not OK or has no `posts` field: decrement `tries`; if `tries <= 0`, raise `{ botMessage: "There seems to be an error with e621!" }`; otherwise continue.
   - Filter the returned posts against `forbidden` (full-tag, case-insensitive).
   - Merge into `found` with deduplication.
   - If the *raw* response had fewer than 75 posts, break out of the loop ("not a popular tag").
   - If `found.length === previousLength`, break (no progress).
   - If `found.length >= 75`, break.
   - Otherwise sleep 500 ms, set `previousLength = found.length`, repeat.
4. Trim `found` to length 75 (drop from the **front**, keeping the most recent additions).
5. Return.

The 75 number is hard-coded and corresponds to e621's default page size. The 500 ms sleep is to be polite to the API.

#### `PreviewPostsToOperator(chatId, posts)`
For each post, send it to the operator's private chat with the **curation keyboard**:

- Row 1: `Send` (callback `{id, type:"send"}`)
- Row 2: `Check e621 Src` (URL button → `https://e621.net/post/show/<id>`), `Check src` (URL button → `SourcePicker.preferred(post.sources) ?? <e621 url>`)
- Row 3: `Erase` (callback `{id, type:"erase"}`)

Use `sendAnimation` for `gif` media; `sendPhoto` otherwise (see `MediaKind` quirk above).

Errors during send are caught and logged but do not abort the loop.

#### `ApprovePost(externalId)` — `send` callback
1. Delete the message that hosted the keyboard (best-effort; ignore failures).
2. If a `Post` with this `externalId` already exists, log and return without error (idempotent).
3. Fetch full post metadata from e621 (`/post/show/<id>.json`). On non-OK, raise `{ botMessage: "There seems to be an error with e621!" }`.
4. Insert `Post(externalId, lastUpdated = today in UTC−06:00)` and `QueueEntry(postId)` in that order. If either insert fails, raise an appropriate error.

#### `DismissCandidate(externalId)` — `erase` callback
Just delete the message. No DB writes. (Type is checked but no further work is done.)

#### `DestroyPost(externalId)` — `destroy` callback
1. Delete the host message (best-effort).
2. Find the `Post` by `externalId`. If absent, reply `"No post found!"` and return.
3. Find the corresponding `QueueEntry` (if any) and destroy it.
4. Destroy the `Post`.

### 5.2 Publication use case

#### `SendNextPost({ force?: bool })`
The core publication routine, called every 5 minutes by cron and also by `/sendnext`.

> **Important quirk:** the outer `sendNext` retries indefinitely on failure (an `await runSendasync` inside `while(true) try/catch`). On a persistent failure (e.g. e621 outage) this currently spins forever. The rewrite should bound retries (e.g. 3 attempts with backoff, then give up until the next tick).

Algorithm:
1. **Pick a post:**
   - Try to find one `QueueEntry` joined with its `Post`. If found, this is `toBeSent` and `isPartOfQueue = true`.
   - Otherwise, find a random `Post` whose `lastUpdated <= today − 20 days`. If found, `toBeSent = { post: <that post> }` and `isPartOfQueue = false`.
   - If neither exists, return silently (nothing to publish today).
2. **Fetch metadata:** GET `https://e621.net/post/show/<post.externalId>.json`. On non-OK, return.
3. **Forbidden-tag re-check:** load all `'F'` tags from the repo. If the e621 metadata contains any forbidden tag:
   - If part of queue, destroy the queue entry.
   - Destroy the `Post`.
   - Raise `{ botMessage: "Post contains forbidden tags!" }` (which `sendNext` catches and retries — but with the post now destroyed, the retry will pick a different post).
4. **Recent-publish guard:** maintain an **in-process** ring buffer `latestPosts` of the last 100 published external IDs. If the candidate is in this buffer, raise `{ botMessage: "This post has been sent recently!" }`. Otherwise, append the id (evicting the oldest when over 100). This protects against the random-fallback path picking the same post twice in quick succession.
5. **Build caption** (§4.3 `Caption.build`).
6. **Read channel handle** from vault file at `channel.at`.
7. **Publish:**
   - If extension is `gif`, call `sendAnimation(channel, fileUrl, { caption, parse_mode: MarkdownV2 })`.
   - Otherwise call `sendPhoto(...)`.
   - (See media-kind quirk for `webm`/`mp4`.)
8. **Cleanup:** if `isPartOfQueue`, destroy the `QueueEntry`. The `Post` itself is **kept** so the random-fallback path can re-circulate it after 20 days. If not part of queue, the `Post.lastUpdated` is **not** updated in the current code — meaning a re-circulated post stays eligible for re-circulation forever once its 20-day clock expires. **The rewrite should update `lastUpdated` to today after every publish.**

### 5.3 Administration use cases

#### `AddEntries(type, args)` — `/add <type> <args...>`
- `type` ∈ `{ admin, defaultTag, forbiddenTag }`. Reject otherwise with the message
  `"Invalid types!\nAllowed types: admin, defaultTag, forbiddenTag!"`.
- For each `arg`:
  - If `type == admin`, insert `Admin(username = arg)`.
  - If `type == defaultTag`, insert `Tag(name = arg, type = 'D')`.
  - If `type == forbiddenTag`, insert `Tag(name = arg, type = 'F')`.
- On success reply `"Added!"`; on any error reply `"There seems to be an error with the database!"`.
- **No duplicate prevention** in current code — repeated `/add` of the same value creates duplicate rows. The rewrite should make these uniqueness-constrained.

#### `RemoveEntries(type, args)` — `/remove <type> <args...>`
Symmetric to `Add`: deletes rows matching `username = arg` (admin) or `name = arg` (tag). Reply `"Removed!"`.

> **Bug:** for tags, this deletes *all* rows with the given `name` regardless of type. So `/remove defaultTag rating:e` would also delete a forbidden tag named `rating:e`. The rewrite should also filter by `type`.

#### `ListEntries(type)` — `/list <type>`
- Validate `type` as above.
- Query and return rows of that type, ordered alphabetically.
- Format reply as `<type>:\n\n>entry1\n>entry2\n...` (the `>` is part of MarkdownV2 quote syntax but is sent as plain text — no `parse_mode` is set on this reply, so it renders literally).
- If empty, reply `"No values!"`.

#### `CountQueue()` — `/count`
Reply `"There are <n> posts in queue."` where `n` is the row count of `QueueEntry`.

#### `RemoveFromSaved(externalId)` — `/removeFromSaved <id>` (owner)
1. Find the `Post` by `externalId`. If not found, reply `"No post found!"` and stop.
2. Fetch the post metadata from e621.
3. Send it to the operator with a **delete-confirmation keyboard**:
   - Row 1: `Delete` (callback `{id, type:"destroy"}`)
   - Row 2: `Check e621 Src` (URL button)
   - Row 3: `Cancel` (callback `{id, type:"erase"}`)

The actual deletion happens in the `destroy` callback handler (§5.1).

#### `SetFurAffinityCookies(a, b)` — `/setcookiefa <a> <b>` (owner)
1. Validate two non-empty arguments.
2. Resolve current `NODE_ENV` (e.g. `production`).
3. Write `a` to `<vault>/<env>/cookie_a.txt` and `b` to `<vault>/<env>/cookie_b.txt`.
4. If both writes succeeded, call the FurAffinity `Login(a, b)` adapter and reply `"Cookies set!"`.
5. Otherwise reply `"Could not set cookies!"`.

> **Note:** the rewrite should validate that `NODE_ENV` (or its replacement) corresponds to an existing vault subdirectory; the current code happily creates files in arbitrary names.

#### `ForceSendNext()` — `/sendnext` (owner)
Calls `SendNextPost({ force: true })` then replies `"Sent!"`. The `force` flag is currently **unused** by the implementation — it's plumbed through but never read. Decide whether to honour it (e.g. bypass the recent-publish guard) or remove it.

### 5.4 Operational / introspection use cases

| Use case | Trigger | Behavior |
|---|---|---|
| `Ping` | `/ping` | Reply `"Pong!"` (no auth) |
| `ShowTime` | `/time` | Reply `"Bot time is YYYY-MM-DD HH:mm:ss"` (UTC−06:00) (no auth) |
| `ShowVersion` | `/version` | Reply `"Version: v<botVersion>"` |
| `ShowChangelog` | `/changelog` | Reply with a multi-version changelog rendered from in-code data (§16) |
| `ShowInstance` | `/instance` (owner) | Reply `"Instance: <bot-start-time UTC−06:00>"` |
| `GetLogs` | `/getlogs` (owner) | Read `logs.dir`; for each file: if size 0 reply `"<filename> is empty!"`, else upload as a Telegram document with filename `<original>.txt` (the `.txt` is appended verbatim — so `info.log` becomes `info.log.txt`). End with `"Done"`. |
| `Probability` | `/prob` (owner) | Reply `"<minutesSinceLastSent>: <shouldSend()>"` |
| `SetMinutes` | `/setMins <n>` (owner) | Set in-process `minutesSinceLastSent`, reply `"Set to <n>!"` |

---

## 6. Ports

Ports are the boundary interfaces. Adapters implement them (driven) or invoke them (driving).

### 6.1 Driving ports (inbound)

- **`CommandHandler`** — receives a parsed Telegram command (`name`, `args[]`, sender info, chat info) and returns a response or side effect.
- **`CallbackHandler`** — receives a parsed callback query (`payload`, sender, host-message reference).
- **`SchedulerTick`** — fires every 5 minutes; invokes `SendNextPost`.
- **`Lifecycle`** — `start()`, `stop()` for graceful boot/shutdown. Current code has no `stop`; the rewrite should add one.

### 6.2 Driven ports (outbound)

- **`AdminRepository`** — `findByUsername(username)`, `add(username)`, `remove(username)`, `list()`.
- **`PostRepository`** — `findByExternalId(externalId)`, `add(post)`, `removeByExternalId(externalId)`, `findRecirculatable(olderThan: Date, limit=1)` (random ORDER BY for the fallback case), `markPublished(externalId, today)` *(new in rewrite — see §5.2 cleanup)*.
- **`QueueRepository`** — `count()`, `dequeueOldest()` (returns the entry plus its post), `enqueue(postId)`, `removeByPostId(postId)`.
- **`TagRepository`** — `listByType(type)`, `add(name, type)`, `removeByNameAndType(name, type)`.
- **`ImageBoardClient`** (e621) — `searchPosts(tagQuery)`, `getPost(externalId)`. Returns domain `E621Post` values, not raw HTTP responses.
- **`MessagingClient`** (Telegram) — `sendText(chat, text, options?)`, `sendPhoto(chat, url, options?)`, `sendAnimation(chat, url, options?)`, `sendVideo(chat, url, options?)` (new), `sendDocument(chat, source, options?)`, `deleteMessage(chat, messageId)`, `answerCallbackQuery(id)`. Options carry caption, parse mode, inline keyboard.
- **`FurAffinityAuth`** — `login(cookieA, cookieB)`. The current implementation only logs in; nothing else uses the resulting session. Keep the port but consider whether the rewrite needs it at all.
- **`Vault`** — `read(relativePath)`, `write(relativePath, content)`. Backed by per-environment subdirectories of `config/vault/<env>/`. **Never** logs file *contents* (secrets).
- **`ConfigSource`** — `get<T>(key)`, `getOrThrow<T>(key)`. Backed by static per-environment config files (§8).
- **`Logger`** — structured logger with `info|warn|error|debug`, fluent `subCaller(name)` builder, per-request `caller` and correlation `id` tags (§14).
- **`LogStore`** — `listFiles()`, `read(filename)`, `sizeOf(filename)`. Used only by `GetLogs`. Distinct from the logger's own sinks.
- **`Clock`** — `now()`, `nowInOffset(offset)`, `today(offset)`. Used by every place currently calling `moment().utcOffset(...)`.
- **`Random`** — `intInclusive(low, high)`. Used by `ProbabilityModel`.
- **`IdGenerator`** — `newCorrelationId()` (UUID v4 in current code). Used to tag a log scope per Telegram update.

---

## 7. External Integrations (Adapter Contracts)

### 7.1 e621 image board

**Base URL:** `https://e621.net`.

**Required headers on every request:**
- `Cookie: gw=seen` (suppresses the "guest warning" interstitial; not authentication)
- `User-Agent: PostSelector-ZielAnima/v0.3` (or rename per e621 ToS — they require a descriptive UA; pick one that names the bot and the operator)

**Endpoints used:**

| Operation | Method | Path | Notes |
|---|---|---|---|
| Search | `GET` | `/posts.json?tags=<query>` | Query built by `TagQuery.build` (§4.3). Response shape: `{ posts: [...] }` (or `{ posts: null }` / non-OK). Page size is 75 by default — the use case relies on this. |
| Show | `GET` | `/post/show/<id>.json` | Response shape: `{ post: { id, file: { url, ext, ... }, sources: [...], tags: { artist:[], character:[], copyright:[], general:[], lore:[], meta:[], species:[], invalid:[] }, score: { up, down, total }, ... } }` |

The post object's relevant fields used by the system:
- `id` (number)
- `file.url` (string, CDN URL)
- `file.ext` (string: `'jpg' | 'png' | 'gif' | 'webm' | 'mp4'`)
- `sources` (string[])
- `tags.artist` (string[]), `tags.meta` (string[]), and the union of all categories for forbidden-tag screening.

### 7.2 Telegram Bot API

The MessagingClient adapter must support these primitives (mapping to standard Bot API methods):

- `sendMessage(chat_id, text, parse_mode?, reply_markup?)`
- `sendPhoto(chat_id, photo_url, caption?, parse_mode?, reply_markup?)`
- `sendAnimation(chat_id, animation_url, caption?, parse_mode?, reply_markup?)`
- `sendVideo(chat_id, video_url, caption?, parse_mode?, reply_markup?)` *(new for the webm/mp4 fix)*
- `sendDocument(chat_id, file_source, filename?)`
- `deleteMessage(chat_id, message_id)`
- `answerCallbackQuery(callback_query_id)` *(currently never called — the existing code relies on `deleteMessage` to dismiss the keyboard, which leaves the callback unanswered. The rewrite should answer it explicitly to remove the "loading" spinner on the client.)*

Updates the bot must consume:
- `message` events with `text` starting with `/`
- `callback_query` events
- Channel updates **must be ignored** — current code aborts the middleware chain when `ctx.chat.type === 'channel'`.

Inline keyboards are JSON objects matching the Telegram spec; payload sizes for `callback_data` must stay under 64 bytes (current `JSON.stringify({id, type})` does easily).

### 7.3 FurAffinity

Used only via the `furaffinity-api` library's `Login(cookieA, cookieB)`. Called on startup (if cookies are present) and on `/setcookiefa`. Its session state is held internally by that library; nothing else in the bot reads from it. Treat this as **dormant scaffolding** — the rewrite may either implement it as a no-op port for now or remove it entirely.

---

## 8. Configuration Model

### 8.1 Layered config

Three layers, resolved at startup:

1. **Environment selector** — `NODE_ENV` (or your equivalent). Values seen: `default`, `development`, `production`. The `default` layer is always loaded; the env-specific layer overrides.
2. **Static config files** — keyed by env. Provide *paths* and small scalars, **not secrets**. Keys actually consumed by the code:

| Key | Type | Purpose |
|---|---|---|
| `bot.token` | string (vault path) | Path under `vault/` to the file containing the Telegram bot token |
| `owner.id` | integer | Telegram numeric user ID of the operator |
| `commands` | nested object | Command-name aliases — see §10.1 |
| `name` | string (vault path) | Path to a file containing the bot's display name (used in log tags) |
| `cookies.a` | string (vault path) | Path to FurAffinity cookie A |
| `cookies.b` | string (vault path) | Path to FurAffinity cookie B |
| `db` | string (vault path) | Path under `vault/` to the SQLite file (e.g. `./storage/db.sqlite`) |
| `logs.dir` | absolute string path | Directory the operator's `/getlogs` will read from (e.g. `/home/yagdrassyl/bots/logs`). **Distinct from where the logger writes!** |
| `channel.at` | string (vault path) | Path to the file containing the target channel handle (e.g. `@my_channel`) |

3. **Vault** — per-env directory at `<NODE_CONFIG_DIR>/vault/<env>/` containing flat text files for secrets and per-env values. Plus a shared `<NODE_CONFIG_DIR>/vault/storage/` directory for the SQLite database. Treated as the source of truth for all secret material.

### 8.2 Files in vault

For each env (`default`, `development`, `production`) the system expects:

- `token.txt` — Telegram bot token
- `at.txt` — target channel handle
- `name.txt` — display name (may be empty)
- `cookie_a.txt`, `cookie_b.txt` — FurAffinity cookies (created lazily by `/setcookiefa`)

And shared:
- `storage/db.sqlite` — Sequelize DB

Vault files are read with **synchronous** I/O at startup (the bot fails fast if the token is missing) and may be written at runtime via `/setcookiefa`.

### 8.3 Required-config validation at startup

`GetInitConfiguration` collects: `token`, `ownerId`, `commands`, `name`, `cookies`. If any are missing it logs a warning (and optionally throws if the caller passes `fatal: true`). The current `Server.ts` then explicitly throws `"Missing configuration"` if `token`, `commands`, or `ownerId` is missing — but **not** if `name` or `cookies` is missing, those are tolerated.

The rewrite should make the required-vs-optional split explicit:

| Required (fail-fast on startup) | Optional (warn) |
|---|---|
| `bot.token`, `owner.id`, `commands`, `db`, `channel.at` | `name`, `cookies.a`, `cookies.b`, `logs.dir` |

---

## 9. Persistence Schema

SQLite, no migrations, no timestamps. Tables match the current generated `init-models.ts`. The rewrite should consider migrating to typed migrations and adding constraints noted below.

### `admin`
| Col | Type | Constraints |
|---|---|---|
| `id` | INTEGER | PK, AUTOINCREMENT |
| `username` | TEXT | NOT NULL — *recommend: UNIQUE, COLLATE NOCASE* |

### `post`
| Col | Type | Constraints |
|---|---|---|
| `id` | INTEGER | PK, AUTOINCREMENT |
| `post_id` | TEXT | NOT NULL — *recommend: UNIQUE; consider INTEGER since values are e621 numeric IDs* |
| `last_updated` | TEXT | nullable, default `"current_date"` *(literal string — see quirk below)* |

> **Quirk:** the `last_updated` default in the generated code is the literal string `"current_date"`, not a SQL function call. So a row inserted without an explicit value gets the four-letter word `"current_date"`. The application code always sets `last_updated` explicitly via `moment().format('YYYY-MM-DD')`, so this default never fires in practice. The rewrite should drop the bogus default and use either `DEFAULT CURRENT_DATE` (real SQLite function) or always set it from the application.

### `queue`
| Col | Type | Constraints |
|---|---|---|
| `id` | INTEGER | PK, AUTOINCREMENT |
| `post_id` | INTEGER | NOT NULL, FK → `post.id` *(recommend: ON DELETE CASCADE; current code does the cascade manually and can leave orphans on crash)* |

### `tag`
| Col | Type | Constraints |
|---|---|---|
| `id` | INTEGER | PK, AUTOINCREMENT |
| `name` | TEXT | NOT NULL — *recommend: composite UNIQUE(`name`, `type`)* |
| `type` | TEXT | NOT NULL, default `'F'`. Domain enum: `'D'` (default/allow) \| `'F'` (forbidden) — *recommend: CHECK constraint* |

### Suggested additional indexes for the rewrite

- `post(post_id)` for `findByExternalId`
- `post(last_updated)` for the recirculate query
- `queue(post_id)` for cascade lookups
- `tag(type, name)` for `listByType`
- `admin(username)` for the auth checkpoint

---

## 10. Command Catalog

### 10.1 Command-name aliasing

The config exposes commands as objects of `string[]` aliases produced by `variants(name) = [name, name.toUpperCase(), name.toLowerCase()]`. Telegraf is fed the array and matches any element. The rewrite should preserve case-insensitive command matching by lowercasing the incoming command before lookup, then matching against canonical names.

### 10.2 Command catalog (canonical, ordered as in the source)

For each: name, auth tier, args, behavior, error replies. "Auth tier" is the strictest gate the command must pass (channel guard and restart guard apply to **all** commands).

#### Public (anyone with a username)
- `/ping` → `"Pong!"`.
- `/time` → `"Bot time is YYYY-MM-DD HH:mm:ss"` in UTC−06:00.
- `/changelog` → multi-version changelog (§16).
- `/version` → `"Version: v<botVersion>"`.

#### Admin-gated (after `AdminCheckpoint` middleware)
- `/count` → `"There are <n> posts in queue."` Errors: `"There was an error with the database!"`.
- `/getPosts` → fetch+preview (no overrides).
- `/getByRank` → fetch+preview with default override `["order:rank", "rating:e"]`.
- `/getPromising` → fetch+preview with default override `["score:>50", "rating:e"]`.
- `/getWithTags <args...>` → split args; those starting with `-` go to forbidden-override (after stripping the `-`), the rest go to default-override. If no args: `"You need to specify at least one tag!"`. If no posts: `"No posts found!"`.
- `/list <type>` → see §5.3.

#### Owner-gated (after `OwnerCheckpoint` middleware)
- `/setcookiefa <a> <b>` → see §5.3.
- `/removeFromSaved <id>` → see §5.3.
- `/add <type> <args...>` → see §5.3.
- `/remove <type> <args...>` → see §5.3.
- `/instance` → `"Instance: <startedAt>"`.
- `/sendnext` → forces SendNextPost; reply `"Sent!"`.
- `/getlogs` → see §5.4.
- `/prob` → see §5.4.
- `/setMins <n>` → see §5.4.

> **Layering quirk:** in the current code, `/list` is registered *after* the OwnerCheckpoint middleware in source order, but the middleware runs before each command, so `/list` is effectively **owner-only** — even though the comments and config classify it as "authenticated". The rewrite should route this explicitly. Decide whether `/list` should be admin or owner.

### 10.3 Argument parsing

The current parser is a literal `text.split(' ')`. This means:
- Multi-word arguments (with spaces) are not supported.
- Quoting is not respected.
- Empty arguments (consecutive spaces) become empty strings in the args list.

For `genericDatabaseChangeCommand`, the second token is treated as the `type` and the rest as `args[]`. For `genericCommand`, all post-command tokens become `args[]`.

The rewrite can keep this contract for compatibility, or upgrade to a proper command parser; if upgrading, document the new grammar.

---

## 11. Callback Catalog

Callbacks deliver a JSON payload `{ id: <e621 external id>, type: 'send' | 'erase' | 'destroy' }` via `callback_data`. Behavior:

| `type` | Use case | Side effects |
|---|---|---|
| `send` | `ApprovePost` | Delete host msg; create `Post` + `QueueEntry` (idempotent on existing post) |
| `erase` | `DismissCandidate` | Delete host msg; nothing else |
| `destroy` | `DestroyPost` | Delete host msg; cascade-delete `Post` + any `QueueEntry` |

The current code does **not** call `answerCallbackQuery` — the rewrite should.

---

## 12. Scheduling

- **Cadence:** every 5 minutes (`*/5 * * * *`), the system attempts `SendNextPost`.
- **Time source:** local server time (no offset adjustment for the cron expression itself; the timezone-aware logic is only inside `RestartGuard` and timestamp formatting).
- **Reentrancy:** the current code does not guard against overlapping ticks. If `SendNextPost` takes longer than 5 minutes (e.g. e621 outage causing the unbounded retry loop to spin), a second tick can fire concurrently. The rewrite **must** ensure single-flight execution — e.g. with a mutex, an advisory lock, or a `running` flag.
- **Bounded retries:** as noted in §5.2, the retry loop should be bounded.
- **Daily restart contract:** the bot does not self-restart; an external mechanism (Docker `restart: always` plus an external killer) is assumed to recycle the process daily. The `RestartGuard` middleware refuses commands during the last 5 minutes of the local day to avoid mid-operation kills. The rewrite should either preserve this contract or replace it with explicit graceful shutdown on `SIGTERM`.

---

## 13. Authorization & Middleware Pipeline

Telegram updates (commands and callback queries) flow through this pipeline in order. Any step that doesn't call `next` short-circuits the rest.

1. **Channel guard** — drop everything from chats of type `channel`.
2. **Public unconditional commands** — `/ping`, `/time` reply immediately and short-circuit.
3. **Restart guard** — refuse if within 5 minutes of UTC−06:00 midnight.
4. **Username guard** — reject senders without a Telegram username (reply `"You need to set a username to use this bot!"`). On pass: generate UUID, build per-request logger, attach `logger`, `uuid`, `username` to the request context.
5. **Public conditional commands** — `/changelog`, `/version` (after username guard, so anonymous senders don't trigger them).
6. **Admin checkpoint** — look up `username` in `admin` repo. If not found, *throw silently* — the current code's `try/catch {}` block swallows the error and never calls `next`, so the command is dropped without reply. **The rewrite should either reply `"Forbidden!"` or remain silent — pick one and document it.** Also remove the leftover `console.log("Hello")`.
7. **Admin-gated commands** — `/count`, `/getPosts`, `/getByRank`, `/getPromising`, `/getWithTags`, `/list` (despite its config classification — see §10.2 quirk), and the `callback_query` handler.
8. **Owner checkpoint** — compare numeric `from.id` to `owner.id`; throw `"Only the owner can do this!"` on mismatch (caught by the global error handler and replied to the user).
9. **Owner-gated commands** — everything in §10.2 owner section.

> **Username case sensitivity:** the admin lookup compares `username` strings exactly. Telegram allows mixed-case usernames but treats them case-insensitively at login. The rewrite should normalise to lowercase on both sides at the repository boundary to avoid surprising lockouts.

---

## 14. Logging

### 14.1 Logger contract

A logger has:
- A **caller** label (set at construction, usually the username of the requester or the name of a system component like `Cron`).
- An optional **id** (a UUID v4 correlation ID for one Telegram update).
- A mutable **subCaller** label set via `setSubCaller(name)`, usually identifying the function within the use case.

It produces lines of the form:

```
[<level>] [<YYYY-MM-DD HH:mm:ss.000 UTC−06:00>] [<CALLER> <id?>] [<subCaller?>] <message>
```

with these levels: `info`, `warn`, `error`, `debug`.

### 14.2 Sinks

- **Console** (colourised) — always on.
- **File sinks** under `./logs/` (relative to CWD): `info.log`, `warn.log`, `error.log`, `combined.log`. Each level-specific file gets only entries at or above that level (`info.log` gets info+; `warn.log` gets warn+; etc.; `combined.log` gets everything).

### 14.3 Two log directories!

The logger writes to `./logs/` (relative). The `/getlogs` command reads from `config.logs.dir` (currently `/home/yagdrassyl/bots/logs`, an absolute path on the deploy host). These are **different directories** in the current setup — if the bot is run with CWD = `/home/yagdrassyl/bots`, they happen to coincide; otherwise `/getlogs` returns files unrelated to what the logger is producing.

The rewrite should unify these — one config key, one directory, both read and written through the same path.

### 14.4 What gets logged

- Every middleware step has `setSubCaller` calls (`PostFetchCheckpoint`, `Cron`, `Add fn`, `Remove fn`, `List fn`, `Callback fn`, etc.) for traceability.
- Errors are JSON-stringified before being logged, including in the global `bot.catch` handler.
- The logger logs **synchronous** Sequelize SQL via `logging: options?.logger.info` — every query is written to `info.log`. This is verbose; the rewrite should make SQL logging optional (e.g. only in dev).
- **Never log secrets** — the rewrite should ensure tokens, cookies, and the channel handle are not accidentally logged via `JSON.stringify` of full objects.

---

## 15. MarkdownV2 Caption Construction (Detailed)

Pseudocode (all string concatenation; assume `escape` = `MarkdownV2.escape`):

```
caption = "\\[" + post.id + "\\]"

artistTags = post.tags.artist
  .filter(a => a not in {"sound_warning","conditional_dnp","unknown_artist","anonymous_artist"})
  .map(a => "#" + stripChars(a, "().-"))
  .join(" ")
artistsLine = artistTags.isEmpty() ? "" : "\n\nArtists: " + escape(artistTags)

metaTags = post.tags.meta
  .map(t =>
     if t matches /^\d+$/ or /^\d+:\d+$/: t
     else: "#" + stripChars(t, "().-"))
  .join(" ")
metaLine = post.tags.meta.isEmpty() ? "" : "\nMeta: " + escape(metaTags)

e6url = "https://e621.net/post/show/" + post.id
sources = post.sources.length > 0 ? post.sources : [e6url]
sourcesLine = "[e621 Source](" + e6url + ") \\| "
for (i, src) in enumerate(sources):
    isLast = i == sources.length - 1
    sourcesLine += "[Source " + (i+1) + "](" + src + ")" + (isLast ? "" : " \\|") + " "

caption += artistsLine + metaLine + "\n\n" + sourcesLine + "\n\n" + channelHandle
```

Notes:
- The escape happens **only on the artist/meta tag strings**, not on the URL/link parts of the sources line. URLs in MarkdownV2 link syntax `[label](url)` don't need URL-escaping but the `label` does — the `Source N` label is hard-coded and contains no special chars, so it's fine.
- The double pipe escapes (` \\| `) are intentional — `|` is a MarkdownV2 special character.
- The `channelHandle` is read from disk and trusted; it is appended raw.

---

## 16. Versioning & Changelog

The bot version (currently `"1.2.3"`) and the changelog (a map from version string to `{Added: [], Removed: [], Fixed: []}`) are **inline in source**. The current code's changelog includes a `1.2.4` entry but the version string is still `1.2.3` — drift. The rewrite should:

- Externalise the changelog to a data file (e.g. `CHANGELOG.md` or a YAML file).
- Derive the version from a single source (e.g. `package.json` / `Cargo.toml` / `pyproject.toml` depending on target language).
- Render the `/changelog` reply by parsing that file at startup or per-request.

The changelog rendering format is:

```
1.2.1
==========
+ Added entry 1
+ Added entry 2
~ Fixed entry

1.2.2
==========
~ Bug fixes
```

(`+` for Added, `-` for Removed, `~` for Fixed.) If a version has nothing in any list, render `* No changes listed`.

---

## 17. Error Handling Conventions

### 17.1 Domain errors

The current code throws plain objects of shape `{ botMessage: string }` to indicate a user-facing error. The global Telegraf error handler catches anything thrown inside a command handler, and:

- If the error has `botMessage`, replies with `error.botMessage`.
- Otherwise replies `"There was an error in the bot."` and logs the error.

The rewrite should formalise this with a typed `BotError` class (or sum type) carrying both the user-facing message and any internal context. Internal errors (DB connection lost, e621 unreachable) should never leak stack traces to the user.

### 17.2 Silent middleware failures

The admin checkpoint catches all errors silently. This is a deliberate behaviour (drop unauthorised users without giving them any signal) but is undocumented and easy to mistake for a bug. The rewrite should:
- Decide between silent-drop and explicit `Forbidden!` reply.
- Always log the rejection (with username + correlation ID) for audit purposes.

### 17.3 Cron errors

`runSendasync` errors are logged and the outer loop retries indefinitely. This is wrong (§5.2). Fix: bound retries, log at `error` level on final failure, do not block the next tick.

---

## 18. Concurrency & State

In-process state currently held by `Server.ts` (top-level `let` bindings):
- `latestPosts: string[]` — ring buffer of last 100 published external IDs (§5.2).
- `minutesSinceLastSent: number` — manually set via `/setMins`; consulted only by `/prob`.
- The `Telegraf` bot instance, the config, and the various controllers.

This state is **process-local**. If the bot is run in multiple replicas (the changelog mentions clustering), they each maintain independent `latestPosts` buffers and could publish duplicates. The rewrite should:
- Move the recent-publish guard to a persisted store (e.g. a `published_log` table with `(external_id, sent_at)` and a query for "sent within last X").
- Make `minutesSinceLastSent` either persistent or remove it.

The Sequelize / sqlite backend serialises writes in a single process. A multi-process deployment over a single SQLite file is **not safe** and is not currently supported.

---

## 19. Lifecycle

### 19.1 Startup

In order:
1. Set `NODE_CONFIG_DIR` env var (or rewrite-language equivalent) to the project's `config/` directory.
2. Load configuration.
3. Validate required keys; throw if anything required is missing.
4. Read vault files synchronously: token, name, channel handle path.
5. If FurAffinity cookies are present in vault, call `Login(a, b)`.
6. Open the SQLite database, ensure schema exists.
7. Construct the Telegraf bot, register middlewares and command handlers.
8. Schedule the cron job.
9. Call `bot.launch()` (long polling against Telegram).

Total startup cost is dominated by the first long-poll connection.

### 19.2 Shutdown

The current code has **no shutdown handling**. The rewrite should:
- On `SIGTERM` / `SIGINT`: stop accepting new updates, finish in-flight ones with a deadline (e.g. 30 s), close DB, flush logs, exit 0.
- On `SIGKILL`: nothing to do; we trust restart-on-crash to recover.

---

## 20. Quirks, Bugs, and Decisions for the Rewrite

A consolidated punch list of the issues called out throughout this document. Each one is a deliberate decision the rewrite must make.

### Bugs to fix
1. **`webm` / `mp4` posts go to `sendPhoto`** — they should go to `sendVideo` (or `sendAnimation`). §4.3.
2. **`Remove` for tags ignores `type`** — `/remove defaultTag X` can delete a forbidden tag with the same name. §5.3.
3. **`last_updated` not updated on re-circulated posts** — once a post hits the 20-day threshold, it stays eligible forever. Must update on every publish. §5.2.
4. **Cron retries forever on persistent failure** — bound retries. §5.2, §17.3.
5. **No reentrancy guard on cron** — a slow tick + a second tick + concurrent SQLite = corruption risk. §12.
6. **`callback_query` is not answered** — clients see a "loading" spinner. Call `answerCallbackQuery`. §11.
7. **Two log directories** (logger writes to `./logs`, `/getlogs` reads from `config.logs.dir`). Unify. §14.3.
8. **`post.last_updated` schema default is the literal string `"current_date"`** — drop or fix. §9.
9. **Stray `console.log("Hello")` in admin middleware.** §13.
10. **Bot version string lags the changelog.** §16.

### Behaviours to keep but document
- Username middleware refuses Telegram users without a username. Keep.
- Channel guard drops all channel-type chats. Keep.
- Restart guard refuses commands in the last 5 minutes of UTC−06:00 days. Keep, document the contract.
- Admin checkpoint silently drops unauthorised users. Keep, but log explicitly.
- Recent-publish ring buffer is in-process. Keep for single-process deployments; persist if multi-process.

### Features to either complete or delete
- **`shouldSend` probability gate** — currently dead code. Either wire into the cron with proper accounting of `minutesSinceLastSent` (and persist it), or delete `/prob` and `/setMins`. §4.3.
- **FurAffinity scaffolding** — `CFuraffinity` and the `IImageSite` interface are stubs returning empty strings. Either implement them and wire them through (e.g. for cross-posting) or delete. §7.3.
- **`force` flag on `SendNextPost`** — plumbed but unread. Either use it (e.g. bypass recent-publish guard) or remove.

### Architecture upgrades (the hexagonal payoff)
- Replace `CConfiguration`, `CDatabase`, `CFileSystem`, `CLogger`, `Ce621API`, `CFuraffinity` (the existing "controllers", which are really services bundled with adapters) with cleanly separated **driven ports + adapters**.
- Replace the union of `init.ts`, `auth.ts`, `getters.ts`, `adders.ts`, `posts.ts` and the bulk of `Server.ts` with **use case classes** that depend only on ports.
- Make all framework-specific code (Telegraf, Sequelize, node-cron, winston, apisauce) live in **adapters**; the application layer should import zero framework code.
- Move every pure helper (`prepareMarkdown`, `removePostsWithTags`, `removeDuplicatePosts`, `getPreferredSource`, the URL builder, the caption builder) into **domain services** with full unit-test coverage.

---

## 21. Suggested Module Layout (target language)

A starting point — adjust to your language's idioms. The pattern matters more than the names.

```
src/
  domain/                 # pure, no I/O
    entities/             # Post, QueueEntry, Tag, Admin
    value_objects/        # TagSet, E621Post, PostCaption, CallbackPayload
    services/             # TagQuery, PostFilter, SourcePicker, MarkdownV2,
                          # Caption, MediaKind, ProbabilityModel, RestartGuard
    errors/               # BotError, AuthorizationError, ExternalServiceError

  application/            # use cases, depend on domain + ports
    curation/             # FetchCandidatePosts, PreviewPostsToOperator,
                          # ApprovePost, DismissCandidate, DestroyPost
    publication/          # SendNextPost
    administration/       # AddEntries, RemoveEntries, ListEntries, CountQueue,
                          # RemoveFromSaved, SetFurAffinityCookies
    operations/           # Ping, ShowTime, ShowVersion, ShowChangelog,
                          # ShowInstance, GetLogs, Probability, SetMinutes,
                          # ForceSendNext

  ports/                  # interface declarations only
    driving/              # CommandHandler, CallbackHandler, SchedulerTick, Lifecycle
    driven/               # AdminRepo, PostRepo, QueueRepo, TagRepo,
                          # ImageBoardClient, MessagingClient, FurAffinityAuth,
                          # Vault, ConfigSource, Logger, LogStore,
                          # Clock, Random, IdGenerator

  adapters/
    inbound/
      telegram_command_router/
      telegram_callback_router/
      cron_scheduler/
      lifecycle/
    outbound/
      sqlite/             # SqliteAdminRepo, SqlitePostRepo, ...
      e621/               # E621HttpClient
      telegram/           # TelegramBotApiClient
      furaffinity/        # FurAffinityHttpClient (or NoOp)
      vault_fs/           # FilesystemVault
      config_files/       # FileBasedConfigSource
      logger/             # StructuredLogger + sinks
      log_store_fs/       # FilesystemLogStore
      clock_system/       # SystemClock
      rng_system/         # SystemRandom
      uuid_system/        # SystemUuid

  composition/            # the only place `new` is allowed for adapters
    container.{ext}       # builds the dependency graph
    main.{ext}            # entry point: load config, wire, run
```

Tests live alongside each module:
- **Domain** tests are pure and run without any setup.
- **Application** tests use in-memory fakes for every port. Aim for 90%+ coverage of use cases.
- **Adapter** tests are integration tests against real dependencies (test SQLite file, mock HTTP server for e621, Telegram is hard to test live — record-and-replay or mock the HTTP layer).

---

## 22. Out of Scope (Explicitly)

The following are present in the current code but are **not** required for the rewrite:

- The `furaffinity-api` integration's actual functionality (only `Login` is called, never any subsequent API).
- Multi-process clustering (the changelog mentions it but no actual coordination logic exists).
- The "automatic restarts at 00:00" claim from the changelog (no implementation in code; relies on external process management).
- The `update-models.ts` script — it regenerates Sequelize models from the SQLite schema; not relevant in a non-TypeScript rewrite.
- The webpack bundling step — replace with whatever your target language's build tool is.

---

## 23. Acceptance Checklist

A rewrite is complete when:

- [ ] All commands in §10.2 produce identical (or explicitly-decided-to-differ) replies.
- [ ] All callbacks in §11 produce identical state transitions, plus `answerCallbackQuery`.
- [ ] The cron tick publishes from queue → falls back to recirculate → updates `last_updated` correctly.
- [ ] Forbidden tags are checked at fetch time **and** publish time.
- [ ] All bugs in §20 are either fixed or explicitly documented as intentional.
- [ ] Domain layer has no I/O imports.
- [ ] Application layer has no framework imports.
- [ ] All driven ports have at least one in-memory fake for testing.
- [ ] Startup fails fast with a clear message if any required config is missing.
- [ ] Logs do not contain the bot token, FurAffinity cookies, or the channel handle.
- [ ] `SIGTERM` triggers graceful shutdown within 30 seconds.
