import { Channel, invoke } from "@tauri-apps/api/core";

export interface UpdateInfo { version: string | null; notes: string | null; installSupported: boolean }
export interface UpdateProgress { phase: "downloading" | "verifying" | "installing"; downloaded: number; total: number | null }

// Share the in-flight request across StrictMode mounts and rapid manual clicks.
let checking: Promise<UpdateInfo> | null = null;
export function checkForUpdates(): Promise<UpdateInfo> {
  checking ??= invoke<UpdateInfo>("check_for_updates").finally(() => { checking = null; });
  return checking;
}

export function installUpdate(onProgress: (event: UpdateProgress) => void): Promise<boolean> {
  const channel = new Channel<UpdateProgress>();
  channel.onmessage = onProgress;
  return invoke<boolean>("install_update", { onProgress: channel });
}

export function updateProgressText(progress: UpdateProgress): string {
  if (progress.phase === "verifying") return "Verifying signature…";
  if (progress.phase === "installing") return "Installing — restarting…";
  if (progress.total && progress.total > 0) return `Downloading ${Math.min(100, Math.max(0, Math.round(progress.downloaded / progress.total * 100)))}%`;
  return `Downloading ${(progress.downloaded / 1024 / 1024).toFixed(1)} MB…`;
}
