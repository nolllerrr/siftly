import { describe, expect, it } from "vitest";
import { displayPath } from "./paths";

describe("displayPath", () => {
  it("displays verbatim drive paths without changing the original", () => {
    const path = String.raw`\\?\D:\Видосы\STALCRAFT`;
    expect(displayPath(path)).toBe(String.raw`D:\Видосы\STALCRAFT`);
    expect(path).toBe(String.raw`\\?\D:\Видосы\STALCRAFT`);
    expect(displayPath("\\\\?\\C:\\")).toBe("C:\\");
  });
  it("preserves UNC network syntax", () => {
    expect(displayPath(String.raw`\\?\UNC\server\share\Фото`)).toBe(String.raw`\\server\share\Фото`);
    expect(displayPath(String.raw`\\?\unc\server\share`)).toBe(String.raw`\\server\share`);
  });
  it("leaves ordinary and special device paths unchanged", () => {
    for (const path of ["", String.raw`D:\Фото`, String.raw`\\server\share`, String.raw`\\?\Volume{abc}\folder`, String.raw`\\.\PhysicalDrive0`, "/tmp/files"]) {
      expect(displayPath(path)).toBe(path);
    }
  });
});
