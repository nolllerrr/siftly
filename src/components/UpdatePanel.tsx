import { isTauri } from "@tauri-apps/api/core";
import { Download, RefreshCw } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { checkForUpdates, installUpdate, updateProgressText, type UpdateInfo } from "../lib/updates";

export function UpdatePanel({ busy, onInstallingChange }: { busy: boolean; onInstallingChange: (value: boolean) => void }) {
  const [info, setInfo] = useState<UpdateInfo | null>(null);
  const [checking, setChecking] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [status, setStatus] = useState("");
  const [error, setError] = useState("");
  const mounted = useRef(false);
  const active = useRef(false);

  async function check() {
    if (!isTauri() || active.current) return;
    active.current = true;
    setChecking(true); setInfo(null); setError(""); setStatus("Checking for updates…");
    try {
      const result = await checkForUpdates();
      if (mounted.current) {
        setInfo(result);
        setStatus(result.version ? `Version ${result.version} available` : "You're up to date");
      }
    } catch {
      if (mounted.current) { setStatus(""); setError("Cannot check for updates. Check your connection or try again later; a release may not be published yet."); }
    } finally {
      active.current = false;
      if (mounted.current) setChecking(false);
    }
  }

  useEffect(() => {
    mounted.current = true;
    void check();
    return () => { mounted.current = false; };
  }, []);

  async function install() {
    if (busy || active.current || !info?.installSupported) return;
    active.current = true;
    setInstalling(true); onInstallingChange(true); setError(""); setStatus("Waiting for confirmation…");
    let finished = false;
    try {
      const installed = await installUpdate((event) => {
        if (!finished && mounted.current) setStatus(updateProgressText(event));
      });
      setStatus(installed ? "Installer started" : "Update cancelled — you can install it later");
    } catch (reason) {
      setStatus(""); setError(`Update failed: ${String(reason)}`);
    } finally {
      finished = true; active.current = false;
      setInstalling(false); onInstallingChange(false);
    }
  }

  if (!isTauri()) return null;
  return <section className="update-panel" aria-label="App updates">
    <button className="theme-toggle" disabled={checking || installing} onClick={() => void check()}><RefreshCw size={14} />Check for updates</button>
    <div role="status" aria-live="polite">{status}</div>
    {error && <p className="update-error" role="alert">{error}</p>}
    {info?.version && <>
      {info.notes && <details><summary>Release notes</summary><p className="update-notes">{info.notes}</p></details>}
      <button className="button secondary" disabled={busy || installing || !info.installSupported} onClick={() => void install()}><Download size={14} />Install update</button>
      {!info.installSupported && <p>Installation is available in release builds.</p>}
      {busy && !installing && <p>Finish the current operation before installing.</p>}
    </>}
  </section>;
}
