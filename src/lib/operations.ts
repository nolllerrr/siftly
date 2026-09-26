import { Channel, invoke } from "@tauri-apps/api/core";
import type { BackendEvent, CopyProgress, CopyResult } from "../types";

export async function runOperation<T>(
  operation: "scan" | "copy",
  opId: string,
  payload: unknown,
  onProgress: (event: BackendEvent) => void,
): Promise<T> {
  const channel = new Channel<BackendEvent>();
  let active = true;
  channel.onmessage = (event) => { if (active) onProgress(event); };
  try {
    return await invoke<T>("run_operation", { operation, opId, payload, onProgress: channel });
  } finally {
    // Final results are authoritative; queued events from a completed operation
    // must not overwrite a new operation's state.
    active = false;
  }
}

export function copyDisplay(progress: CopyProgress | null, result: CopyResult | null) {
  const completed = result ? result.copied + result.failed : progress?.completed ?? 0;
  const total = result?.total ?? progress?.total ?? 0;
  const bytes = result?.bytes_copied ?? progress?.bytes_copied ?? 0;
  const totalBytes = progress?.total_bytes ?? bytes;
  const fraction = totalBytes ? bytes / totalBytes : total ? (result?.copied ?? completed) / total : 0;
  return { completed, total, bytes, totalBytes, percentage: Math.min(100, Math.max(0, Math.round(fraction * 100))) };
}
