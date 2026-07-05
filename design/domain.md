# What Does This Do?

This program is a curator of art, specifically furry. It is designed to allow for users with varying roles to submit art using a source (never hosted directly by us) and have it approved by a moderator before being posted somewhere else on a per-channel cadence (each Channel — equivalently, each Poster — has its own posting interval). The program has self-regulation features to ensure that the same post is not posted multiple times within a certain time frame, and a system to try and resist duplication of content.

The program has a system to report content to admins, and a system to prevent report abuse. It also has a system to allow users to request content removal, and a system to prevent abuse of that feature as well.

# What Can Be Done?
For the MVP:

- Any user can submit art to the bot for approval using a source URL. Source types are a closed set: Twitter, BlueSky, Telegram, FurAffinity, DeviantArt, e621.
- Users have one of three roles:
  - User:
    - Can submit art through a source to the bot
  - Moderator:
    - Can request changes on a submission: the change list is relayed to the submitter, and the SAME source becomes re-submittable by that submitter (the post returns to the moderation queue with fresh tags). Rejection stays permanent.
    - Everything the User can do
    - Approves or rejects submissions
    - Can check the queue
    - Can delete, add or move things around in the queue
    - Can ban users from submitting art
    - Every moderator has the same set of permissions
  - Owner:
    - Everything that the Moderator can do
    - Can add and remove moderators
    - Can create new Posters, attach them to Channels, and configure their tag subscription
- User Roles: Owner > Moderator > User
- Posts can have more than one source. Sources have exactly one post.

# Entities
- Zuri (Me) -- Owner, forever. <- User Entity, but it felt right to make the distinction over here. 
  - Is the only one able to create Posters and add them to channels
- Scheduler
  - Tells Posters that they need to post
  - Follows the MPSC model. 
  - NOTE: Early iterations show that we can have a central scheduler through DelayQueue rather than asking persistence. Outside domain talk, but good for infrastructure. 
- Posts
  - Have one or more sources
  - Are MEDIA, which means they can have a type, such as png, mp4, or otherwise.
  - Only Posts with an e621 source can be re-posted. 
  - Are described by zero or more tags
  - Non e621 posts have zero tags
  - Has a last-posted date
  - Has a pHash
- Source
  - Belong to a single post
  - Have a derived type based on the URL. The type set is closed: e621, Twitter, BlueSky, Telegram, FurAffinity, DeviantArt. URLs that don't match any known type are rejected.
- Tags
  - Describe e621-sourced posts
  - Can be:
    - Default
      - Always applied with every query to e621
      - Required for QUERIES, not for POSTS. Queries are ways to look up posts in e621.
    - Forbidden
      - A single forbidden tag on an e621 post prohibits it from being posted (a post must own NONE of these to be eligible).
- User
  - Has One and Only One role (Described above)
- Channel
  - Owned by One and Only One User (The Owner of this Channel)
  - Is where a Poster goes to put media in
  - Loaded at cold start (Zuri-configured; not created at runtime by other Users)
  - Topology assumes 1:1 with Posters — one Poster per Channel. Not enforced at runtime.
- Poster
  - Owned by Zuri (for MVP, Zuri is the only User who creates Posters, so the owning User is always Zuri)
  - May request posts
  - Has exactly one Channel
  - Tag subscription is configured by Zuri at creation time

# Amendments (2026-07-04)

- **All six source types are usable for publishing.** e621 and FurAffinity
  resolve media natively (e621 API / FA page fetch with the session cookies);
  Twitter and BlueSky resolve through the FixUp embed family (FixupX API,
  fxbsky); DeviantArt and Telegram publish as fixed-embed links. The tag
  lookup query for media remains e621-only — tags never exist for other
  sources.
- **Submission attribution.** A Post submitted by a plain User is credited on
  publication: the caption carries "Submitted by <display name>". The display
  name is captured from Telegram at contact time and cached on the User, so
  publishing needs no live Telegram lookup. Admin-added pool posts carry no
  attribution.
- **Bans.** Moderators+ can ban a User from submitting (`is_banned` on User;
  strict outranking required). A ban blocks new submissions only.
- **Channel-forward submissions.** Forwarding a public channel's post to the
  bot is a submission (source: `https://t.me/<channel>/<msg>`). The bot never
  re-forwards — it *copies* the content (no "Forwarded from" header) and tags
  the origin at the bottom of the caption as
  `Forwarded from channel: @<channel>`. Moderation DMs show the same copy
  with the same attribution. Private channels (no @username) are rejected.

# Amendment: The Feed Model (2026-07-05)

Supersedes the queue/pool split above. Curation now produces ONE ordered
feed, consumed BSky-style:

- **The feed**: every curated Post (moderator-approved submission or admin
  `/browse` save) is assigned the next monotonic `feed_position`. One pool,
  all curated, all tagged.
- **Consumers**: each Poster stores what it wants to see (tag subscription)
  and its `cursor`. On each fire it snapshots the feed end, scans
  `(cursor, end]` in order, posts the first tag-match, and sets its cursor to
  the match — or to the *pre-scan* snapshot end when nothing matched, so an
  entry appended mid-scan is never skipped. Cursor advances only after a
  successful publish.
- **Consume-once**: at the feed end a consumer stays quiet until new content
  is curated. No recycling, no repost cooldown. Infinite consumers under one
  pool.
- **Tags on everything**: e621 entries get API tags at submission (and are
  still re-validated fresh at consume time — Banned↔Accepted flips live on);
  every other source requires submitter tags, inline or via the bot's
  ask-for-tags dialogue (forwards included). The Banned verdict for non-e621
  entries re-checks curated tags against the *current* global forbidden list.
- The old rules "non-e621 posts have zero tags", "only e621 can be
  re-posted", and "submission queue disjoint from tag pool" are obsolete.

# Open Questions

## 1. User capabilities around Channels/Posters
The Entities section says Channels and Posters are owned by Users, but no role in `What Can Be Done?` mentions them. Should there be a User capability like "Can request a Poster on a Channel they own and configure its tag subscription" (which Zuri then fulfills), or are Channels and Posters Zuri-owned only?

> Changed. Only Zuri can create posters and add them to channels now. Channels are loaded on a cold start. 

## 2. Poster owner identity
When Zuri creates a Poster for someone else's Channel, who is the User in "Owned by One and Only One User" — Zuri (the creator), or the User whose Channel it serves?

> Zuri

## 3. Channel-side cardinality
A Poster is capped at one Channel. Can one Channel host multiple Posters, or is it strictly 1:1?

> 1:1. We are not going to enforce it, but the topology assumes 1:1

## 4. Tag subscription configuration
Who configures a Poster's tag subscription? Zuri at creation time, or the requesting User?

> Zuri

## 5. Missing entities
The intro describes a report system, report-abuse prevention, content-removal requests, and removal-abuse prevention. Mods also have a ban capability. None of these have entities. Are they MVP, or out-of-scope-for-now?

> MVP. We can add them.

## 6. Submission flow vs Poster flow
Two ingestion paths exist: manual submission and Poster tag-subscription pulls. Do approved manual submissions become eligible for tag-based Poster requests, or are the two pools disjoint?

> Disjoint. Queue is peeked. If tags don't match, the next post is selected at random from the saved posts. 

## 7. "Distinct time intervals"
The intro mentions "distinct time intervals." What does "distinct" qualify — per Channel, per Poster, per tag subscription, something else?

> Per channel. Technically per poster, since a poster and a channel are coupled.

## 8. Source type set
Source types are listed by example ending in "etc." Is this a closed enum (only known types accepted) or open (any URL gets a best-effort type)?

> Closed.

## 9. Editorial scaffolding
The Zuri entity has an inline aside (`<- User Entity, but it felt right...`) and the Scheduler has a self-flagged infra note. Both read as out-of-place. Clean them up, or keep them as working notes?

> They are just notes for now
