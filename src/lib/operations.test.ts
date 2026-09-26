import { beforeEach, describe, expect, it, vi } from "vitest";
import type { BackendEvent, CopyProgress, CopyResult } from "../types";
import { copyDisplay, runOperation } from "./operations";

const mock = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: mock.invoke,
  Channel: class { onmessage: (event: BackendEvent) => void = () => {}; },
}));

beforeEach(() => { mock.invoke.mockReset(); });

describe("native operation channels", () => {
  it("passes each operation its own channel and ignores events after completion", async () => {
    let send: (event: BackendEvent) => void = () => {};
    const event: BackendEvent = { type: "scan_progress", files_checked: 1, files_matched: 1, current_directory: "source" };
    mock.invoke.mockImplementation(async (_, args) => {
      send = args.onProgress.onmessage;
      send(event);
      return { files: [] };
    });
    const progress = vi.fn();
    const result = await runOperation("scan", "scan-1", { source: "source" }, progress);
    expect(result).toEqual({ files: [] });
    expect(mock.invoke).toHaveBeenCalledWith("run_operation", expect.objectContaining({ operation: "scan", opId: "scan-1", onProgress: expect.anything() }));
    expect(progress).toHaveBeenCalledTimes(1);
    send(event);
    expect(progress).toHaveBeenCalledTimes(1);
  });

  it("propagates native failures so the UI can finish its busy state", async () => {
    mock.invoke.mockRejectedValue("Cannot open destination folder");
    await expect(runOperation("copy", "copy-1", {}, vi.fn())).rejects.toBe("Cannot open destination folder");
  });
});

describe("copy progress", () => {
  const progress: CopyProgress = { completed: 0, total: 1, bytes_copied: 50, total_bytes: 100, current_file: "large.mp4" };
  it("advances inside the first large file", () => {
    expect(copyDisplay(progress, null).percentage).toBe(50);
  });
  it("uses final committed bytes after cancellation, not a queued chunk", () => {
    const result: CopyResult = { type: "copy_result", copied: 0, failed: 0, skipped: 1, total: 1, bytes_copied: 0, cancelled: true, duration: 1, errors: [] };
    expect(copyDisplay(progress, result)).toMatchObject({ percentage: 0, bytes: 0, completed: 0 });
  });
  it("handles empty files and includes failed files in the processed count", () => {
    const result: CopyResult = { type: "copy_result", copied: 1, failed: 1, skipped: 0, total: 2, bytes_copied: 0, cancelled: false, duration: 1, errors: [] };
    expect(copyDisplay(null, result)).toMatchObject({ percentage: 50, completed: 2 });
  });
});
