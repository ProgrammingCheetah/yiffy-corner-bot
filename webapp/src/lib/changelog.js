// The changelog, newest first. The first entry is the current version:
// the layout banner announces it until the user dismisses that exact
// version (per-user via CloudStorage), and /changelog renders it all.
//
// Release ritual: ask Ziel whether the batch bumps the MAJOR or the MINOR
// number, add the entry here (each minor version gets an `alias`), and
// bump crates/telegram_bot/Cargo.toml + webapp/package.json to match.

// Every release is an Alpha until further notice.
export const stage = 'Alpha';

export const changelog = [
  {
    version: '0.4.0',
    alias: 'Fox',
    date: '2026-07-11',
    changes: [
      'A homepage! The bottom bar keeps the daily drivers; everything else lives in tiles on Home.',
      'Real icons and motion: Lucide icons everywhere, animated loading states, and a once-a-day startup splash.',
      'Tag autocomplete on every tag field, suggested straight from e621 like its own search bar.',
      'Poster queues got their own pages: only what the poster will actually publish, paginated, with inline media Load and remove-from-feed.',
      'Profiles for users (stats, submitted artwork, admin actions) and posters (publish stats, taste editor, plain-language rules).',
      'Rules read like language now: "solo AND avian REQUIRE ((NO female) OR intersex) AND bird".',
      'Forbidden tags carry a reason, shown wherever the ban bites.',
      'Shadowbans: silently drop reports, wishes, and submissions — they never know.',
      'Browse remembers your deck when you leave; saved & historic queries moved to their own two-tab page; the deck buttons say what they do.',
      'Long names no longer wreck the Users list.'
    ]
  },
  {
    version: '0.3.0',
    alias: 'Wolf',
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
export const currentAlias = changelog[0].alias;

// "Alpha 0.3.0 “Wolf”" — the one way a release is written out.
export function releaseName(entry) {
  return `${stage} ${entry.version}${entry.alias ? ` “${entry.alias}”` : ''}`;
}
