import { describe, expect, it } from "vitest";
import { formatDuration, formatSats } from "@/lib/clock";

describe("formatting", () => {
  it("formats countdowns", () => {
    expect(formatDuration(5)).toBe("5s");
    expect(formatDuration(65)).toBe("1m 05s");
    expect(formatDuration(3725)).toBe("1h 02m 05s");
    expect(formatDuration(-3)).toBe("0s");
  });

  it("formats sats", () => {
    expect(formatSats(1000000)).toBe("1,000,000 sats");
  });
});
