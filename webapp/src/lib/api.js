// One fetch wrapper: inside Telegram we sign with initData; on desktop
// (userscript/dev) a personal token from /apitoken lives in localStorage.
function authHeader() {
  const initData = window.Telegram?.WebApp?.initData;
  if (initData) return `tma ${initData}`;
  const token = localStorage.getItem('ycb_token');
  return token ? `Bearer ${token}` : '';
}

export async function api(path, options = {}) {
  const res = await fetch(`/api${path}`, {
    ...options,
    headers: {
      'Content-Type': 'application/json',
      Authorization: authHeader(),
      ...(options.headers ?? {})
    }
  });
  const body = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(body.error ?? `HTTP ${res.status}`);
  return body;
}

export const get = (path) => api(path);
export const post = (path, body) => api(path, { method: 'POST', body: JSON.stringify(body) });
export const patch = (path, body) => api(path, { method: 'PATCH', body: JSON.stringify(body) });
export const del = (path) => api(path, { method: 'DELETE' });
