import { describe, expect, it, vi } from "vitest";
import type { Command } from "@core/api";

/** The hidden windows openHiddenPage made; loadURL fails while `failLoad` is set. */
const windows: FakeWindow[] = [];
let failLoad = false;
class FakeWindow {
  destroyed = false;
  webContents = {
    setUserAgent: () => undefined,
    setAudioMuted: () => undefined,
    isCrashed: () => false,
    executeJavaScript: async () => undefined,
  };
  constructor() {
    windows.push(this);
  }
  loadURL(): Promise<void> {
    return failLoad ? Promise.reject(new Error("ERR_INTERNET_DISCONNECTED (-106)")) : Promise.resolve();
  }
  isDestroyed(): boolean {
    return this.destroyed;
  }
  destroy(): void {
    this.destroyed = true;
  }
}
vi.mock("electron", () => ({ BrowserWindow: FakeWindow, session: {} }));

const { YtmJsHost, callFor, answerOf, openHiddenPage } = await import("./ytm-js-host");
type JsPage = import("./ytm-js-host").JsPage;

const SCRIPT = "/* host script */";

function errorOf(c: Command | undefined): string | undefined {
  return c?.type === "ytmJsAnswer" ? c.data.error : undefined;
}

function answerOf4(c: Command | undefined): string | undefined {
  return c?.type === "ytmJsAnswer" ? c.data.answer?.poToken : undefined;
}

/** A page whose `hocketYtm` is faked: it answers only once the host script ran. */
class FakePage implements JsPage {
  scripts = 0;
  calls: string[] = [];
  closed = false;
  failNext: string | undefined;

  async run(code: string): Promise<unknown> {
    if (this.closed) throw new Error("the page is gone");
    if (code === SCRIPT) {
      this.scripts++;
      return undefined;
    }
    if (this.scripts === 0) throw new Error("hocketYtm is not defined");
    this.calls.push(code);
    if (this.failNext) {
      const m = this.failNext;
      this.failNext = undefined;
      throw new Error(m);
    }
    const prepare = /^hocketYtm\.prepare\((.*), (.*)\)$/.exec(code);
    if (prepare) return { signatureTimestamp: 20767, poToken: `pot-for-${JSON.parse(prepare[2] ?? "null")}`, extra: 1 };
    const decipher = /^hocketYtm\.decipher\((".*?"), (\[.*\])\)$/.exec(code);
    if (decipher) {
      const formats = JSON.parse(decipher[2] ?? "[]") as string[];
      return { urls: formats.map((f, i) => `https://rr1.googlevideo.com/videoplayback?signed=${i}&len=${f.length}`) };
    }
    throw new Error(`unexpected ${code}`);
  }

  close(): void {
    this.closed = true;
  }
}

function at(pages: FakePage[], i: number): FakePage {
  const p = pages[i];
  if (!p) throw new Error(`no page ${i}`);
  return p;
}

function host(pages: FakePage[], idleMs = 60_000) {
  const sent: Command[] = [];
  let opened = 0;
  const h = new YtmJsHost(
    async () => {
      const p = new FakePage();
      pages.push(p);
      opened++;
      return p;
    },
    async () => SCRIPT,
    (c) => sent.push(c),
    idleMs,
  );
  return { h, sent, opened: () => opened };
}

const prepare = { type: "prepare" as const, data: { video_id: "4D7u5KF7SP8", visitor_data: "CgtWaXNpdG9y" } };
const decipher = {
  type: "decipher" as const,
  data: { video_id: "4D7u5KF7SP8", formats: ["s=ABC&sp=sig&url=https%3A%2F%2Frr1.googlevideo.com%2Fvideoplayback%3Fitag%3D141", "https://rr1.googlevideo.com/videoplayback?n=xyz"] },
};

describe("YouTube Music JS host", () => {
  it("loads the host script once, then answers each request through hocketYtm", async () => {
    const pages: FakePage[] = [];
    const { h, sent, opened } = host(pages);
    await h.handle("r1", prepare);
    await h.handle("r2", decipher);
    expect(opened()).toBe(1);
    expect(at(pages, 0).scripts).toBe(1);
    expect(sent).toEqual([
      { type: "ytmJsAnswer", data: { request_id: "r1", answer: { signatureTimestamp: 20767, poToken: "pot-for-CgtWaXNpdG9y" }, error: undefined } },
      {
        type: "ytmJsAnswer",
        data: {
          request_id: "r2",
          answer: { urls: [expect.stringContaining("signed=0"), expect.stringContaining("signed=1")] },
          error: undefined,
        },
      },
    ]);
    h.close();
  });

  it("answers a failure with its error and makes a dead page again", async () => {
    const pages: FakePage[] = [];
    const { h, sent, opened } = host(pages);
    await h.handle("r1", prepare);
    at(pages, 0).failNext = "BotGuard: no VM";
    await h.handle("r2", prepare);
    expect(sent.at(1)).toEqual({ type: "ytmJsAnswer", data: { request_id: "r2", answer: undefined, error: "BotGuard: no VM" } });
    // Still alive: reused.
    await h.handle("r3", prepare);
    expect(opened()).toBe(1);
    // It crashed: the next request gets a new page with the script in it.
    at(pages, 0).close();
    await h.handle("r4", prepare);
    expect(errorOf(sent.at(3))).toBe("the page is gone");
    await h.handle("r5", prepare);
    expect(opened()).toBe(2);
    expect(at(pages, 1).scripts).toBe(1);
    expect(answerOf4(sent.at(4))).toBe("pot-for-CgtWaXNpdG9y");
    h.close();
  });

  it("closes the page once idle", async () => {
    vi.useFakeTimers();
    try {
      const pages: FakePage[] = [];
      const { h } = host(pages, 1_000);
      await h.handle("r1", prepare);
      expect(at(pages, 0).closed).toBe(false);
      await vi.advanceTimersByTimeAsync(1_500);
      expect(at(pages, 0).closed).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });

  it("doesn't close the page under a request while others overlapped it", async () => {
    vi.useFakeTimers();
    try {
      const pages: FakePage[] = [];
      const { h, sent } = host(pages, 1_000);
      await h.handle("r0", prepare);
      const page = at(pages, 0);
      // Hold the page's answers until released.
      const held: (() => void)[] = [];
      const run = page.run.bind(page);
      page.run = (code) => new Promise<void>((go) => held.push(go)).then(() => run(code));
      const r1 = h.handle("r1", prepare);
      const r2 = h.handle("r2", prepare);
      await vi.advanceTimersByTimeAsync(0);
      held.splice(0).forEach((go) => go());
      await Promise.all([r1, r2]);
      // Another request starts, and runs longer than the idle time.
      const r3 = h.handle("r3", prepare);
      await vi.advanceTimersByTimeAsync(5_000);
      expect(page.closed).toBe(false);
      held.splice(0).forEach((go) => go());
      await r3;
      expect(sent.map(errorOf)).toEqual([undefined, undefined, undefined, undefined]);
      // Idle once all of them are answered.
      await vi.advanceTimersByTimeAsync(1_500);
      expect(page.closed).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });

  it("destroys the hidden window when its page fails to load", async () => {
    failLoad = true;
    try {
      await expect(openHiddenPage()).rejects.toThrow("ERR_INTERNET_DISCONNECTED");
      expect(windows.at(-1)?.destroyed).toBe(true);
    } finally {
      failLoad = false;
    }
    const page = await openHiddenPage();
    expect(windows.at(-1)?.destroyed).toBe(false);
    page.close();
    expect(page.closed).toBe(true);
  });

  it("builds the calls and refuses a short answer", () => {
    expect(callFor(prepare)).toBe('hocketYtm.prepare("4D7u5KF7SP8", "CgtWaXNpdG9y")');
    expect(callFor(decipher)).toContain('hocketYtm.decipher("4D7u5KF7SP8", ["s=ABC');
    expect(() => answerOf(decipher, { urls: ["only one"] })).toThrow("signed no URLs");
    expect(answerOf(prepare, { signatureTimestamp: "x", poToken: "" })).toEqual({ signatureTimestamp: undefined, poToken: undefined });
  });
});
