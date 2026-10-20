import { describe, expect, it, vi } from "vitest";

vi.mock("electron", () => ({
  app: { getName: () => "Hocket", getVersion: () => "0.1.0", isPackaged: false, getLocale: () => "en-GB", getSystemLocale: () => "en-AU" },
}));

const { DevLog, LOG_WINDOW_MS, formatLogLine } = await import("./devlog");

describe("DevLog", () => {
  it("formats lines like the core does", () => {
    expect(formatLogLine(951_782_400_123, "info", "renderer#1", "hi")).toBe("2000-02-29T00:00:00.123Z  INFO renderer#1: hi\n");
    expect(formatLogLine(0, "error", "main", "x")).toBe("1970-01-01T00:00:00.000Z ERROR main: x\n");
  });

  it("without the core keeps an hour in its own ring, and debug only in developer mode", () => {
    let now = 1_000_000;
    const log = new DevLog(undefined, () => now);
    expect(log.usesCore).toBe(false);
    log.append("info", "main", "first");
    log.append("debug", "main", "hidden");
    log.setDetailed(true);
    log.append("debug", "main", "shown");
    expect(log.recent()).toMatch(/first\n.*shown\n$/);
    expect(log.recent()).not.toContain("hidden");
    now += LOG_WINDOW_MS + 1;
    log.append("warn", "main", "later");
    expect(log.recent()).not.toContain("first");
    expect(log.recent(1)).toMatch(/WARN main: later\n$/);
  });

  it("with the core hands every line to it, untouched by its own filter, and truncates huge ones", () => {
    const lines: [string, string, string][] = [];
    const native = { appendLog: (l: string, t: string, m: string) => void lines.push([l, t, m]), recentLogs: (ms: number) => `window ${ms}` };
    const log = new DevLog(native);
    log.append("debug", "main", "d");
    log.append("info", "renderer#2", "x".repeat(20_000));
    expect(lines[0]).toEqual(["debug", "main", "d"]);
    expect(lines[1]![2]).toMatch(/^x{16384}… \(20000 chars\)$/);
    expect(log.recent(5)).toBe("window 5");
    // A shut-down addon throws; the log never does.
    const broken = new DevLog({ appendLog: () => { throw new Error("gone"); }, recentLogs: () => { throw new Error("gone"); } });
    expect(() => broken.append("info", "main", "x")).not.toThrow();
    expect(broken.recent()).toBe("");
  });

  it("captures console output once, still printing it", () => {
    const printed: unknown[][] = [];
    const fake = { log: (...a: unknown[]) => printed.push(a), info: () => undefined, warn: () => undefined, error: () => undefined, debug: () => undefined } as unknown as Console;
    const log = new DevLog(undefined, () => 5_000);
    log.captureConsole(fake);
    log.captureConsole(fake);
    fake.log("[core] %s ready", "native", { a: 1 });
    expect(printed).toHaveLength(1);
    expect(log.recent()).toBe(formatLogLine(5_000, "info", "main", "[core] native ready { a: 1 }"));
  });

  it("bundles the environment, the core's diagnostics and the log", () => {
    const log = new DevLog(undefined, () => Date.UTC(2026, 9, 20, 3, 4, 5));
    log.append("error", "renderer#1", "boom");
    const b = log.bundle({ coreDiagnostics: "Hocket core 0.1.0\n", extra: { core: "fake core" } });
    expect(b.fileName).toMatch(/^hocket-diagnostics-\d{8}-\d{6}\.txt$/);
    expect(b.head).toMatch(/^Hocket diagnostics\n/);
    expect(b.head).toContain("generated       2026-10-20T03:04:05.000Z");
    expect(b.head).toContain("core            fake core");
    expect(b.head).toContain("Hocket core 0.1.0");
    expect(b.text.startsWith(b.head)).toBe(true);
    expect(b.text).toContain("ERROR renderer#1: boom");
    expect(b.logLines).toBe(1);
  });
});
