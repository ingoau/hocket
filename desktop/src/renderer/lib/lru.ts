/** Insertion-order LRU: `get` and `set` move the key to the newest slot; overflow drops the oldest. */
export class LruCache<V> {
  private readonly map = new Map<string, V>();

  constructor(readonly max: number) {}

  get(key: string): V | undefined {
    const v = this.map.get(key);
    if (v === undefined && !this.map.has(key)) return undefined;
    this.map.delete(key);
    this.map.set(key, v as V);
    return v;
  }

  has(key: string): boolean {
    return this.map.has(key);
  }

  set(key: string, value: V): void {
    this.map.delete(key);
    this.map.set(key, value);
    while (this.map.size > this.max) {
      const oldest = this.map.keys().next().value;
      if (oldest === undefined) break;
      this.map.delete(oldest);
    }
  }

  get size(): number {
    return this.map.size;
  }

  clear(): void {
    this.map.clear();
  }
}
