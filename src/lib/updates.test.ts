import { describe, expect, it, vi, afterEach } from "vitest";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), Channel: class { onmessage = () => {}; } }));
import { invoke } from "@tauri-apps/api/core";
import { checkForUpdates, installUpdate, updateProgressText } from "./updates";

afterEach(() => vi.clearAllMocks());
describe("updates", () => {
  it("coalesces checks and allows retries after failure", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("offline"));
    const first = checkForUpdates();
    expect(checkForUpdates()).toBe(first);
    await expect(first).rejects.toThrow("offline");
    vi.mocked(invoke).mockResolvedValueOnce({ version: null, notes: null, installSupported: true });
    await expect(checkForUpdates()).resolves.toMatchObject({ version: null });
    expect(invoke).toHaveBeenCalledTimes(2);
    expect(invoke).toHaveBeenCalledWith("check_for_updates");
  });
  it("passes only a progress channel, never a URL, signature or public key", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(false);
    expect(await installUpdate(() => {})).toBe(false);
    const args = vi.mocked(invoke).mock.calls[0];
    expect(args[0]).toBe("install_update");
    expect(Object.keys(args[1]!)).toEqual(["onProgress"]);
  });
  it("handles unknown download sizes and signature verification separately", () => {
    expect(updateProgressText({ phase: "downloading", downloaded: 1024 * 1024, total: null })).toContain("1.0 MB");
    expect(updateProgressText({ phase: "downloading", downloaded: 300, total: 100 })).toContain("100%");
    expect(updateProgressText({ phase: "verifying", downloaded: 0, total: null })).toContain("Verifying");
    expect(updateProgressText({ phase: "installing", downloaded: 0, total: null })).toContain("restarting");
  });
});
