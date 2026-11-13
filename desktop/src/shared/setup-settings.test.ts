import { describe, expect, it } from "vitest";
import { packSetupSettings, setupSettingsCount, unpackSetupSettings } from "./setup-settings";

describe("setup settings", () => {
  it("packs link-safe and unpacks", async () => {
    const json = JSON.stringify({ "queue.savedCap": 7, "nav.mobileBar": ["home", "search"], note: "ü" });
    const packed = await packSetupSettings(json);
    expect(packed).toMatch(/^[A-Za-z0-9_-]+$/);
    expect(await unpackSetupSettings(packed)).toBe(json);
  });

  it("reads what Android packs", async () => {
    // java.util.zip.Deflater(BEST_COMPRESSION) and unpadded base64url, as SetupLink.packSettings writes them.
    expect(await unpackSetupSettings("eNqrViosTS1N1StOLEtNcU4sULIy11FKySwuyEms1AMRqUVBiSWZeelKViVFpam1AKegEag"))
      .toBe('{"queue.savedCap":7,"display.playerRating":true}');
  });

  it("refuses what isn't packed settings", async () => {
    expect(await unpackSetupSettings("%%%")).toBeUndefined();
    expect(await unpackSetupSettings("AAAA")).toBeUndefined();
    expect(await unpackSetupSettings(await packSetupSettings("[1]"))).toBeUndefined();
    expect(await unpackSetupSettings(await packSetupSettings("x".repeat(70_000)))).toBeUndefined();
  });

  it("counts settings", () => {
    expect(setupSettingsCount('{"a":1,"b":2}')).toBe(2);
    expect(setupSettingsCount("{}")).toBe(0);
    expect(setupSettingsCount("[1]")).toBe(0);
    expect(setupSettingsCount(undefined)).toBe(0);
  });
});
