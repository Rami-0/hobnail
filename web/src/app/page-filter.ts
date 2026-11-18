/** The placeholder of this page's filter, if it has one on screen. */
export function pageFilter(): HTMLInputElement | null {
  const f = document.querySelector<HTMLInputElement>('main input[data-testid="filter"]');
  return f && f.offsetParent !== null && !f.closest("[hidden]") ? f : null;
}

