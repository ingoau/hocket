import { describe, expect, it } from "vitest";
import type { NetworkState } from "@core/api";
import { networkReporter } from "./network";

describe("network reporter", () => {
  it("passes on changes only", () => {
    const pushed: NetworkState[] = [];
    const report = networkReporter((s) => pushed.push(s));
    report(true);
    report(true);
    report(true);
    report(false);
    report(false);
    report(true);
    expect(pushed.map((s) => s.kind)).toEqual(["unknown", "offline", "unknown"]);
  });

  it("never claims a metered connection or a network id", () => {
    const pushed: NetworkState[] = [];
    networkReporter((s) => pushed.push(s))(false);
    expect(pushed).toEqual([{ kind: "offline", metered: false, networkId: undefined }]);
  });
});
