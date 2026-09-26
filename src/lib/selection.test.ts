import { describe, expect, it } from "vitest";
import type { FileResult } from "../types";
import { selectAllFiles, selectedFiles, togglePathSelection } from "./selection";

const files: FileResult[] = [
  {
    path: "C:\\source\\first.mp4",
    name: "first.mp4",
    extension: ".mp4",
    size: 10,
    modified_time: "2026-09-25T10:00:00+03:00",
    parent_folder: "C:\\source",
  },
  {
    path: "C:\\source\\second.mkv",
    name: "second.mkv",
    extension: ".mkv",
    size: 20,
    modified_time: "2026-09-26T10:00:00+03:00",
    parent_folder: "C:\\source",
  },
];

describe("result selection", () => {
  it("toggles one path without mutating the previous selection", () => {
    const original = new Set([files[0].path]);
    const removed = togglePathSelection(original, files[0].path);
    const added = togglePathSelection(removed, files[1].path);

    expect(original).toEqual(new Set([files[0].path]));
    expect(removed).toEqual(new Set());
    expect(added).toEqual(new Set([files[1].path]));
  });

  it("selects all files and clears with an empty set", () => {
    expect(selectAllFiles(files)).toEqual(new Set(files.map((file) => file.path)));
    expect(selectAllFiles([])).toEqual(new Set());
  });

  it("returns only selected file records in result order", () => {
    expect(selectedFiles(files, new Set([files[1].path]))).toEqual([files[1]]);
  });
});
