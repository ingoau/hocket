// The Save to Coordinator upload across the top of the window (docs/reports.md): its progress with
// Cancel, then the report's id with Copy ID (going after a few seconds), or why it failed with
// Retry and Dismiss. The live region stays mounted so each change is announced; the percentage
// is left to the progress bar, so it isn't read out on every step.
import { useEffect } from "react";
import type { ReportUploadError } from "@core/api";
import { t } from "@shared/strings";
import { bridge } from "../core/bridge";
import { showToast } from "../lib/ui-toast";
import { useApp } from "../store/app";
import { Icon } from "./Icon";

/** How long the "saved" bar stays before it dismisses itself. */
export const REPORT_DONE_MS = 8000;

const FAILED: Record<ReportUploadError, Parameters<typeof t>[0]> = {
  notAllowed: "report.failed.notAllowed",
  unreadable: "report.failed.unreadable",
  tooLarge: "report.failed.tooLarge",
  network: "report.failed.network",
  server: "report.failed.server",
};

export function ReportUploadBar() {
  const status = useApp((s) => s.reportUpload);
  const dispatch = useApp((s) => s.dispatch);
  const done = status?.state === "done";
  const startedAt = status?.startedAt;
  useEffect(() => {
    if (!done) return;
    const id = setTimeout(() => dispatch({ type: "dismissReportUpload" }), REPORT_DONE_MS);
    return () => clearTimeout(id);
  }, [done, startedAt, dispatch]);

  let body: React.ReactNode = null;
  if (status?.state === "uploading") {
    const percent = status.totalBytes > 0 ? Math.min(100, Math.floor((status.sentBytes / status.totalBytes) * 100)) : 0;
    body = (
      <>
        <Icon name="cloud" size={14} />
        <span className="sr-only">{t("report.uploadingShort")}</span>
        <span aria-hidden="true" data-testid="report-upload-text">{t("report.uploading", { percent })}</span>
        <div className="progress report-progress" role="progressbar" aria-label={t("report.progress")} aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent} data-testid="report-upload-progress"><div style={{ width: `${percent}%` }} /></div>
        <button type="button" className="link-button" onClick={() => dispatch({ type: "dismissReportUpload" })} data-testid="report-upload-cancel">{t("report.cancel")}</button>
      </>
    );
  } else if (status?.state === "done") {
    const id = status.reportId;
    body = (
      <>
        <Icon name="check" size={14} />
        <span data-testid="report-upload-text">{id ? t("report.done", { id }) : t("report.doneNoId")}</span>
        {id ? <button type="button" className="link-button" onClick={() => { bridge().clipboard.writeText(id); showToast(t("report.idCopied", { id })); }} data-testid="report-upload-copy">{t("report.copyId")}</button> : null}
      </>
    );
  } else if (status?.state === "failed") {
    body = (
      <>
        <Icon name="warn" size={14} />
        <span data-testid="report-upload-text" title={status.errorDetail}>{t(FAILED[status.error ?? "server"])}</span>
        <button type="button" className="link-button" onClick={() => dispatch({ type: "retryReportUpload" })} data-testid="report-upload-retry">{t("report.retry")}</button>
        <button type="button" className="link-button" onClick={() => dispatch({ type: "dismissReportUpload" })} data-testid="report-upload-dismiss">{t("report.dismiss")}</button>
      </>
    );
  }
  return (
    <div role="status" className="report-live" data-testid="report-upload-live">
      {body ? <div className={`report-bar ${status?.state ?? ""}`} data-state={status?.state} data-testid="report-upload-bar">{body}</div> : null}
    </div>
  );
}
