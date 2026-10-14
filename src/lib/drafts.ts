// Drafts outlive editor components and closed tabs until explicitly saved.
export class Drafts {
  #texts = new Map<string, string>();

  get(key: string): string | undefined { return this.#texts.get(key); }
  set(key: string, text: string) { this.#texts.set(key, text); }
  delete(key: string) { this.#texts.delete(key); }
  clear() { this.#texts.clear(); }
  keys() { return [...this.#texts.keys()]; }

  async save(writer: (key: string, text: string) => Promise<unknown>, key?: string) {
    const entries = key === undefined
      ? [...this.#texts.entries()]
      : this.#texts.has(key) ? [[key, this.#texts.get(key)!] as const] : [];
    const saved: string[] = [];
    for (const [id, text] of entries) {
      await writer(id, text);
      // Typing during an asynchronous save must remain an unsaved draft.
      if (this.#texts.get(id) === text) this.#texts.delete(id);
      saved.push(id);
    }
    return saved;
  }
}
