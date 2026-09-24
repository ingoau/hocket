// Shared helpers for the accessibility specs: axe scans, theme switching,
// keyboard-only setup, the syllable-lyrics showcase track, focus inspection.
import AxeBuilder from "@axe-core/playwright";
import type { ElectronApplication, Page } from "@playwright/test";
import { expect } from "./fixtures";

/** Rules switched off everywhere, with the reason. Empty: every axe rule runs. */
export const DISABLED_RULES: Record<string, string> = {};

/**
 * Per-element exemptions: a violation of `rule` is ignored only for nodes
 * inside `within`; the rule still runs on everything else.
 */
export const EXEMPT: { rule: string; within: string; why: string }[] = [
  {
    rule: "region",
    within: '[data-testid="context-menu"]',
    why: "The context menu is a transient popup rendered at the end of the document, like a native menu; it belongs to the item that opened it, not to a page region, and wrapping it in a landmark would add a bogus region to the landmarks list.",
  },
];

/** Elements excluded from every scan, with the reason. Empty: nothing is excluded. */
export const EXCLUDED: { selector: string; why: string }[] = [];

export interface ScanResult {
  id: string;
  impact: string | null | undefined;
  nodes: string[];
}

/** axe over the whole window (legacy mode: Electron can't open the extra page axe's iframe mode wants). */
export async function axe(page: Page): Promise<ScanResult[]> {
  let b = new AxeBuilder({ page }).setLegacyMode(true);
  if (Object.keys(DISABLED_RULES).length) b = b.disableRules(Object.keys(DISABLED_RULES));
  for (const e of EXCLUDED) b = b.exclude(e.selector);
  const r = await b.analyze();
  const out: ScanResult[] = [];
  for (const v of r.violations) {
    const nodes: string[] = [];
    for (const n of v.nodes) {
      const target = n.target.join(" ");
      const exempt = await page.evaluate(({ target, sels }) => {
        const el = document.querySelector(target);
        return !!el && sels.some((s) => !!el.closest(s));
      }, { target, sels: EXEMPT.filter((e) => e.rule === v.id).map((e) => e.within) });
      if (!exempt) nodes.push(`${target} :: ${(n.failureSummary ?? "").replace(/\s+/g, " ").slice(0, 200)}`);
    }
    if (nodes.length) out.push({ id: v.id, impact: v.impact, nodes });
  }
  return out;
}

/** Scan and require zero violations of any impact (serious and critical included, and moderate/minor too). */
export async function expectNoViolations(page: Page, label: string): Promise<void> {
  const v = await axe(page);
  const summary = v.map((x) => `${x.impact} ${x.id}\n    ${x.nodes.slice(0, 4).join("\n    ")}`).join("\n");
  expect(v, `${label}\n${summary}`).toEqual([]);
}

export async function setTheme(page: Page, theme: "light" | "dark"): Promise<void> {
  await page.evaluate((th) => window.hocket.dispatch({ type: "setSetting", data: { key: "display.theme", value: JSON.stringify(th) } }), theme);
  await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
}

export async function serverId(page: Page): Promise<string> {
  const servers = await page.evaluate(async () => (await window.hocket.query({ type: "servers" })) as { type: string; data: { id: string }[] });
  return servers.data[0]!.id;
}

/** The FakeCore's "Word by Word": syllable-tier lyrics with background vocals. */
export async function playShowcase(page: Page): Promise<void> {
  const sid = await serverId(page);
  await page.evaluate((s) => window.hocket.dispatch({ type: "playTracks", data: { server_id: s, track_ids: ["tr-1"], start_index: 0, label: "e2e", shuffle: false } }), sid);
  await expect(page.getByTestId("queue-row-current")).toContainText("Word by Word");
}

/** Setup with the keyboard only: the URL field has focus, Tab through the form, Enter submits. */
export async function keyboardSetup(page: Page): Promise<void> {
  await expect(page.getByTestId("setup")).toBeVisible();
  await expect(page.getByTestId("setup-url")).toBeFocused();
  await page.keyboard.type("https://music.example.org");
  await page.keyboard.press("Tab");
  await expect(page.getByTestId("setup-username")).toBeFocused();
  await page.keyboard.type("alice");
  await page.keyboard.press("Tab");
  await expect(page.getByTestId("setup-password")).toBeFocused();
  await page.keyboard.type("secret");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("app")).toBeVisible({ timeout: 15_000 });
}

export interface FocusInfo {
  testId: string | null;
  role: string | null;
  tag: string;
  name: string;
  outlineStyle: string;
  outlineWidth: number;
  outlineColor: string;
  /** Landmark the element sits in: banner, navigation, main, complementary, player, toasts, dialog, other. */
  region: string;
  visible: boolean;
}

export async function focused(page: Page): Promise<FocusInfo | undefined> {
  return page.evaluate(() => {
    const el = document.activeElement as HTMLElement | null;
    if (!el || el === document.body) return undefined;
    const cs = getComputedStyle(el);
    const region = el.closest("header") ? "banner" : el.closest("nav.sidebar") ? "navigation" : el.closest("main") ? "main" : el.closest("aside") ? "complementary" : el.closest('[data-testid="player-bar"]') ? "player" : el.closest('[data-testid="toasts"]') ? "toasts" : el.closest("[role=dialog],[role=alertdialog]") ? "dialog" : "other";
    const r = el.getBoundingClientRect();
    return {
      testId: el.getAttribute("data-testid"),
      role: el.getAttribute("role"),
      tag: el.tagName.toLowerCase(),
      name: (el.getAttribute("aria-label") ?? el.textContent ?? "").trim().slice(0, 60),
      outlineStyle: cs.outlineStyle,
      outlineWidth: Number.parseFloat(cs.outlineWidth),
      outlineColor: cs.outlineColor,
      region,
      visible: r.width > 0 && r.height > 0 && r.bottom > 0 && r.right > 0 && r.top < innerHeight && r.left < innerWidth,
    };
  });
}

/** A focus indicator is drawn: a solid 2px+ outline in a non-transparent colour. */
export function hasRing(f: FocusInfo | undefined): boolean {
  return !!f && f.outlineStyle !== "none" && f.outlineWidth >= 2 && !/rgba\(.*,\s*0\)$/.test(f.outlineColor) && f.outlineColor !== "transparent";
}

/** Browser zoom, like Ctrl+= in a browser (200% = 2). */
export async function setZoom(app: ElectronApplication, factor: number): Promise<void> {
  await app.evaluate(({ BrowserWindow }, f) => {
    for (const w of BrowserWindow.getAllWindows()) if (w.isVisible()) w.webContents.setZoomFactor(f);
  }, factor);
}
