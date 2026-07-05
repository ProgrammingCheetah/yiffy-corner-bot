// ==UserScript==
// @name         Yiffy Corner — submit to the bot
// @namespace    https://got-paws.net
// @version      1.4
// @description  Per-post 🐾 submit buttons for the Yiffy Corner curation feed: inline on Twitter/X and BlueSky (feeds included), overlays on e621/FA galleries.
// @match        https://e621.net/*
// @match        https://e926.net/*
// @match        https://www.furaffinity.net/*
// @match        https://furaffinity.net/*
// @match        https://twitter.com/*
// @match        https://x.com/*
// @match        https://bsky.app/*
// @run-at       document-idle
// @grant        GM_xmlhttpRequest
// @grant        GM_getValue
// @grant        GM_setValue
// @grant        GM_registerMenuCommand
// @connect      got-paws.net
// ==/UserScript==

(function () {
  'use strict';

  const DEFAULT_BASE = 'https://app.got-paws.net';

  GM_registerMenuCommand('Set API token (/apitoken in the bot)', () => {
    const token = prompt('Paste the token from /apitoken:', GM_getValue('ycb_token', ''));
    if (token !== null) GM_setValue('ycb_token', token.trim());
  });
  GM_registerMenuCommand('Set server URL', () => {
    const base = prompt('Bot web server URL:', GM_getValue('ycb_base', DEFAULT_BASE));
    if (base !== null) GM_setValue('ycb_base', base.trim().replace(/\/$/, ''));
  });

  const SITE = (() => {
    const h = location.hostname;
    if (/(^|\.)(twitter|x)\.com$/.test(h)) return 'x';
    if (/(^|\.)bsky\.app$/.test(h)) return 'bsky';
    if (/e(621|926)\.net$/.test(h)) return 'e6';
    if (/furaffinity\.net$/.test(h)) return 'fa';
    return null;
  })();

  // e621 has authoritative tags server-side; everything else asks you.
  function submitUrl(url, e621) {
    const base = GM_getValue('ycb_base', DEFAULT_BASE);
    const token = GM_getValue('ycb_token', '');
    if (!token) {
      alert('Yiffy Corner: set your API token first (Tampermonkey menu → Set API token).');
      return;
    }
    let tags = [];
    if (!e621) {
      const raw = prompt(
        'Tags for this piece (space-separated).\nCredit the artist with artist:<name>:',
        ''
      );
      if (raw === null) return; // cancelled
      tags = raw.split(/\s+/).filter(Boolean);
      if (!tags.filter((t) => !t.startsWith('artist:')).length) {
        alert('Yiffy Corner: at least one content tag is required.');
        return;
      }
    }
    flash('Submitting…');
    GM_xmlhttpRequest({
      method: 'POST',
      url: `${base}/api/suggest`,
      headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
      data: JSON.stringify({ url, tags }),
      onload: (res) => {
        try {
          const body = JSON.parse(res.responseText);
          flash(body.message ?? body.error ?? `HTTP ${res.status}`);
        } catch {
          flash(`HTTP ${res.status}`);
        }
      },
      onerror: () => flash('Network error — is the tunnel up?')
    });
  }

  function pawButton(getUrl, e621, styles) {
    const b = document.createElement('button');
    b.textContent = '🐾';
    b.title = 'Submit to Yiffy Corner';
    b.dataset.ycbBtn = '1';
    Object.assign(b.style, {
      border: 'none',
      background: 'transparent',
      cursor: 'pointer',
      fontSize: '15px',
      lineHeight: '1',
      padding: '6px',
      opacity: '0.8',
      ...styles
    });
    b.addEventListener('mouseenter', () => (b.style.opacity = '1'));
    b.addEventListener('mouseleave', () => (b.style.opacity = '0.8'));
    b.addEventListener('click', (e) => {
      e.preventDefault();
      e.stopPropagation();
      const url = getUrl();
      if (url) submitUrl(url, e621);
    });
    return b;
  }

  const clean = (href) => {
    const u = new URL(href, location.origin);
    u.search = '';
    u.hash = '';
    return u.href;
  };

  // --- per-site injection, feed-aware ------------------------------------

  function scan() {
    if (SITE === 'x') {
      // Every tweet card, timeline or detail: the action bar is the
      // [role=group] row; the permalink is the timestamp's link.
      for (const art of document.querySelectorAll('article[data-testid="tweet"]:not([data-ycb])')) {
        art.dataset.ycb = '1';
        const timeLink = art.querySelector('a[href*="/status/"] time')?.closest('a');
        const group = art.querySelector('[role="group"]');
        const href = timeLink?.getAttribute('href') ?? (art.closest('main') && /\/status\/\d+/.test(location.pathname) ? location.pathname : null);
        if (!group || !href) continue;
        group.appendChild(pawButton(() => clean(href), false, { marginLeft: '4px' }));
      }
    } else if (SITE === 'bsky') {
      for (const item of document.querySelectorAll(
        '[data-testid^="feedItem-by-"]:not([data-ycb]), [data-testid^="postThreadItem-by-"]:not([data-ycb])'
      )) {
        item.dataset.ycb = '1';
        const like = item.querySelector('[data-testid="likeBtn"]');
        if (!like) continue;
        const href =
          item.querySelector('a[href*="/post/"]')?.getAttribute('href') ??
          (/^\/profile\/[^/]+\/post\//.test(location.pathname) ? location.pathname : null);
        if (!href) continue;
        // React Native Web stacks every container by default (column).
        // Force the like button's wrapper into a row and sit right of it.
        const wrap = like.parentElement;
        if (wrap) {
          wrap.style.display = 'flex';
          wrap.style.flexDirection = 'row';
          wrap.style.alignItems = 'center';
        }
        const btn = pawButton(() => clean(href), false, {
          display: 'inline-flex',
          alignItems: 'center',
          marginLeft: '10px'
        });
        like.insertAdjacentElement('afterend', btn);
      }
    } else if (SITE === 'e6') {
      // Gallery thumbnails get a corner paw; instant submit (API tags).
      for (const prev of document.querySelectorAll('article.post-preview:not([data-ycb])')) {
        prev.dataset.ycb = '1';
        const a = prev.querySelector('a[href^="/posts/"]');
        if (!a) continue;
        const href = a.getAttribute('href');
        prev.style.position = 'relative';
        prev.appendChild(
          pawButton(() => clean(href), true, {
            position: 'absolute',
            top: '4px',
            right: '4px',
            zIndex: 10,
            background: 'rgba(0,0,0,.55)',
            borderRadius: '999px'
          })
        );
      }
    } else if (SITE === 'fa') {
      for (const fig of document.querySelectorAll('figure:not([data-ycb])')) {
        fig.dataset.ycb = '1';
        const a = fig.querySelector('a[href^="/view/"]');
        if (!a) continue;
        const href = a.getAttribute('href');
        fig.style.position = 'relative';
        fig.appendChild(
          pawButton(() => clean(href), false, {
            position: 'absolute',
            top: '4px',
            right: '4px',
            zIndex: 10,
            background: 'rgba(0,0,0,.55)',
            borderRadius: '999px'
          })
        );
      }
    }
  }

  // --- floating fallback on single-work pages without an action bar ------

  const DETAIL = [
    { re: /^https:\/\/e(621|926)\.net\/posts\/\d+/, e621: true },
    { re: /^https:\/\/(www\.)?furaffinity\.net\/view\/\d+/, e621: false }
  ];

  const floating = pawButton(
    () => clean(location.origin + location.pathname),
    false,
    {
      position: 'fixed',
      bottom: '18px',
      right: '18px',
      zIndex: 99999,
      padding: '10px 16px',
      borderRadius: '999px',
      background: '#5288c1',
      color: '#fff',
      fontSize: '14px',
      fontWeight: '600',
      boxShadow: '0 4px 14px rgba(0,0,0,.4)',
      display: 'none'
    }
  );
  floating.textContent = '🐾 Submit';
  floating.onclick = (e) => {
    e.preventDefault();
    const page = DETAIL.find((d) => d.re.test(location.href));
    if (page) submitUrl(clean(location.origin + location.pathname), page.e621);
  };

  function flash(text) {
    const toast = document.createElement('div');
    toast.textContent = `Yiffy Corner: ${text}`;
    Object.assign(toast.style, {
      position: 'fixed',
      bottom: '70px',
      right: '18px',
      zIndex: 99999,
      maxWidth: '320px',
      padding: '12px 16px',
      borderRadius: '12px',
      background: 'rgba(0,0,0,.88)',
      color: '#fff',
      fontSize: '13px',
      boxShadow: '0 4px 14px rgba(0,0,0,.4)'
    });
    document.body.appendChild(toast);
    setTimeout(() => toast.remove(), 6000);
  }

  function mount() {
    document.body.appendChild(floating);
    scan();
    // Feeds render as you scroll: rescan on DOM churn, debounced.
    let pending = null;
    new MutationObserver(() => {
      if (pending) return;
      pending = setTimeout(() => {
        pending = null;
        scan();
      }, 300);
    }).observe(document.body, { childList: true, subtree: true });
    // SPA navigation: keep the floating fallback in sync with the URL.
    let lastHref = '';
    setInterval(() => {
      if (location.href !== lastHref) {
        lastHref = location.href;
        floating.style.display = DETAIL.some((d) => d.re.test(location.href)) ? 'block' : 'none';
      }
    }, 400);
  }
  if (document.body) mount();
  else window.addEventListener('DOMContentLoaded', mount);
})();
