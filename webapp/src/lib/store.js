// Small persistent KV: Telegram CloudStorage when available (synced across
// the user's devices), localStorage otherwise (desktop/Bearer path).
function cloud() {
  const wa = window.Telegram?.WebApp;
  return wa?.CloudStorage && wa.isVersionAtLeast?.('6.9') ? wa.CloudStorage : null;
}

function getRaw(key) {
  return new Promise((resolve) => {
    const c = cloud();
    if (c) c.getItem(key, (err, value) => resolve(err ? null : value));
    else resolve(localStorage.getItem(key));
  });
}

function setRaw(key, value) {
  return new Promise((resolve) => {
    const c = cloud();
    if (c) c.setItem(key, value, () => resolve());
    else { localStorage.setItem(key, value); resolve(); }
  });
}

export async function loadJson(key, fallback) {
  try {
    const raw = await getRaw(key);
    return raw ? JSON.parse(raw) : fallback;
  } catch {
    return fallback;
  }
}

export function saveJson(key, value) {
  return setRaw(key, JSON.stringify(value));
}
