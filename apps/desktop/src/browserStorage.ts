const fallbackValues = new Map<string, string>();

const fallbackStorage: Storage = {
  get length(): number {
    return fallbackValues.size;
  },
  clear(): void {
    fallbackValues.clear();
  },
  getItem(key: string): string | null {
    return fallbackValues.get(key) ?? null;
  },
  key(index: number): string | null {
    return [...fallbackValues.keys()][index] ?? null;
  },
  removeItem(key: string): void {
    fallbackValues.delete(key);
  },
  setItem(key: string, value: string): void {
    fallbackValues.set(key, value);
  },
};

export function getBrowserStorage(): Storage {
  try {
    if (window.localStorage) {
      return window.localStorage;
    }
  } catch (error) {
    console.error(
      "Plankton local storage is unavailable; using an in-memory fallback.",
      error,
    );
  }

  try {
    Object.defineProperty(window, "localStorage", {
      configurable: true,
      value: fallbackStorage,
    });
  } catch (error) {
    console.error(
      "Plankton could not install its in-memory local storage fallback.",
      error,
    );
  }
  return fallbackStorage;
}
