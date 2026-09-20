// Device-local UI conveniences that don't belong in the core: panel sizes,
// sidebar width, column widths. localStorage, guarded.
export function loadLocal<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(`hocket.${key}`);
    return raw === null ? fallback : (JSON.parse(raw) as T);
  } catch {
    return fallback;
  }
}

export function saveLocal<T>(key: string, value: T): void {
  try {
    localStorage.setItem(`hocket.${key}`, JSON.stringify(value));
  } catch {
    /* quota or private mode */
  }
}
