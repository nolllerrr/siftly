export const extensionPresets = [
  { id: "video", label: "Video", extensions: ".mp4, .mkv, .avi, .mov, .webm, .m4v, .wmv, .mpg, .mpeg, .mts, .m2ts" },
  { id: "photos", label: "Photos & images", extensions: ".jpg, .jpeg, .png, .webp, .gif, .bmp, .tif, .tiff, .heic, .heif, .avif, .svg" },
  { id: "raw", label: "RAW photos", extensions: ".dng, .cr2, .cr3, .nef, .arw, .raf, .orf, .rw2, .pef" },
  { id: "audio", label: "Audio", extensions: ".mp3, .wav, .flac, .aac, .m4a, .ogg, .opus, .wma, .aiff" },
  { id: "text", label: "Text files", extensions: ".txt, .md, .log, .rtf" },
  { id: "documents", label: "Documents & PDF", extensions: ".pdf, .doc, .docx, .odt, .rtf, .epub" },
  { id: "spreadsheets", label: "Spreadsheets", extensions: ".xls, .xlsx, .xlsm, .ods, .csv, .tsv" },
  { id: "presentations", label: "Presentations", extensions: ".ppt, .pptx, .pps, .ppsx, .odp" },
  { id: "archives", label: "Archives", extensions: ".zip, .7z, .rar, .tar, .gz, .bz2, .xz, .tgz" },
] as const;

export function normalizeExtensions(value: string): string[] {
  return [...new Set(value.split(/[\s,;]+/).map((item) => item.trim().toLowerCase()).filter(Boolean).map((item) => item.startsWith(".") ? item : `.${item}`))];
}

export function matchingPreset(value: string) {
  const extensions = new Set(normalizeExtensions(value));
  return extensionPresets.find((preset) => {
    const expected = normalizeExtensions(preset.extensions);
    return expected.length === extensions.size && expected.every((extension) => extensions.has(extension));
  });
}
