import type { CodingHistorySnapshot } from "./types";

interface HistoryDependencies {
  snapshot(): Promise<CodingHistorySnapshot>;
  save(events: unknown[], revision: number): Promise<boolean>;
  events(): unknown[];
  restore(events: unknown[]): void;
  changed(): void;
  log(): void;
}

/** Serial saves and epoch checks keep delayed I/O from resurrecting cleared evidence. */
export class HistoryPersistence {
  busy = false;
  error = false;
  loaded = false;
  private epoch = 0;
  private revision = 0;
  private enabled = false;
  private timer?: ReturnType<typeof setTimeout>;
  private writes: Promise<void> = Promise.resolve();
  constructor(private readonly deps: HistoryDependencies) {}
  get active(): boolean { return this.enabled; }
  cancel(): void { ++this.epoch; this.enabled = false; this.loaded = false; clearTimeout(this.timer); this.timer = undefined; this.busy = false; }
  async load(): Promise<void> {
    this.cancel(); this.enabled = true;
    const epoch = this.epoch; this.busy = true; this.error = false; this.deps.changed();
    try {
      const snapshot = await this.deps.snapshot();
      if (epoch !== this.epoch || !this.enabled) return;
      if (!Number.isSafeInteger(snapshot.revision) || snapshot.revision < 0 || !Array.isArray(snapshot.events) || snapshot.events.length > 512) throw new Error("invalid history envelope");
      this.revision = snapshot.revision; this.deps.restore(snapshot.events); this.loaded = true;
    } catch {
      if (epoch === this.epoch) { this.error = true; this.deps.log(); }
    } finally { if (epoch === this.epoch) { this.busy = false; this.deps.changed(); } }
  }
  schedule(): void {
    if (!this.enabled || !this.loaded) return;
    if (this.timer !== undefined) return;
    // Anchor the deadline to the first change so continuous activity is still saved.
    this.timer = setTimeout(() => { this.timer = undefined; void this.flush(); }, 500);
  }
  async flush(): Promise<void> {
    clearTimeout(this.timer);
    this.timer = undefined;
    const epoch = this.epoch;
    const write = async () => {
      if (!this.enabled || !this.loaded || epoch !== this.epoch) return;
      try {
        if (!await this.deps.save(this.deps.events(), this.revision) && epoch === this.epoch) { this.error = true; this.loaded = false; }
      } catch { if (epoch === this.epoch) { this.error = true; this.deps.log(); } }
      if (epoch === this.epoch) this.deps.changed();
    };
    this.writes = this.writes.then(write); await this.writes;
  }
}
