// ==UserScript==
// @name         Yiffy Corner — submit to the bot
// @namespace    https://got-paws.net
// @version      2.7
// @description  Per-post 🐾 submit buttons for the Yiffy Corner curation feed: inline on Twitter/X and BlueSky (feeds included), overlays on e621/FA galleries. Persistent-tags panel and vim-style keyboard shortcuts for form-free, mouse-free submitting.
// @match        https://e621.net/*
// @match        https://e926.net/*
// @match        https://www.furaffinity.net/*
// @match        https://furaffinity.net/*
// @match        https://twitter.com/*
// @match        https://x.com/*
// @match        https://bsky.app/*
// @run-at       document-start
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
  GM_registerMenuCommand('Set keyboard leader key', () => {
    const leader = prompt('Leader key for shortcuts (single character):', GM_getValue('ycb_leader', '\\'));
    if (leader) GM_setValue('ycb_leader', leader[0]);
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
    annotateShortcuts(panelBody);
    panelBody.addEventListener('change', (e) => {
      if (e.target !== enable) savePanelState();
    });
    panelBody.addEventListener('input', (e) => {
      if (e.target.matches('input[type="text"]')) savePanelState();
    });
    // Same shortcut-swallowing the form needs: feeds bind single keys.
    panelBody.addEventListener('keydown', (e) => e.stopPropagation());

    panelBody.appendChild(buildStatsSection(false));
    return panelBody;
  }

  // --- session stats ------------------------------------------------------
  // Counted at submit time, persisted via GM values so the numbers span
  // pages AND sites; "session" = since the last reset. e621 submits carry
  // no client-side tags (the server fetches them), so they count toward
  // posted/per-site but not the tag chart.

  function defaultStats() {
    return { since: Date.now(), posted: 0, failed: 0, sites: {}, tags: {} };
  }
  let stats = (() => {
    try {
      return JSON.parse(GM_getValue('ycb_stats', '')) ?? defaultStats();
    } catch {
      return defaultStats();
    }
  })();

  const statsBoxes = [];

  function recordResult(ok, tags) {
    if (ok) {
      stats.posted += 1;
      if (SITE) stats.sites[SITE] = (stats.sites[SITE] ?? 0) + 1;
      for (const t of tags) stats.tags[t] = (stats.tags[t] ?? 0) + 1;
    } else {
      stats.failed += 1;
    }
    GM_setValue('ycb_stats', JSON.stringify(stats));
    renderStats();
  }

  function renderStats() {
    const topN = GM_getValue('ycb_stats_topn', 5);
    const top = Object.entries(stats.tags)
      .sort((a, b) => b[1] - a[1])
      .slice(0, topN);
    for (const box of statsBoxes) {
      box.querySelector('[data-s="line"]').textContent =
        `${stats.posted} posted · ${stats.failed} failed`;
      box.querySelector('[data-s="sites"]').textContent = Object.entries(stats.sites)
        .map(([site, count]) => `${site} ${count}`)
        .join(' · ');
      box.querySelector('[data-s="tags"]').textContent = top.length
        ? top.map(([tag, count]) => `${tag} ×${count}`).join(', ')
        : 'no tags yet';
      box.querySelector('[data-s="since"]').textContent =
        `since ${new Date(stats.since).toLocaleString()}`;
    }
  }

  // In the panel where there is one, its own small box on e621.
  function buildStatsSection(standalone) {
    const box = document.createElement('div');
    if (standalone) {
      Object.assign(box.style, {
        position: 'fixed',
        bottom: '18px',
        left: '18px',
        zIndex: 99998,
        background: '#1b1e23',
        color: '#fff',
        borderRadius: '12px',
        padding: '10px 12px',
        font: '12px system-ui, sans-serif',
        boxShadow: '0 4px 14px rgba(0,0,0,.4)',
        maxWidth: '230px'
      });
    } else {
      Object.assign(box.style, {
        borderTop: '1px solid #333',
        paddingTop: '10px',
        fontSize: '12px'
      });
    }
    box.innerHTML = `
      <div style="display:flex;align-items:center;gap:6px;font-weight:600">
        📊 Session stats
        <button type="button" data-s="reset" style="margin-left:auto;border:none;background:transparent;color:#f87171;cursor:pointer;font:inherit">reset</button>
      </div>
      <div data-s="line" style="opacity:.85;margin-top:4px"></div>
      <div data-s="sites" style="opacity:.6;margin-top:2px"></div>
      <label style="display:flex;align-items:center;gap:6px;margin-top:6px;opacity:.85;cursor:pointer">top
        <input data-s="n" type="number" min="1" max="50" style="width:48px;background:#2a2e35;color:#fff;border:1px solid #444;border-radius:6px;padding:2px 4px;font:inherit"> tags
      </label>
      <div data-s="tags" style="margin-top:4px;line-height:1.5;overflow-wrap:anywhere"></div>
      <div data-s="since" style="opacity:.45;margin-top:4px"></div>`;
    const n = box.querySelector('[data-s="n"]');
    n.value = GM_getValue('ycb_stats_topn', 5);
    n.addEventListener('change', () => {
      GM_setValue('ycb_stats_topn', Math.min(50, Math.max(1, parseInt(n.value, 10) || 5)));
      n.value = GM_getValue('ycb_stats_topn', 5);
      renderStats();
    });
    box.querySelector('[data-s="reset"]').addEventListener('click', () => {
      stats = defaultStats();
      GM_setValue('ycb_stats', JSON.stringify(stats));
      renderStats();
    });
    box.addEventListener('keydown', (e) => e.stopPropagation());
    statsBoxes.push(box);
    renderStats();
    return box;
  }

  // --- vim-style keyboard shortcuts ---------------------------------------
  // j/k walk the page's posts (centered + highlighted), ctrl+a / ctrl+x
  // step the character count up/down, <leader> sequences toggle panel
  // fields (gm/gf/gi/gu genders, pg/ps/pl pairings, r irl, et focuses the
  // extra-tags input), and ctrl+s submits the highlighted post with the
  // panel's current tags — no form, no mouse. Leader defaults to vim's
  // "\" (Tampermonkey menu to change it). Keys are ignored while typing,
  // except ctrl+s from inside our own panel.

  const POST_SELECTOR = {
    x: 'article[data-testid="tweet"]',
    bsky: '[data-testid^="feedItem-by-"], [data-testid^="postThreadItem-by-"]',
    e6: 'article.post-preview',
    fa: 'figure'
  }[SITE];

  let currentPost = null;

  function highlightPost(el) {
    if (currentPost) {
      currentPost.style.outline = '';
      currentPost.style.outlineOffset = '';
      currentPost.querySelector('video')?.pause();
    }
    currentPost = el;
    el.style.outline = '2px solid #5288c1';
    el.style.outlineOffset = '2px';
    el.scrollIntoView({ behavior: 'smooth', block: 'center' });
    // Feeds gate autoplay on their own scroll heuristics, which programmatic
    // scrolling doesn't trip — start the selected post's video ourselves.
    el.querySelector('video')?.play().catch(() => {});
  }

  function movePost(dir) {
    const posts = [...document.querySelectorAll(POST_SELECTOR)];
    if (!posts.length) return;
    let i = posts.indexOf(currentPost);
    // Nothing highlighted yet (or the feed recycled the node): start from
    // the first post still visible instead of stepping from the DOM top.
    if (i === -1) i = Math.max(0, posts.findIndex((p) => p.getBoundingClientRect().bottom > 0));
    else i += dir;
    highlightPost(posts[Math.max(0, Math.min(posts.length - 1, i))]);
  }

  function postUrl(el) {
    if (SITE === 'x') {
      const a = el.querySelector('a[href*="/status/"] time')?.closest('a');
      return a ? clean(a.getAttribute('href')) : null;
    }
    const a = el.querySelector(
      { bsky: 'a[href*="/post/"]', e6: 'a[href^="/posts/"]', fa: 'a[href^="/view/"]' }[SITE]
    );
    return a ? clean(a.getAttribute('href')) : null;
  }

  function submitCurrent() {
    let url = null;
    let e621 = SITE === 'e6';
    if (currentPost && document.contains(currentPost)) {
      url = postUrl(currentPost);
    } else {
      const page = DETAIL.find((d) => d.re.test(location.href));
      if (page) {
        url = clean(location.origin + location.pathname);
        e621 = page.e621;
      }
    }
    if (!url) {
      flash('nothing to submit — j/k to pick a post');
      return;
    }
    if (e621) {
      post(url, []);
      refocusPost();
      return;
    }
    const read = readTags(panelBody);
    if (read.error) {
      flash(`persistent tags: ${read.error}`);
      return; // keep focus in the panel — it's what needs fixing
    }
    post(url, read.tags);
    refocusPost();
  }

  // ctrl+s from the panel's extra-tags input would otherwise leave focus
  // there, turning the next j/k into typing. Hand focus back to the
  // selected post once the submit is on its way.
  function refocusPost() {
    if (panelBody?.contains(document.activeElement)) document.activeElement.blur();
    if (!currentPost || !document.contains(currentPost)) return;
    if (!currentPost.hasAttribute('tabindex')) currentPost.setAttribute('tabindex', '-1');
    currentPost.focus({ preventScroll: true });
  }

  function togglePanelField(name, value) {
    if (!panelBody) return; // e621: server-side tags, no panel
    const input = panelBody.querySelector(`input[name="${name}"][value="${value}"]`);
    input.checked = !input.checked;
    input.dispatchEvent(new Event('change', { bubbles: true }));
    flash(`${value}: ${input.checked ? 'on' : 'off'}`);
  }

  function cycleCount(dir) {
    if (!panelBody) return;
    const sel = panelBody.querySelector('select');
    const next = COUNTS[Math.max(0, Math.min(COUNTS.length - 1, COUNTS.indexOf(sel.value) + dir))];
    sel.value = next;
    sel.dispatchEvent(new Event('change', { bubbles: true }));
    flash(`characters: ${next}`);
  }

  // Like/repost the selected post via its own action buttons. X's native
  // "l" acts on X's internal focus, not our cursor — and we mute their
  // handlers for keys we take anyway.
  function xAction(kind) {
    if (!currentPost || !document.contains(currentPost)) {
      flash('no post selected — j/k to pick one');
      return;
    }
    if (kind === 'like') {
      currentPost.querySelector('[data-testid="like"], [data-testid="unlike"]')?.click();
      return;
    }
    const rt = currentPost.querySelector('[data-testid="retweet"], [data-testid="unretweet"]');
    if (!rt) return;
    rt.click();
    // The confirm button lives in a menu that mounts async — poll briefly.
    let tries = 20;
    const timer = setInterval(() => {
      const btn = document.querySelector(
        '[data-testid="retweetConfirm"], [data-testid="unretweetConfirm"]'
      );
      if (btn) {
        btn.click();
        clearInterval(timer);
      } else if (--tries <= 0) {
        clearInterval(timer);
      }
    }, 50);
  }

  // Ratings are radios: the shortcut selects, it doesn't toggle.
  function setPanelRating(value) {
    if (!panelBody) return;
    const input = panelBody.querySelector(`input[name="rating"][value="${value}"]`);
    input.checked = true;
    input.dispatchEvent(new Event('change', { bubbles: true }));
    flash(`rating: ${value}`);
  }

  const SEQUENCES = {
    gm: () => togglePanelField('gender', 'male'),
    gf: () => togglePanelField('gender', 'female'),
    gi: () => togglePanelField('gender', 'intersex'),
    gu: () => togglePanelField('gender', 'unknown'),
    pg: () => togglePanelField('pairing', 'male/male'),
    ps: () => togglePanelField('pairing', 'male/female'),
    pl: () => togglePanelField('pairing', 'female/female'),
    r: () => togglePanelField('irl', 'irl'),
    n: () => setPanelRating('NSFW'),
    s: () => setPanelRating('SFW'),
    q: () => setPanelRating('Questionable'),
    et: () => panelBody?.querySelector('input[type="text"]').focus()
  };

  // Hover hints on the panel so the shortcuts stay discoverable.
  function annotateShortcuts(root) {
    const L = GM_getValue('ycb_leader', '\\');
    const hints = [
      ['gender', 'male', `${L}gm`],
      ['gender', 'female', `${L}gf`],
      ['gender', 'intersex', `${L}gi`],
      ['gender', 'unknown', `${L}gu`],
      ['pairing', 'male/male', `${L}pg`],
      ['pairing', 'male/female', `${L}ps`],
      ['pairing', 'female/female', `${L}pl`],
      ['irl', 'irl', `${L}r`],
      ['rating', 'NSFW', `${L}n`],
      ['rating', 'SFW', `${L}s`],
      ['rating', 'Questionable', `${L}q`]
    ];
    for (const [name, value, keys] of hints) {
      const label = root.querySelector(`input[name="${name}"][value="${value}"]`)?.closest('label');
      if (label) label.title = keys;
    }
    root.querySelector('select').closest('label').title = 'ctrl+a / ctrl+x';
    root.querySelector('input[type="text"]').closest('label').title = `${L}et`;
    root.querySelector('[data-p="enable"]').closest('label').title =
      'ctrl+s submits the highlighted post (j/k to pick) with these tags';
  }

  // Low-opacity cheat-sheet pinned to the left edge; solid on hover.
  function buildCheatSheet() {
    const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;');
    const L = esc(GM_getValue('ycb_leader', '\\'));
    const rows = [
      ['j / k', 'next / prev post'],
      ['click', 'select post'],
      ['ctrl+s', 'submit selected']
    ];
    if (SITE === 'x') rows.push(['h', 'like'], ['l', 'repost']);
    // The rest drive the panel, which e621 doesn't have.
    if (SITE !== 'e6') {
      rows.push(
        ['ctrl+a / ctrl+x', 'characters + / −'],
        [`${L}gm ${L}gf ${L}gi ${L}gu`, 'genders'],
        [`${L}pg ${L}ps ${L}pl`, 'pairings'],
        [`${L}n ${L}s ${L}q`, 'rating'],
        [`${L}r`, 'irl'],
        [`${L}et`, 'extra tags']
      );
    }
    const box = document.createElement('div');
    Object.assign(box.style, {
      position: 'fixed',
      left: '0',
      top: '50%',
      transform: 'translateY(-50%)',
      zIndex: 99997,
      background: '#1b1e23',
      color: '#fff',
      borderRadius: '0 12px 12px 0',
      padding: '10px 12px',
      font: '11px ui-monospace, monospace',
      lineHeight: '1.7',
      opacity: '0.35',
      transition: 'opacity .15s'
    });
    box.addEventListener('mouseenter', () => (box.style.opacity = '1'));
    box.addEventListener('mouseleave', () => (box.style.opacity = '0.35'));
    box.innerHTML = rows
      .map(([keys, what]) => `<div><b style="color:#8fb8e8">${keys}</b> <span style="opacity:.75">${what}</span></div>`)
      .join('');
    return box;
  }

  let seq = null; // null = idle; '' = leader pressed, collecting keys
  let seqTimer = null;

  function endSeq() {
    seq = null;
    clearTimeout(seqTimer);
  }

  function installKeys() {
    // Window capture, registered at document-start — X binds at window
    // capture too, and same-node/same-phase listeners fire in registration
    // order, so being first is the only thing stopImmediatePropagation
    // respects. Running before the page's own scripts guarantees it; the
    // feeds' single-key bindings (X uses j/k too) then never see the keys
    // we handle. Swallowing the keydown alone isn't enough — X reacts to
    // the keyup/keypress tail as well, so those get muted for handled
    // keys too.
    const swallowUp = new Set();
    const swallow = (e) => {
      e.preventDefault();
      e.stopImmediatePropagation();
      swallowUp.add(e.key);
    };
    for (const type of ['keyup', 'keypress']) {
      window.addEventListener(
        type,
        (e) => {
          if (!swallowUp.has(e.key)) return;
          if (type === 'keyup') swallowUp.delete(e.key);
          e.stopImmediatePropagation();
        },
        true
      );
    }
    window.addEventListener(
      'keydown',
      (e) => {
        const typing =
          e.target instanceof Element &&
          e.target.closest('input, textarea, select, [contenteditable="true"]');
        if (e.ctrlKey && !e.altKey && !e.metaKey) {
          endSeq();
          const key = e.key.toLowerCase();
          if (key === 's' && (!typing || panelBody?.contains(e.target))) {
            swallow(e);
            submitCurrent();
          } else if (!typing && key === 'a') {
            swallow(e);
            cycleCount(1);
          } else if (!typing && key === 'x') {
            swallow(e);
            cycleCount(-1);
          }
          return;
        }
        if (typing || e.altKey || e.metaKey) return;
        if (seq !== null) {
          if (e.key.length !== 1) return; // bare modifier mid-sequence
          swallow(e);
          seq += e.key;
          const hit = SEQUENCES[seq];
          if (hit) {
            endSeq();
            hit();
          } else if (!Object.keys(SEQUENCES).some((s) => s.startsWith(seq))) {
            endSeq();
          }
          return;
        }
        if (e.key === GM_getValue('ycb_leader', '\\')) {
          swallow(e);
          seq = '';
          seqTimer = setTimeout(endSeq, 1500);
        } else if (e.key === 'j') {
          swallow(e);
          movePost(1);
        } else if (e.key === 'k') {
          swallow(e);
          movePost(-1);
        } else if (SITE === 'x' && e.key === 'h') {
          swallow(e);
          xAction('like');
        } else if (SITE === 'x' && e.key === 'l') {
          swallow(e);
          xAction('repost');
        }
      },
      true
    );
    // Mouse picks a post too: center + highlight it so j/k continue from
    // there. Capture phase, no preventDefault — the click still lands.
    window.addEventListener(
      'click',
      (e) => {
        if (!POST_SELECTOR || !(e.target instanceof Element)) return;
        const post = e.target.closest(POST_SELECTOR);
        if (post && post !== currentPost) highlightPost(post);
      },
      true
    );
  }

  // e621 has authoritative tags server-side; everything else gets the form —
  // unless the persistent panel is on, which submits its tags form-free.
  // `forceForm` (the 📝 buttons) always opens the form, toggle be damned.
  function apiToken() {
    const token = GM_getValue('ycb_token', '');
    if (!token) alert('Yiffy Corner: set your API token first (Tampermonkey menu → Set API token).');
    return token;
  }

  function post(url, tags) {
    const base = GM_getValue('ycb_base', DEFAULT_BASE);
    const token = apiToken();
    if (!token) return;
    flash('Submitting…');
    GM_xmlhttpRequest({
      method: 'POST',
      url: `${base}/api/suggest`,
      headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
      data: JSON.stringify({ url, tags }),
      onload: (res) => {
        recordResult(res.status >= 200 && res.status < 300, tags);
        try {
          const body = JSON.parse(res.responseText);
          flash(body.message ?? body.error ?? `HTTP ${res.status}`);
        } catch {
          flash(`HTTP ${res.status}`);
        }
      },
      onerror: () => {
        recordResult(false, tags);
        flash('Network error — is the tunnel up?');
      }
    });
  }

  async function submitUrl(url, e621, forceForm = false) {
    if (!apiToken()) return;
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
    post(url, tags);
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
    // where the form would have appeared. Stats still deserve a home there.
    if (SITE && SITE !== 'e6') document.body.appendChild(buildPanel());
    else if (SITE === 'e6') document.body.appendChild(buildStatsSection(true));
    document.body.appendChild(buildCheatSheet());
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
  // Keys go in NOW (document-start, before the page's scripts run — see
  // installKeys); everything that needs a DOM waits for the body.
  installKeys();
  if (document.body) mount();
  else window.addEventListener('DOMContentLoaded', mount);
})();
