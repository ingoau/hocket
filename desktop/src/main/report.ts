// Save to Coordinator (docs/reports.md): main stages a report's attachments and starts the core's
// upload; the core does the HTTP. Screenshots are decoded here, resized to at most 2000 px a side
// and re-encoded as JPEG (which drops their metadata) into `<userData>/report-staging/`; the core
// deletes those copies once the upload is done or dismissed. Recordings go as they are, from where
// the user keeps them. The page names attachments by id only, never by path.
import { randomUUID } from "node:crypto";
import { mkdir, readFile, rm, stat, writeFile } from "node:fs/promises";
import { basename, extname, join } from "node:path";
import { BrowserWindow, clipboard, ipcMain, nativeImage, type NativeImage } from "electron";
import type { ReportAttachment } from "@core/api";
import type { CoreHandle } from "@shared/core-handle";
import type { PickedRecording, StagedScreenshot, UndecodedImage } from "@shared/bridge-types";
import { IPC, REPORT_MAX_SCREENSHOTS } from "@shared/bridge-types";
import { t } from "@shared/strings";
import { showOpenDialog } from "./file-dialog";

/** Longest side of a staged screenshot. */
export const SCREENSHOT_MAX_SIDE = 2000;
const JPEG_QUALITY = 85;
const THUMB_SIDE = 160;
/** The coordinator takes screenshots up to 25 MiB; an original larger than this isn't read. */
const MAX_IMAGE_BYTES = 25 * 1024 * 1024;
/** The coordinator caps a report's note at 8000 characters. */
const MAX_NOTE = 8000;
const SCREENSHOT_EXTENSIONS = ["png", "jpg", "jpeg", "webp"];
const RECORDING_EXTENSIONS = ["mp4", "webm", "mov", "mkv"];
/** How long staging waits for the core to start before it gives up on the startup cleanup. */
const CLEANUP_WAIT_MS = 15_000;

export interface ReportIpcDeps {
  core: CoreHandle;
  /** Where screenshots are staged (`<userData>/report-staging`). */
  stagingDir: string;
  isOurs(sender: Electron.WebContents): boolean;
  /** This device's diagnostics bundle (with the other devices' when asked), as Save to file writes it. */
  build(allDevices: boolean): Promise<{ text: string }>;
}

interface Staged {
  kind: ReportAttachment["kind"];
  path: string;
}

function isId(v: unknown): v is string {
  return typeof v === "string" && /^[0-9a-f-]{36}$/.test(v);
}

/** `img` fitted within `side` × `side` (never enlarged). */
function fit(img: NativeImage, side: number): NativeImage {
  const { width, height } = img.getSize();
  if (width <= side && height <= side) return img;
  return width >= height ? img.resize({ width: side, quality: "best" }) : img.resize({ height: side, quality: "best" });
}

export function installReportIpc(deps: ReportIpcDeps): void {
  const { core, stagingDir, isOurs } = deps;
  const staged = new Map<string, Staged>();

  /**
   * Leftovers from a dialog that never closed cleanly (a crash, a reload) are removed once the core
   * has started (and so loaded any upload it resumes), unless it still has an upload that may need
   * them (running, or failed and waiting for Retry). Staging waits for this so it never removes a
   * fresh file; a core that hasn't started within CLEANUP_WAIT_MS is left alone.
   */
  const cleanup = async (): Promise<void> => {
    try {
      const r = await core.query({ type: "reportUpload" });
      if (r.type !== "reportUpload" || r.data) return;
      await rm(stagingDir, { recursive: true, force: true });
    } catch (err) {
      console.warn("[report] staging cleanup skipped", err);
    }
  };
  const cleaned = new Promise<void>((resolve) => {
    const timer = setTimeout(() => {
      off();
      resolve();
    }, CLEANUP_WAIT_MS);
    timer.unref?.();
    const off = core.onEvent((event) => {
      if (event.type !== "started") return;
      off();
      clearTimeout(timer);
      void cleanup().finally(resolve);
    });
  });

  const stage = async (img: NativeImage, name: string): Promise<StagedScreenshot> => {
    await cleaned;
    const resized = fit(img, SCREENSHOT_MAX_SIDE);
    const id = randomUUID();
    const path = join(stagingDir, `${id}.jpg`);
    await mkdir(stagingDir, { recursive: true });
    await writeFile(path, resized.toJPEG(JPEG_QUALITY));
    staged.set(id, { kind: "screenshot", path });
    const thumbnailDataUrl = `data:image/jpeg;base64,${fit(resized, THUMB_SIDE).toJPEG(80).toString("base64")}`;
    return { id, name: name.slice(0, 256), thumbnailDataUrl };
  };

  const forget = async (id: string): Promise<void> => {
    const s = staged.get(id);
    if (!s) return;
    staged.delete(id);
    // Recordings are the user's own files: only the screenshots are copies of ours.
    if (s.kind === "screenshot") await rm(s.path, { force: true });
  };

  ipcMain.handle(IPC.reportPickScreenshots, async (e, room: unknown): Promise<(StagedScreenshot | UndecodedImage)[]> => {
    const win = BrowserWindow.fromWebContents(e.sender);
    if (!win || !isOurs(e.sender)) throw new Error("rejected");
    const max = typeof room === "number" && Number.isInteger(room) ? Math.max(0, Math.min(REPORT_MAX_SCREENSHOTS, room)) : 0;
    if (max === 0) return [];
    const r = await showOpenDialog(win, {
      title: t("report.pickScreenshots"),
      filters: [{ name: t("report.imageFilter"), extensions: SCREENSHOT_EXTENSIONS }],
      properties: ["openFile", "multiSelections"],
    });
    if (r.canceled) return [];
    const out: (StagedScreenshot | UndecodedImage)[] = [];
    for (const path of r.filePaths.slice(0, max)) {
      try {
        if ((await stat(path)).size > MAX_IMAGE_BYTES) {
          console.warn(`[report] ${basename(path)} is too large to attach`);
          continue;
        }
        const bytes = await readFile(path);
        const img = nativeImage.createFromBuffer(bytes);
        if (!img.isEmpty()) out.push(await stage(img, basename(path)));
        // nativeImage decodes PNG and JPEG only: the page decodes WebP and hands it back.
        else if (extname(path).toLowerCase() === ".webp") out.push({ name: basename(path), bytes: new Uint8Array(bytes) });
        else console.warn(`[report] ${basename(path)} isn't an image this app can read`);
      } catch (err) {
        console.error("[report] couldn't stage a screenshot", err);
      }
    }
    return out;
  });

  /** The first PNG or JPEG on the clipboard, if any. */
  async function readClipboardImage(): Promise<NativeImage | undefined> {
    for (const item of await clipboard.read()) {
      const mime = item.types.find((type) => type === "image/png" || type === "image/jpeg");
      if (mime) return nativeImage.createFromBuffer(Buffer.from(await (await item.getType(mime)).arrayBuffer()));
    }
    return undefined;
  }

  ipcMain.handle(IPC.reportPasteScreenshot, async (e): Promise<StagedScreenshot | undefined> => {
    if (!isOurs(e.sender)) throw new Error("rejected");
    const img = await readClipboardImage();
    return img === undefined || img.isEmpty() ? undefined : stage(img, t("report.pastedName"));
  });

  ipcMain.handle(IPC.reportStageImage, async (e, bytes: unknown, name: unknown): Promise<StagedScreenshot | undefined> => {
    if (!isOurs(e.sender) || !(bytes instanceof Uint8Array) || bytes.length === 0 || bytes.length > MAX_IMAGE_BYTES || typeof name !== "string") throw new Error("rejected");
    const img = nativeImage.createFromBuffer(Buffer.from(bytes.buffer, bytes.byteOffset, bytes.byteLength));
    return img.isEmpty() ? undefined : stage(img, name);
  });

  ipcMain.handle(IPC.reportPickRecording, async (e): Promise<PickedRecording | undefined> => {
    const win = BrowserWindow.fromWebContents(e.sender);
    if (!win || !isOurs(e.sender)) throw new Error("rejected");
    const r = await showOpenDialog(win, {
      title: t("report.pickRecording"),
      filters: [{ name: t("report.videoFilter"), extensions: RECORDING_EXTENSIONS }],
      properties: ["openFile"],
    });
    const path = r.filePaths[0];
    if (r.canceled || !path) return undefined;
    const info = await stat(path);
    if (!info.isFile()) return undefined;
    const id = randomUUID();
    staged.set(id, { kind: "recording", path });
    return { id, name: basename(path), size: info.size };
  });

  ipcMain.handle(IPC.reportDiscard, async (e, ids: unknown): Promise<void> => {
    if (!isOurs(e.sender) || !Array.isArray(ids)) throw new Error("rejected");
    await Promise.all(ids.filter(isId).map(forget));
  });

  ipcMain.handle(IPC.reportSend, async (e, raw: unknown): Promise<boolean> => {
    if (!isOurs(e.sender) || typeof raw !== "object" || raw === null) throw new Error("rejected");
    const req = raw as Record<string, unknown>;
    if (typeof req.note !== "string" || !Array.isArray(req.screenshots) || (req.recording !== undefined && !isId(req.recording))) throw new Error("rejected");
    // The core takes one upload at a time and ignores a second: keep the files for the page to discard.
    try {
      const r = await core.query({ type: "reportUpload" });
      if (r.type === "reportUpload" && r.data?.state === "uploading") return false;
    } catch (err) {
      console.warn("[report] couldn't ask the core about the upload", err);
    }
    const attachments: ReportAttachment[] = [];
    const ids = req.screenshots.filter(isId);
    for (const id of ids.slice(0, REPORT_MAX_SCREENSHOTS)) {
      const s = staged.get(id);
      if (s?.kind !== "screenshot") continue;
      attachments.push({ path: s.path, kind: "screenshot", deleteAfter: true });
      staged.delete(id);
    }
    await Promise.all(ids.slice(REPORT_MAX_SCREENSHOTS).map(forget));
    const rec = req.recording !== undefined ? staged.get(req.recording) : undefined;
    if (rec?.kind === "recording") {
      attachments.push({ path: rec.path, kind: "recording", deleteAfter: false });
      staged.delete(req.recording as string);
    }
    const bundle = await deps.build(req.allDevices === true);
    try {
      core.dispatch({ type: "startReportUpload", data: { note: req.note.trim().slice(0, MAX_NOTE), bundle: bundle.text, attachments } });
      return true;
    } catch (err) {
      console.error("[report] the core refused the report", err);
      await Promise.all(attachments.filter((a) => a.deleteAfter).map((a) => rm(a.path, { force: true })));
      return false;
    }
  });
}
