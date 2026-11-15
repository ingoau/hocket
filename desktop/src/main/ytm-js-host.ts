// YouTube Music's JS host (docs/providers.md, Phase 4 "Premium JS path"): a
// hidden BrowserWindow on https://www.youtube.com runs the core's host
// script (yt-dlp's EJS solver plus `hocketYtm`, from
// `Query::YtmJsHostScript`) and answers the core's `ytmJsRequest` events
// with `ytmJsAnswer`: the player's signature timestamp and a PO token, or
// signed, `n`-transformed stream URLs. The web client that needs them is
// the last in the chain, and the one Premium's 256k AAC (itag 141) comes from.
//
// The window is made on the first request and closed after a while idle.
import { BrowserWindow } from "electron";
import type { Command, YtmJsAnswer, YtmJsRequest } from "@core/api";
import { CHROME_UA } from "./ytm-signin";

/** A page that runs JavaScript (the hidden window; a fake in tests). */
export interface JsPage {
  /** Evaluate `code`, awaiting a promise it yields. */
  run(code: string): Promise<unknown>;
  close(): void;
  readonly closed: boolean;
}

/** The expression that answers `request` once the host script is in the page. */
export function callFor(request: YtmJsRequest): string {
  switch (request.type) {
    case "prepare":
      return `hocketYtm.prepare(${JSON.stringify(request.data.video_id)}, ${JSON.stringify(request.data.visitor_data)})`;
    case "decipher":
      return `hocketYtm.decipher(${JSON.stringify(request.data.video_id)}, ${JSON.stringify(request.data.formats)})`;
  }
}

/** The page's result as the core's answer (unknown fields dropped, wrong types refused). */
export function answerOf(request: YtmJsRequest, value: unknown): YtmJsAnswer {
  const v = (value ?? {}) as { signatureTimestamp?: unknown; poToken?: unknown; urls?: unknown };
  if (request.type === "decipher") {
    const urls = v.urls;
    if (!Array.isArray(urls) || urls.length !== request.data.formats.length || !urls.every((u) => typeof u === "string")) {
      throw new Error("the JS host signed no URLs");
    }
    return { urls };
  }
  return {
    signatureTimestamp: typeof v.signatureTimestamp === "number" ? v.signatureTimestamp : undefined,
    poToken: typeof v.poToken === "string" && v.poToken ? v.poToken : undefined,
  };
}

export const IDLE_CLOSE_MS = 10 * 60_000;

export class YtmJsHost {
  private page: Promise<JsPage> | undefined;
  private idle: ReturnType<typeof setTimeout> | undefined;

  constructor(
    private readonly open: () => Promise<JsPage>,
    private readonly script: () => Promise<string>,
    private readonly dispatch: (command: Command) => void,
    private readonly idleMs = IDLE_CLOSE_MS,
  ) {}

  /** Answer one request; never throws (a failure is the answer's `error`). */
  async handle(requestId: string, request: YtmJsRequest): Promise<void> {
    if (this.idle) clearTimeout(this.idle);
    try {
      const page = await this.ready();
      const answer = answerOf(request, await page.run(callFor(request)));
      if (request.type === "prepare") {
        console.info(`[ytm-js] prepared: signatureTimestamp ${answer.signatureTimestamp ?? "none"}, PO token ${answer.poToken ? "minted" : "none"}`);
      }
      this.dispatch({ type: "ytmJsAnswer", data: { request_id: requestId, answer, error: undefined } });
    } catch (e) {
      // A page that died is made again for the next request.
      const page = await this.page?.catch(() => undefined);
      if (!page || page.closed) this.page = undefined;
      const error = e instanceof Error ? e.message : String(e);
      console.warn(`[ytm-js] ${request.type} failed: ${error}`);
      this.dispatch({ type: "ytmJsAnswer", data: { request_id: requestId, answer: undefined, error } });
    } finally {
      this.idle = setTimeout(() => this.close(), this.idleMs);
    }
  }

  close(): void {
    if (this.idle) clearTimeout(this.idle);
    this.idle = undefined;
    const page = this.page;
    this.page = undefined;
    void page?.then((p) => p.close()).catch(() => undefined);
  }

  private ready(): Promise<JsPage> {
    if (!this.page) {
      this.page = (async () => {
        const [page, script] = await Promise.all([this.open(), this.script()]);
        try {
          await page.run(script);
        } catch (e) {
          page.close();
          throw e;
        }
        return page;
      })();
      this.page.catch(() => {
        this.page = undefined;
      });
    }
    return this.page;
  }
}

/** Where the hidden page lives: youtube.com's origin, so its fetches are same-origin. */
export const HOST_URL = "https://www.youtube.com/robots.txt";
const PARTITION = "hocket-ytm-js";

/** A hidden window on www.youtube.com (a session of its own, never signed in). */
export async function openHiddenPage(): Promise<JsPage> {
  const win = new BrowserWindow({
    show: false,
    width: 400,
    height: 300,
    webPreferences: { partition: PARTITION, sandbox: true, contextIsolation: true, nodeIntegration: false, backgroundThrottling: false },
  });
  win.webContents.setUserAgent(CHROME_UA);
  win.webContents.setAudioMuted(true);
  await win.loadURL(HOST_URL);
  return {
    run: (code) => win.webContents.executeJavaScript(code, true),
    close: () => {
      if (!win.isDestroyed()) win.destroy();
    },
    get closed() {
      return win.isDestroyed() || win.webContents.isCrashed();
    },
  };
}
