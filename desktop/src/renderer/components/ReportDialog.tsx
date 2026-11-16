// Save to Coordinator (docs/reports.md): a note, up to five screenshots (picked, or pasted with
// Mod+V while the dialog is open) and one screen recording. Main stages the screenshots as they are
// added (resized, re-encoded); removing one, or closing the dialog without sending, deletes its
// copy. Send closes the dialog at once; the upload's progress is the bar across the top.
import { useEffect, useRef, useState } from "react";
import type { PickedRecording, StagedScreenshot, UndecodedImage } from "@shared/bridge-types";
import { REPORT_MAX_SCREENSHOTS } from "@shared/bridge-types";
import { t } from "@shared/strings";
import { bridge } from "../core/bridge";
import { fmtBytes } from "../lib/format";
import { showToast } from "../lib/ui-toast";
import { useApp } from "../store/app";
import { formatChord, parseChord } from "../store/shortcuts";
import { Icon } from "./Icon";

/** A WebP main can't decode: the page decodes it (Chromium can) and hands main a PNG. */
async function decodeHere(img: UndecodedImage): Promise<StagedScreenshot | undefined> {
  const bitmap = await createImageBitmap(new Blob([img.bytes as Uint8Array<ArrayBuffer>]));
  try {
    const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
    canvas.getContext("2d")?.drawImage(bitmap, 0, 0);
    const png = new Uint8Array(await (await canvas.convertToBlob({ type: "image/png" })).arrayBuffer());
    return await bridge().report.stageImage(png, img.name);
  } finally {
    bitmap.close();
  }
}

function hasImage(data: DataTransfer | null): boolean {
  return !!data && Array.from(data.items).some((i) => i.kind === "file" && i.type.startsWith("image/"));
}

export function ReportDialog({ allDevices }: { allDevices: boolean }) {
  const close = useApp((s) => s.closeDialog);
  const platform = useApp((s) => s.meta?.platform ?? "linux");
  const [note, setNote] = useState("");
  const [shots, setShots] = useState<StagedScreenshot[]>([]);
  const [recording, setRecording] = useState<PickedRecording | undefined>(undefined);
  const [adding, setAdding] = useState(false);
  // What main holds for this dialog, for the cleanup on close (state is stale by then).
  const held = useRef(new Set<string>());
  const sent = useRef(false);
  const shotsRef = useRef(shots);
  shotsRef.current = shots;

  useEffect(() => {
    const ids = held.current;
    return () => {
      if (!sent.current && ids.size) void bridge().report.discard([...ids]);
    };
  }, []);

  const addShots = (added: (StagedScreenshot | undefined)[]) => {
    const fresh = added.filter((s): s is StagedScreenshot => !!s);
    const room = REPORT_MAX_SCREENSHOTS - shotsRef.current.length;
    const keep = fresh.slice(0, Math.max(0, room));
    const extra = fresh.slice(keep.length);
    for (const s of keep) held.current.add(s.id);
    if (extra.length) {
      void bridge().report.discard(extra.map((s) => s.id));
      showToast(t("report.tooManyScreenshots", { n: REPORT_MAX_SCREENSHOTS }));
    }
    if (keep.length) setShots((prev) => [...prev, ...keep]);
  };

  const pickShots = () => {
    setAdding(true);
    void bridge()
      .report.pickScreenshots(REPORT_MAX_SCREENSHOTS - shots.length)
      .then(async (picked) => {
        const out: (StagedScreenshot | undefined)[] = [];
        for (const p of picked) {
          if ("id" in p) out.push(p);
          else out.push(await decodeHere(p).catch(() => undefined) ?? (showToast(t("report.unreadableImage", { name: p.name })), undefined));
        }
        addShots(out);
      })
      .catch((err: unknown) => console.error("report: picking screenshots failed", err))
      .finally(() => setAdding(false));
  };

  const onPaste = (e: React.ClipboardEvent) => {
    if (!hasImage(e.clipboardData)) return;
    e.preventDefault();
    if (shotsRef.current.length >= REPORT_MAX_SCREENSHOTS) {
      showToast(t("report.tooManyScreenshots", { n: REPORT_MAX_SCREENSHOTS }));
      return;
    }
    void bridge()
      .report.pasteScreenshot()
      .then((s) => addShots([s]))
      .catch((err: unknown) => console.error("report: pasting a screenshot failed", err));
  };

  const removeShot = (id: string) => {
    held.current.delete(id);
    setShots((prev) => prev.filter((s) => s.id !== id));
    void bridge().report.discard([id]);
  };

  const pickRecording = () => {
    setAdding(true);
    void bridge()
      .report.pickRecording()
      .then((r) => {
        if (!r) return;
        if (recording) {
          held.current.delete(recording.id);
          void bridge().report.discard([recording.id]);
        }
        held.current.add(r.id);
        setRecording(r);
      })
      .catch((err: unknown) => console.error("report: picking a recording failed", err))
      .finally(() => setAdding(false));
  };

  const removeRecording = () => {
    if (!recording) return;
    held.current.delete(recording.id);
    void bridge().report.discard([recording.id]);
    setRecording(undefined);
  };

  const send = (e: React.FormEvent) => {
    e.preventDefault();
    const ids = [...held.current];
    sent.current = true;
    close();
    void bridge()
      .report.send({ note, screenshots: shots.map((s) => s.id), recording: recording?.id, allDevices })
      .then((ok) => {
        if (ok) return;
        showToast(t("report.busy"));
        void bridge().report.discard(ids);
      })
      .catch((err: unknown) => {
        showToast(t("developer.failed", { error: err instanceof Error ? err.message : String(err) }));
        void bridge().report.discard(ids);
      });
  };

  const pasteKey = formatChord(parseChord("Mod+V")!, platform);
  return (
    <form className="report-dialog" onSubmit={send} onPaste={onPaste} data-testid="report-form">
      <h2 className="dialog-title" id="dialog-title">{t("report.title")}</h2>
      <p className="muted small" id="report-intro">{t("report.intro")}{allDevices ? ` ${t("report.allDevices")}` : ""}</p>
      <label>
        {t("report.note")}
        <textarea className="textarea report-note" rows={5} maxLength={8000} value={note} placeholder={t("report.notePlaceholder")} onChange={(e) => setNote(e.target.value)} aria-describedby="report-intro" data-testid="report-note" />
      </label>
      <section className="report-section" aria-labelledby="report-shots-title">
        <div className="report-section-head">
          <h3 className="report-section-title" id="report-shots-title">{t("report.screenshots")}</h3>
          <button type="button" className="btn" onClick={pickShots} disabled={adding || shots.length >= REPORT_MAX_SCREENSHOTS} aria-describedby="report-shots-hint" data-testid="report-add-screenshot"><Icon name="plus" size={16} /> {t("report.addScreenshot")}</button>
        </div>
        <p className="muted small" id="report-shots-hint">{t("report.screenshotsHint", { n: REPORT_MAX_SCREENSHOTS, key: pasteKey })}</p>
        {shots.length ? (
          <ul className="report-thumbs" data-testid="report-screenshots">
            {shots.map((s) => (
              <li key={s.id} className="report-thumb" data-testid="report-screenshot">
                <img src={s.thumbnailDataUrl} alt={s.name} />
                <button type="button" className="btn icon sm report-thumb-remove" onClick={() => removeShot(s.id)} aria-label={t("report.removeScreenshot", { name: s.name })} title={t("report.removeScreenshot", { name: s.name })} data-testid="report-remove-screenshot"><Icon name="close" size={14} /></button>
              </li>
            ))}
          </ul>
        ) : null}
      </section>
      <section className="report-section" aria-labelledby="report-recording-title">
        <div className="report-section-head">
          <h3 className="report-section-title" id="report-recording-title">{t("report.recording")}</h3>
          <button type="button" className="btn" onClick={pickRecording} disabled={adding} data-testid="report-add-recording"><Icon name="plus" size={16} /> {recording ? t("report.replaceRecording") : t("report.addRecording")}</button>
        </div>
        {recording ? (
          <div className="report-recording" data-testid="report-recording">
            <span className="report-recording-name">{recording.name}</span>
            <span className="muted small">{fmtBytes(recording.size)}</span>
            <button type="button" className="btn icon sm" onClick={removeRecording} aria-label={t("report.removeRecording")} title={t("report.removeRecording")} data-testid="report-remove-recording"><Icon name="close" size={14} /></button>
          </div>
        ) : null}
      </section>
      <div className="buttons">
        <button type="button" className="btn" onClick={close} data-testid="report-cancel">{t("dialog.cancel")}</button>
        <button type="submit" className="btn primary" data-testid="report-send"><Icon name="cloud" size={16} /> {t("report.send")}</button>
      </div>
    </form>
  );
}
