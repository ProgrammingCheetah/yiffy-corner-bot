// ==UserScript==
// @name         Yiffy Corner — submit to the bot
// @namespace    https://got-paws.net
// @version      1.1
// @description  One-click submissions to the Yiffy Corner curation feed from e621, FurAffinity, Twitter/X and BlueSky.
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

  function config() {
    return {
      base: GM_getValue('ycb_base', DEFAULT_BASE),
      token: GM_getValue('ycb_token', '')
    };
  }

  GM_registerMenuCommand('Set API token (/apitoken in the bot)', () => {
    const token = prompt('Paste the token from /apitoken:', GM_getValue('ycb_token', ''));
    if (token !== null) GM_setValue('ycb_token', token.trim());
  });
  GM_registerMenuCommand('Set server URL', () => {
    const base = prompt('Bot web server URL:', GM_getValue('ycb_base', DEFAULT_BASE));
    if (base !== null) GM_setValue('ycb_base', base.trim().replace(/\/$/, ''));
  });

  // Only single-work pages are submittable. Matching happens against the
  // live URL (not @match) because X/BlueSky are SPAs — the page never
  // reloads while you browse, so the button tracks navigation instead.
  const PAGES = [
    { re: /^https:\/\/e(621|926)\.net\/posts\/\d+/, e621: true },
    { re: /^https:\/\/(www\.)?furaffinity\.net\/view\/\d+/ },
    { re: /^https:\/\/(twitter|x)\.com\/[^/]+\/status\/\d+/ },
    { re: /^https:\/\/bsky\.app\/profile\/[^/]+\/post\/[^/]+/ }
  ];
  const currentPage = () => PAGES.find((p) => p.re.test(location.href));

  function submit() {
    const page = currentPage();
    if (!page) return;
    const isE621 = !!page.e621;
    const { base, token } = config();
    if (!token) {
      alert('Yiffy Corner: set your API token first (userscript menu → Set API token).');
      return;
    }
    let tags = [];
    if (!isE621) {
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
    setBusy(true);
    GM_xmlhttpRequest({
      method: 'POST',
      url: `${base}/api/suggest`,
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${token}`
      },
      data: JSON.stringify({ url: location.origin + location.pathname, tags }),
      onload: (res) => {
        setBusy(false);
        try {
          const body = JSON.parse(res.responseText);
          flash(body.message ?? body.error ?? `HTTP ${res.status}`);
        } catch {
          flash(`HTTP ${res.status}`);
        }
      },
      onerror: () => {
        setBusy(false);
        flash('Network error — is the tunnel up?');
      }
    });
  }

  // --- UI: one floating paw button + a toast ---
  const btn = document.createElement('button');
  btn.textContent = '🐾 Submit';
  Object.assign(btn.style, {
    position: 'fixed',
    bottom: '18px',
    right: '18px',
    zIndex: 99999,
    padding: '10px 16px',
    borderRadius: '999px',
    border: 'none',
    background: '#5288c1',
    color: '#fff',
    fontSize: '14px',
    fontWeight: '600',
    cursor: 'pointer',
    boxShadow: '0 4px 14px rgba(0,0,0,.4)'
  });
  btn.addEventListener('click', submit);

  // Mount once the body exists, then follow SPA navigation.
  function mount() {
    document.body.appendChild(btn);
    let lastHref = '';
    setInterval(() => {
      if (location.href !== lastHref) {
        lastHref = location.href;
        btn.style.display = currentPage() ? 'block' : 'none';
      }
    }, 400);
  }
  if (document.body) mount();
  else window.addEventListener('DOMContentLoaded', mount);

  function setBusy(busy) {
    btn.disabled = busy;
    btn.textContent = busy ? '🐾 …' : '🐾 Submit';
  }

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
})();
