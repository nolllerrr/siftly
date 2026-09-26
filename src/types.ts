export type FileResult = {
  path: string;
  name: string;
  extension: string;
  size: number;
  modified_time: string;
  parent_folder: string;
};

export type OperationError = { path: string; message: string };

export type ScanProgress = {
  files_checked: number;
  files_matched: number;
  current_directory: string;
};

export type ScanResult = {
  type: "scan_result";
  files: FileResult[];
  errors: OperationError[];
  files_checked: number;
  cancelled: boolean;
  truncated?: boolean;
  duration: number;
};

export type CopyProgress = {
  completed: number;
  total: number;
  bytes_copied: number;
  total_bytes: number;
  current_file: string;
};

export type CopyResult = {
  type: "copy_result";
  total: number;
  copied: number;
  failed: number;
  skipped: number;
  bytes_copied: number;
  cancelled: boolean;
  duration: number;
  errors: OperationError[];
};

export type BackendEvent =
  | ({ type: "scan_progress" } & ScanProgress)
  | ScanResult
  | ({ type: "copy_progress" } & CopyProgress)
  | CopyResult
  | { type: "fatal_error"; message: string };
