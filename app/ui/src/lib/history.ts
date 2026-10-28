// Undo/redo for tuning (SPEC TUN-10, P9.6): up to 500 steps. A drag produces many updates; edits
// closer together than `coalesceMs` count as one step.

export class History<T> {
  private past: T[] = [];
  private future: T[] = [];
  private lastEdit = -Infinity;

  constructor(
    private readonly limit = 500,
    private readonly coalesceMs = 400,
  ) {}

  /** Call before applying an edit, with the state it replaces. */
  record(prev: T, now = Date.now()) {
    if (now - this.lastEdit > this.coalesceMs || this.past.length === 0) {
      this.past.push(structuredClone(prev));
      if (this.past.length > this.limit) this.past.shift();
    }
    this.lastEdit = now;
    this.future = [];
  }

  /** Ends the current coalescing group (the next edit starts a new step). */
  seal() {
    this.lastEdit = -Infinity;
  }

  undo(cur: T): T | null {
    const prev = this.past.pop();
    if (prev === undefined) return null;
    this.future.push(structuredClone(cur));
    this.seal();
    return prev;
  }

  redo(cur: T): T | null {
    const next = this.future.pop();
    if (next === undefined) return null;
    this.past.push(structuredClone(cur));
    this.seal();
    return next;
  }

  get canUndo() {
    return this.past.length > 0;
  }

  get canRedo() {
    return this.future.length > 0;
  }

  clear() {
    this.past = [];
    this.future = [];
    this.seal();
  }
}
