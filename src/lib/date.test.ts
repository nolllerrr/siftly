import { describe, expect, it } from "vitest";
import { formatDateValue, isDateOutsideRange, parseDateValue } from "./date";

describe("calendar date utilities", () => {
  it("formats dates as a stable local YYYY-MM-DD value", () => {
    expect(formatDateValue(new Date(2026, 8, 6))).toBe("2026-09-06");
  });

  it("parses valid dates and rejects normalized invalid dates", () => {
    expect(parseDateValue("2024-02-29")).toEqual(new Date(2024, 1, 29));
    expect(parseDateValue("2023-02-29")).toBeNull();
    expect(parseDateValue("2026-13-01")).toBeNull();
    expect(parseDateValue("26.09.2026")).toBeNull();
  });

  it("treats min and max as inclusive calendar boundaries", () => {
    expect(isDateOutsideRange("2026-09-10", "2026-09-10", "2026-09-20")).toBe(false);
    expect(isDateOutsideRange("2026-09-20", "2026-09-10", "2026-09-20")).toBe(false);
    expect(isDateOutsideRange("2026-09-09", "2026-09-10", "2026-09-20")).toBe(true);
    expect(isDateOutsideRange("2026-09-21", "2026-09-10", "2026-09-20")).toBe(true);
  });
});
