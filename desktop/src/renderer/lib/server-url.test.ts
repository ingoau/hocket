import { describe, expect, it } from "vitest";
import { normaliseServerUrl } from "./server-url";

describe("normaliseServerUrl", () => {
  it("accepts the scheme in any case and lower-cases it", () => {
    expect(normaliseServerUrl("Https://music.example.net")).toBe("https://music.example.net");
    expect(normaliseServerUrl("HTTPS://music.example.net")).toBe("https://music.example.net");
    expect(normaliseServerUrl("hTTp://10.0.0.2:4533")).toBe("http://10.0.0.2:4533");
  });

  it("drops whitespace and trailing slashes", () => {
    expect(normaliseServerUrl("  https://music.example.net/navidrome// ")).toBe("https://music.example.net/navidrome");
  });

  it("rejects an address without a scheme or host", () => {
    for (const bad of ["music.example.net", "ftp://music.example.net", "https://", ""]) expect(normaliseServerUrl(bad)).toBeUndefined();
  });
});
