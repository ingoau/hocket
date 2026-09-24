import { completeSetup, test } from "./fixtures";
test("debug", async ({ hocket }) => {
  const { page } = hocket;
  await completeSetup(page);
  const r = await page.evaluate(() => {
    const p = document.querySelector(".player")!;
    const out: string[] = [];
    const walk = (el: Element, d: number) => { if (d > 4) return; const b = el.getBoundingClientRect(); out.push(`${"  ".repeat(d)}${el.tagName}.${el.className && typeof el.className === "string" ? el.className : ""} ${Math.round(b.y)} h${Math.round(b.height)} w${Math.round(b.width)}`); for (const c of Array.from(el.children)) walk(c, d + 1); };
    walk(p, 0);
    const app = document.querySelector(".app")!; out.push("app " + getComputedStyle(app).gridTemplateRows + " inner " + window.innerHeight);
    return out.join("\n");
  });
  console.log(r);
});
