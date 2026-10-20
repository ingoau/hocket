// Log capture for developer mode and the developer diagnostics page. With the native core every
// line goes into the core's own buffer (hocket_core::devlog): one timeline with the core's
// tracing output, an hour long, mirrored to disk so a crash doesn't lose it. With the fake core
// a small ring here stands in. Main's console and every renderer's console and crashes are
// captured; `bundle()` assembles the report the page saves or shares.
import { app, type WebContents } from "electron";
import os from "node:os";
import { format } from "node:util";

export const LOG_WINDOW_MS = 60 * 60 * 1000;
/** Longest single line kept (a renderer can log a whole document). */
const MAX_LINE = 16 * 1024;
const FALLBACK_MAX_BYTES = 8 * 1024 * 1024;

export type LogLevel = "trace" | "debug" | "info" | "warn" | "error";

/** The addon's log functions (addons built before they existed lack them). */
export interface LogNative {
  appendLog?(level: string, tag: string, message: string): void;
  recentLogs?(windowMs: number): string;
  flushLogs?(timeoutMs: number): void;
}

/** What the page saves and shares: `head` (environment and core) then the log. */
export interface DiagnosticsBundle {
  fileName: string;
  text: string;
  head: string;
  log: string;
  /** Log lines in the bundle and the window they cover. */
  logLines: number;
  logBytes: number;
  windowMs: number;
  generatedAt: number;
}

/** `2026-10-20T03:04:05.678Z  INFO tag: message`, the core's line shape. */
export function formatLogLine(at: number, level: LogLevel, tag: string, message: string): string {
  return `${new Date(at).toISOString()} ${level.toUpperCase().padStart(5)} ${tag}: ${message}\n`;
}

export class DevLog {
  private ring: { at: number; line: string }[] = [];
  private ringBytes = 0;
  private detailed = false;
  private consoleCaptured = false;

  constructor(
    private readonly native?: LogNative,
    private readonly now: () => number = Date.now,
  ) {}

  /** Lines go to the core's buffer rather than the ring here. */
  get usesCore(): boolean {
    return typeof this.native?.appendLog === "function" && typeof this.native.recentLogs === "function";
  }

  /** developer.mode, as main mirrors it: the fallback ring keeps debug lines only while it is on (the core filters its own). */
  setDetailed(on: boolean): void {
    this.detailed = on;
  }

  get isDetailed(): boolean {
    return this.detailed;
  }

  append(level: LogLevel, tag: string, message: string): void {
    const text = message.length > MAX_LINE ? `${message.slice(0, MAX_LINE)}… (${message.length} chars)` : message;
    if (this.usesCore) {
      try {
        this.native!.appendLog!(level, tag, text);
      } catch {
        /* a shut-down addon: nothing to log to */
      }
      return;
    }
    if ((level === "debug" || level === "trace") && !this.detailed) return;
    const at = this.now();
    const line = formatLogLine(at, level, tag, text);
    this.ring.push({ at, line });
    this.ringBytes += line.length;
    const since = at - LOG_WINDOW_MS;
    for (let first = this.ring[0]; first && (first.at < since || this.ringBytes > FALLBACK_MAX_BYTES); first = this.ring[0]) {
      this.ringBytes -= first.line.length;
      this.ring.shift();
    }
  }

  /** The captured lines of the last `windowMs`, oldest first. */
  recent(windowMs = LOG_WINDOW_MS): string {
    if (this.usesCore) {
      try {
        return this.native!.recentLogs!(windowMs);
      } catch {
        return "";
      }
    }
    const since = this.now() - windowMs;
    return this.ring.filter((l) => l.at >= since).map((l) => l.line).join("");
  }

  flush(timeoutMs = 500): void {
    try {
      this.native?.flushLogs?.(timeoutMs);
    } catch {
      /* ignore */
    }
  }

  /** Main's own console: still printed, and captured as `main`. Once per process. */
  captureConsole(target: Console = console): void {
    if (this.consoleCaptured) return;
    this.consoleCaptured = true;
    const levels: [keyof Console & ("log" | "info" | "warn" | "error" | "debug"), LogLevel][] = [
      ["log", "info"],
      ["info", "info"],
      ["warn", "warn"],
      ["error", "error"],
      ["debug", "debug"],
    ];
    for (const [method, level] of levels) {
      const original = target[method].bind(target) as (...args: unknown[]) => void;
      target[method] = ((...args: unknown[]) => {
        original(...args);
        this.append(level, "main", format(...args));
      }) as never;
    }
  }

  /** A window's console messages, crashes and load failures, captured as `renderer`. */
  watch(contents: WebContents): void {
    const tag = () => `renderer#${contents.id}`;
    contents.on("console-message", (e) => {
      const level: LogLevel = e.level === "warning" ? "warn" : e.level === "error" ? "error" : e.level === "debug" ? "debug" : "info";
      const source = e.sourceId ? ` (${shortSource(e.sourceId)}:${e.lineNumber})` : "";
      this.append(level, tag(), `${e.message}${source}`);
    });
    contents.on("render-process-gone", (_e, details) => this.append("error", tag(), `render process gone: ${details.reason} (exit code ${details.exitCode})`));
    contents.on("unresponsive", () => this.append("warn", tag(), "page unresponsive"));
    contents.on("responsive", () => this.append("info", tag(), "page responsive again"));
    contents.on("preload-error", (_e, path, error) => this.append("error", tag(), `preload ${shortSource(path)} failed: ${error.stack ?? String(error)}`));
    contents.on("did-fail-load", (_e, code, description, url, isMainFrame) => {
      if (isMainFrame) this.append("error", tag(), `load failed ${code} ${description}: ${shortSource(url)}`);
    });
  }

  /** The report: environment, the core's diagnostics, then the log of the last `windowMs`. */
  bundle(parts: { coreDiagnostics: string; extra?: Record<string, string>; windowMs?: number }): DiagnosticsBundle {
    const windowMs = parts.windowMs ?? LOG_WINDOW_MS;
    const generatedAt = this.now();
    const log = this.recent(windowMs);
    const logLines = log ? log.split("\n").filter((l) => l.length > 0).length : 0;
    const env: [string, string][] = [
      ["generated", new Date(generatedAt).toISOString()],
      ["app", `${app.getName()} ${app.getVersion()}${app.isPackaged ? "" : " (development build)"}`],
      ["electron", `${process.versions.electron} (chrome ${process.versions.chrome}, node ${process.versions.node}, v8 ${process.versions.v8})`],
      ["os", `${os.type()} ${os.release()} ${os.arch()} (${process.platform})`],
      ["cpus", `${os.cpus().length} × ${os.cpus()[0]?.model ?? "?"}`],
      ["memory", `${mib(os.freemem())} free of ${mib(os.totalmem())}; main rss ${mib(process.memoryUsage().rss)}`],
      ["uptime", `process ${Math.round(process.uptime())} s, system ${Math.round(os.uptime())} s`],
      ["locale", `${app.getLocale()} (system ${app.getSystemLocale()})`],
      ["developer mode", this.detailed ? "on (detailed logging)" : "off"],
      ["log", `${logLines} lines, ${mib(Buffer.byteLength(log))}, last ${Math.round(windowMs / 60_000)} min (${this.usesCore ? "core buffer" : "main-process buffer"})`],
      ...Object.entries(parts.extra ?? {}),
    ];
    const width = Math.max(...env.map(([k]) => k.length));
    const head = [
      "Hocket diagnostics",
      "==================",
      ...env.map(([k, v]) => `${k.padEnd(width)}  ${v}`),
      "",
      "Core",
      "----",
      parts.coreDiagnostics.trimEnd(),
      "",
    ].join("\n");
    const text = [
      head,
      `Log (last ${Math.round(windowMs / 60_000)} minutes)`,
      "-------------------",
      log.trimEnd() || "(empty)",
      "",
    ].join("\n");
    return { fileName: `hocket-diagnostics-${fileStamp(generatedAt)}.txt`, text, head, log, logLines, logBytes: Buffer.byteLength(log), windowMs, generatedAt };
  }
}

function mib(bytes: number): string {
  return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
}

/** `20261020-030405`, local time, for file names. */
function fileStamp(at: number): string {
  const d = new Date(at);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}`;
}

/** The tail of a source URL or path: enough to find it, without the install location. */
function shortSource(source: string): string {
  const clean = source.split(/[?#]/)[0] ?? source;
  const parts = clean.split(/[/\\]/).filter(Boolean);
  return parts.slice(-2).join("/") || clean;
}
