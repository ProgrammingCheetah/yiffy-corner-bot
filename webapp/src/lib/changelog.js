// The changelog, newest first. The first entry is the current version:
// the layout banner announces it until the user dismisses that exact
// version (per-user via CloudStorage), and /changelog renders it all.
export const changelog = [
  {
    version: '0.3.0',
    date: '2026-07-11',
    changes: [
      'Reports ask for a reason, and moderators see who reported — with a tappable contact.',
      'Reports desk: every open report with reasons and Take down / Dismiss, right here in the app.',
      '"More like this" link on published posts — wishes are relayed straight to the moderators.',
      'Content warnings are named in captions (cw_blood → "CW: blood") whenever a post publishes blurred.',
      'No more pictureless channel posts: refused media is re-uploaded by the bot itself.',
      'Browse: your query owns its ordering (no forced order:random), already-saved posts are hidden, and 🚫 skips a result forever.',
      'Feed tab: poster cursors with progress, the backlog after any post or #CODE, and per-poster queues with remove.',
      'A fresh coat of paint: floating nav, springier cards, tidier everything.',
      'Versioning and this changelog.'
    ]
  },
  {
    version: '0.2.0',
    date: '2026-07-05',
    changes: [
      'The Rust rewrite: one curated feed with per-channel cursors (no repost cooldowns).',
      'Whole-pool submission for e621 comics and collections.',
      'Per-channel community scoreboards and the global /highscore.',
      'Dead-media sweep with pHash duplicate detection.',
      'This Mini App: submit, review, browse, admin.'
    ]
  },
  {
    version: '0.1.0',
    date: '2025-11-20',
    changes: ['The legacy TypeScript bot: suggestions, moderation DMs, timed channel posts.']
  }
];

export const currentVersion = changelog[0].version;
