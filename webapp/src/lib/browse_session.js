// The Browse deck's state, module-scoped so it outlives the page component:
// SvelteKit keeps the JS context across client-side navigation, so leaving
// for another tab and coming back restores the exact deck — query, cards,
// and the next e621 page. A full app reopen starts fresh (the pinned/
// history lists in CloudStorage cover long-term recall).
export const session = {
  query: '',
  page: 1,
  cards: []
};
