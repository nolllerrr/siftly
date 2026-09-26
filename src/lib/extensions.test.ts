import { describe, expect, it } from "vitest";
import { extensionPresets, matchingPreset, normalizeExtensions } from "./extensions";

describe("extension presets", () => {
  it("normalizes custom input and removes duplicates", () => {
    expect(normalizeExtensions(" MP4; .mp4, TXT\nJPG ")).toEqual([".mp4", ".txt", ".jpg"]);
    expect(normalizeExtensions(" , ; ")).toEqual([]);
  });
  it("provides unique, nonempty scan-ready presets", () => {
    expect(new Set(extensionPresets.map((preset) => preset.id)).size).toBe(extensionPresets.length);
    for (const preset of extensionPresets) {
      const extensions = normalizeExtensions(preset.extensions);
      expect(extensions.length).toBeGreaterThan(0);
      expect(extensions.join(", ")).toBe(preset.extensions);
      expect(extensions.every((extension) => /^\.[a-z0-9]+$/.test(extension))).toBe(true);
      expect(matchingPreset(preset.extensions)?.id).toBe(preset.id);
    }
  });
  it("recognizes a preset regardless of order or case, but not a custom subset", () => {
    expect(matchingPreset("RTF; LOG; MD; TXT; txt")?.id).toBe("text");
    expect(matchingPreset(".txt")).toBeUndefined();
    expect(matchingPreset(".txt, .md, .log, .rtf, .json")).toBeUndefined();
    expect(matchingPreset("")).toBeUndefined();
  });
});
