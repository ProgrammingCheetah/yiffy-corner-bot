// ==UserScript==
// @name         Yiffy Corner — submit to the bot
// @namespace    https://got-paws.net
// @version      1.10
// @description  Per-post 🐾 submit buttons for the Yiffy Corner curation feed: inline on Twitter/X and BlueSky (feeds included), overlays on e621/FA galleries. Persistent-tags panel for form-free submitting.
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
  GM_registerMenuCommand('Toggle persistent tags', () => {
    GM_setValue('ycb_panel_on', !GM_getValue('ycb_panel_on', false));
    location.reload();
  });

  const SITE = (() => {
    const h = location.hostname;
    if (/(^|\.)(twitter|x)\.com$/.test(h)) return 'x';
    if (/(^|\.)bsky\.app$/.test(h)) return 'bsky';
    if (/e(621|926)\.net$/.test(h)) return 'e6';
    if (/furaffinity\.net$/.test(h)) return 'fa';
    return null;
  })();

  // Non-e621 pieces are described through a small form: gender (any
  // number), character count (exactly one), optional pairings, a required
  // content rating, an irl checkbox, and a free-text row for artist
  // credit + extra tags.
  const GENDERS = ['male', 'female', 'intersex', 'unknown'];
  const COUNTS = ['solo', 'duo', 'multiple'];
  const PAIRINGS = ['male/male', 'male/female', 'female/female'];
  const RATINGS = ['NSFW', 'SFW', 'Questionable'];

  const check = (group, value, type = 'checkbox') =>
    `<label style="display:inline-flex;align-items:center;gap:5px;margin:0 12px 6px 0;cursor:pointer">
       <input type="${type}" name="${group}" value="${value}">${value}
     </label>`;
  const legend = (text) => `<div style="opacity:.7;margin-bottom:4px">${text}</div>`;

  // The one description of a piece, shared by the modal form and the
  // persistent panel so they can never drift apart.
  const fieldsHtml = () => `
    <div>
      ${legend('Gender (pick all that apply)')}
      ${GENDERS.map((g) => check('gender', g)).join('')}
    </div>
    <label style="display:flex;flex-direction:column;gap:4px">
      <span style="opacity:.7">Characters</span>
      <select required style="background:#2a2e35;color:#fff;border:1px solid #444;border-radius:8px;padding:7px 10px;font:inherit">
        <option value="" disabled selected>choose…</option>
        ${COUNTS.map((c) => `<option value="${c}">${c}</option>`).join('')}
      </select>
    </label>
    <div>
      ${legend('Pairings (optional)')}
      ${PAIRINGS.map((p) => check('pairing', p)).join('')}
    </div>
    <div>
      ${legend('Content rating')}
      ${RATINGS.map((r) => check('rating', r, 'radio')).join('')}
    </div>
    ${check('irl', 'irl')}
    <label style="display:flex;flex-direction:column;gap:4px">
      <span style="opacity:.7">Extra tags — credit the artist with artist:&lt;name&gt;</span>
      <input type="text" spellcheck="false" style="background:#2a2e35;color:#fff;border:1px solid #444;border-radius:8px;padding:7px 10px;font:inherit">
    </label>`;

  // Read the fields under `root` into a tag list, or an error message.
  function readTags(root) {
    const picked = (name) =>
      [...root.querySelectorAll(`input[name="${name}"]:checked`)].map((i) => i.value);
    const genders = picked('gender');
    if (!genders.length) return { error: 'pick at least one gender' };
    const count = root.querySelector('select').value;
    if (!count) return { error: 'pick a character count' };
    const rating = root.querySelector('input[name="rating"]:checked')?.value;
    if (!rating) return { error: 'pick a content rating' };
    const extra = root
      .querySelector('input[type="text"]')
      .value.split(/\s+/)
      .filter(Boolean);
    return {
      tags: [...genders, count, ...picked('pairing'), rating.toLowerCase(), ...picked('irl'), ...extra]
    };
  }

  function tagForm() {
    return new Promise((resolve) => {
      const overlay = document.createElement('div');
      Object.assign(overlay.style, {
        position: 'fixed',
        inset: '0',
        zIndex: 100000,
        background: 'rgba(0,0,0,.55)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center'
      });

      const form = document.createElement('form');
      Object.assign(form.style, {
        background: '#1b1e23',
        color: '#fff',
        borderRadius: '14px',
        padding: '18px 20px',
        width: '340px',
        maxWidth: '92vw',
        font: '14px system-ui, sans-serif',
        boxShadow: '0 8px 30px rgba(0,0,0,.5)',
        display: 'flex',
        flexDirection: 'column',
        gap: '12px'
      });
      form.innerHTML = `
        <strong style="font-size:15px">🐾 Submit to Yiffy Corner</strong>
        ${fieldsHtml()}
        <div style="display:flex;gap:8px;justify-content:flex-end">
          <button type="button" data-f="cancel" style="border:none;background:transparent;color:#aaa;cursor:pointer;padding:8px 12px;font:inherit">Cancel</button>
          <button type="submit" style="border:none;background:#5288c1;color:#fff;cursor:pointer;padding:8px 16px;border-radius:999px;font:inherit;font-weight:600">Submit</button>
        </div>`;
      overlay.appendChild(form);
      document.body.appendChild(overlay);

      // Keep keystrokes in the form: feed sites bind single-key shortcuts
      // (X likes on "l") on the document.
      overlay.addEventListener('keydown', (e) => {
        e.stopPropagation();
        if (e.key === 'Escape') done(null);
      });
      overlay.addEventListener('click', (e) => {
        if (e.target === overlay) done(null);
      });

      const done = (tags) => {
        overlay.remove();
        resolve(tags);
      };
      form.querySelector('[data-f="cancel"]').addEventListener('click', () => done(null));
      form.addEventListener('submit', (e) => {
        e.preventDefault();
        e.stopPropagation();
        const read = readTags(form);
        if (read.error) {
          alert(`Yiffy Corner: ${read.error}.`);
          return;
        }
        done(read.tags);
      });
      form.querySelector('input[type="checkbox"]').focus();
    });
  }

  // --- persistent-tags panel ---------------------------------------------
  // A floating panel on the right with the same fields as the form. While
  // its toggle is ON, submits tag with whatever the panel currently holds
  // and the modal never appears. Selections persist across pages & sites.

  let panelBody = null;

  function savePanelState() {
    if (!panelBody) return;
    const picked = (name) =>
      [...panelBody.querySelectorAll(`input[name="${name}"]:checked`)].map((i) => i.value);
    GM_setValue('ycb_panel_state', JSON.stringify({
      gender: picked('gender'),
      count: panelBody.querySelector('select').value,
      pairing: picked('pairing'),
      rating: panelBody.querySelector('input[name="rating"]:checked')?.value ?? '',
      irl: !!panelBody.querySelector('input[name="irl"]:checked'),
      extra: panelBody.querySelector('input[type="text"]').value
    }));
  }

  function restorePanelState() {
    let s = {};
    try { s = JSON.parse(GM_getValue('ycb_panel_state', '{}')); } catch { /* fresh */ }
    for (const input of panelBody.querySelectorAll('input[type="checkbox"], input[type="radio"]')) {
      if (input.name === 'gender') input.checked = (s.gender ?? []).includes(input.value);
      else if (input.name === 'pairing') input.checked = (s.pairing ?? []).includes(input.value);
      else if (input.name === 'rating') input.checked = s.rating === input.value;
      else if (input.name === 'irl') input.checked = !!s.irl;
    }
    panelBody.querySelector('select').value = s.count ?? '';
    panelBody.querySelector('input[type="text"]').value = s.extra ?? '';
  }

  function buildPanel() {
    panelBody = document.createElement('div');
    Object.assign(panelBody.style, {
      position: 'fixed',
      top: '50%',
      right: '0',
      transform: 'translateY(-50%)',
      zIndex: 99998,
      font: '13px system-ui, sans-serif',
      background: '#1b1e23',
      color: '#fff',
      border: '1px solid transparent',
      borderRight: 'none',
      borderRadius: '14px 0 0 14px',
      padding: '14px 16px',
      width: '250px',
      maxHeight: '80vh',
      overflowY: 'auto',
      boxShadow: '0 8px 30px rgba(0,0,0,.5)',
      display: 'flex',
      flexDirection: 'column',
      gap: '10px'
    });
    panelBody.innerHTML = `
      <label style="display:flex;align-items:center;gap:8px;cursor:pointer;font-weight:600;font-size:14px">
        <input type="checkbox" data-p="enable">🐾 Persistent tags
      </label>
      <div style="opacity:.6;font-size:12px" data-p="hint"></div>
      <div data-p="fields" style="display:flex;flex-direction:column;gap:10px">${fieldsHtml()}</div>`;

    const enable = panelBody.querySelector('[data-p="enable"]');
    const hint = panelBody.querySelector('[data-p="hint"]');
    const fields = panelBody.querySelector('[data-p="fields"]');
    const syncEnabled = () => {
      const on = GM_getValue('ycb_panel_on', false);
      enable.checked = on;
      panelBody.style.borderColor = on ? '#5288c1' : 'transparent';
      fields.style.opacity = on ? '1' : '0.55';
      hint.textContent = on
        ? 'ON — 🐾 submits with these tags, no form.'
        : 'OFF — 🐾 opens the usual form.';
    };
    enable.addEventListener('change', () => {
      GM_setValue('ycb_panel_on', enable.checked);
      syncEnabled();
    });
    syncEnabled();
    restorePanelState();
    panelBody.addEventListener('change', (e) => {
      if (e.target !== enable) savePanelState();
    });
    panelBody.addEventListener('input', (e) => {
      if (e.target.matches('input[type="text"]')) savePanelState();
    });
    // Same shortcut-swallowing the form needs: feeds bind single keys.
    panelBody.addEventListener('keydown', (e) => e.stopPropagation());

    return panelBody;
  }

  // e621 has authoritative tags server-side; everything else gets the form —
  // unless the persistent panel is on, which submits its tags form-free.
  // `forceForm` (the 📝 buttons) always opens the form, toggle be damned.
  async function submitUrl(url, e621, forceForm = false) {
    const base = GM_getValue('ycb_base', DEFAULT_BASE);
    const token = GM_getValue('ycb_token', '');
    if (!token) {
      alert('Yiffy Corner: set your API token first (Tampermonkey menu → Set API token).');
      return;
    }
    let tags = [];
    if (!e621) {
      if (!forceForm && panelBody && GM_getValue('ycb_panel_on', false)) {
        const read = readTags(panelBody);
        if (read.error) {
          flash(`persistent tags: ${read.error} — opening the form`);
          tags = await tagForm();
          if (!tags) return; // cancelled
        } else {
          tags = read.tags;
        }
      } else {
        tags = await tagForm();
        if (!tags) return; // cancelled
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

  function pawButton(getUrl, e621, styles, forceForm = false) {
    const b = document.createElement('button');
    b.textContent = forceForm ? '📝' : '🐾';
    b.title = forceForm
      ? 'Submit to Yiffy Corner via the form (ignores persistent tags)'
      : 'Submit to Yiffy Corner';
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
      if (url) submitUrl(url, e621, forceForm);
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
      // Timelines RECYCLE nodes, so injection is idempotent per card and
      // the permalink resolves at CLICK time from the button's current card.
      for (const art of document.querySelectorAll('article[data-testid="tweet"]')) {
        if (art.querySelector('button[data-ycb-btn]')) continue;
        const group = art.querySelector('[role="group"]');
        if (!group) continue;
        const tweetUrl = (from) => {
          const here = from.closest('article');
          const a = here?.querySelector('a[href*="/status/"] time')?.closest('a');
          const href =
            a?.getAttribute('href') ??
            (/\/status\/\d+/.test(location.pathname) ? location.pathname : null);
          return href ? clean(href) : null;
        };
        const btn = pawButton(() => tweetUrl(btn), false, { marginLeft: '4px' });
        const formBtn = pawButton(() => tweetUrl(formBtn), false, { marginLeft: '2px' }, true);
        group.appendChild(btn);
        group.appendChild(formBtn);
      }
    } else if (SITE === 'bsky') {
      for (const item of document.querySelectorAll(
        '[data-testid^="feedItem-by-"], [data-testid^="postThreadItem-by-"]'
      )) {
        if (item.querySelector('button[data-ycb-btn]')) continue;
        const like = item.querySelector('[data-testid="likeBtn"]');
        if (!like) continue;
        // React Native Web stacks every container by default (column).
        // Force the like button's wrapper into a row and sit right of it.
        const wrap = like.parentElement;
        if (wrap) {
          wrap.style.display = 'flex';
          wrap.style.flexDirection = 'row';
          wrap.style.alignItems = 'center';
        }
        const postUrl = (from) => {
          const here = from.closest('[data-testid^="feedItem-by-"], [data-testid^="postThreadItem-by-"]');
          const link =
            here?.querySelector('a[href*="/post/"]')?.getAttribute('href') ??
            (/^\/profile\/[^/]+\/post\//.test(location.pathname) ? location.pathname : null);
          return link ? clean(link) : null;
        };
        const inline = { display: 'inline-flex', alignItems: 'center', marginLeft: '10px' };
        const btn = pawButton(() => postUrl(btn), false, inline);
        const formBtn = pawButton(() => postUrl(formBtn), false, { ...inline, marginLeft: '4px' }, true);
        like.insertAdjacentElement('afterend', formBtn);
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
        const corner = {
          position: 'absolute',
          top: '4px',
          right: '4px',
          zIndex: 10,
          background: 'rgba(0,0,0,.55)',
          borderRadius: '999px'
        };
        fig.appendChild(pawButton(() => clean(href), false, corner));
        fig.appendChild(pawButton(() => clean(href), false, { ...corner, top: '34px' }, true));
      }
    }
  }

  // --- floating fallback on single-work pages without an action bar ------

  const DETAIL = [
    { re: /^https:\/\/e(621|926)\.net\/posts\/\d+/, e621: true },
    { re: /^https:\/\/(www\.)?furaffinity\.net\/view\/\d+/, e621: false }
  ];

  const floatingStyle = {
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
  };
  const floating = pawButton(() => clean(location.origin + location.pathname), false, floatingStyle);
  floating.textContent = '🐾 Submit';
  floating.onclick = (e) => {
    e.preventDefault();
    const page = DETAIL.find((d) => d.re.test(location.href));
    if (page) submitUrl(clean(location.origin + location.pathname), page.e621);
  };
  // The 📝 twin: always the form, even with persistent tags on. Only shows
  // on non-e621 detail pages (e621 never has a form to force).
  const floatingForm = pawButton(
    () => clean(location.origin + location.pathname),
    false,
    { ...floatingStyle, right: '150px', background: '#3b4a5c' },
    true
  );
  floatingForm.textContent = '📝 Form';

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
    document.body.appendChild(floatingForm);
    // e621 tags are authoritative server-side — the panel only makes sense
    // where the form would have appeared.
    if (SITE && SITE !== 'e6') document.body.appendChild(buildPanel());
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
    // SPA navigation: keep the floating fallbacks in sync with the URL.
    let lastHref = '';
    setInterval(() => {
      if (location.href !== lastHref) {
        lastHref = location.href;
        const page = DETAIL.find((d) => d.re.test(location.href));
        floating.style.display = page ? 'block' : 'none';
        floatingForm.style.display = page && !page.e621 ? 'block' : 'none';
      }
    }, 400);
  }
  if (document.body) mount();
  else window.addEventListener('DOMContentLoaded', mount);
})();
