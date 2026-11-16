import { describe, expect, it } from "vitest";
import { accountIdFor, serverKey } from "./account-id";

describe("accountIdFor", () => {
  it("matches the core's derivation", () => {
    // Same vector as hocket-core's `scope_key_normalises`.
    expect(accountIdFor("HTTPS://Music.Example/", "bob")).toBe("cb1d7475e3b86104");
    expect(accountIdFor(" https://music.example ", "bob")).toBe("cb1d7475e3b86104");
  });

  it("normalises like server_key", () => {
    expect(serverKey("HTTPS://Music.Example:4533/Navi/Drome/")).toBe("https://music.example:4533/Navi/Drome");
    expect(serverKey("Music.Example/Path")).toBe("music.example/Path");
    expect(serverKey("http://HOST?X=Y")).toBe("http://host?X=Y");
  });
});
