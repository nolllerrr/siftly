import { invoke, isTauri } from "@tauri-apps/api/core";
import {
  AlertCircle,
  ArrowDownUp,
  Check,
  ChevronLeft,
  Copy,
  FileSearch,
  Files,
  FolderOpen,
  MonitorUp,
  Moon,
  Search,
  Square,
  Sun,
  X,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { DatePicker, formatDateValue } from "./components/DatePicker";
import { ExtensionField } from "./components/ExtensionField";
import { normalizeExtensions } from "./lib/extensions";
import { selectAllFiles, selectedFiles as filterSelectedFiles, togglePathSelection } from "./lib/selection";
import { copyDisplay, runOperation } from "./lib/operations";
import type {
  CopyProgress,
  CopyResult,
  FileResult,
  OperationError,
  ScanProgress,
  ScanResult,
} from "./types";

type Page = "search" | "results";
type SortKey = "name" | "size" | "modified_time";

const today = formatDateValue(new Date());

function hasNativeBridge(): boolean {
  const internals = (window as unknown as {
    __TAURI_INTERNALS__?: { transformCallback?: unknown };
  }).__TAURI_INTERNALS__;
  return isTauri() && typeof internals?.transformCallback === "function";
}

function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / 1024 ** index;
  return `${value.toFixed(index === 0 || value >= 100 ? 0 : value >= 10 ? 1 : 2)} ${units[index]}`;
}

function operationId(prefix: string): string {
  return `${prefix}-${Date.now()}-${crypto.randomUUID()}`;
}

export default function App() {
  const [page, setPage] = useState<Page>("search");
  const [theme, setTheme] = useState<"light" | "dark">(() =>
    window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light",
  );
  const [source, setSource] = useState("");
  const [destination, setDestination] = useState("");
  const [extensionInput, setExtensionInput] = useState(".mp4, .mkv, .avi");
  const [dateFrom, setDateFrom] = useState("");
  const [dateTo, setDateTo] = useState(today);
  const [results, setResults] = useState<FileResult[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [scanStats, setScanStats] = useState<ScanResult | null>(null);
  const [scanProgress, setScanProgress] = useState<ScanProgress | null>(null);
  const [copyProgress, setCopyProgress] = useState<CopyProgress | null>(null);
  const [copyResult, setCopyResult] = useState<CopyResult | null>(null);
  const [errors, setErrors] = useState<OperationError[]>([]);
  const [fatalError, setFatalError] = useState("");
  const [scanId, setScanId] = useState("");
  const [copyId, setCopyId] = useState("");
  const [isScanning, setIsScanning] = useState(false);
  const [isCopying, setIsCopying] = useState(false);
  const [showConfirm, setShowConfirm] = useState(false);
  const [sortKey, setSortKey] = useState<SortKey>("modified_time");
  const [sortAscending, setSortAscending] = useState(false);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  const sortedResults = useMemo(() => [...results].sort((left, right) => {
    const first = left[sortKey];
    const second = right[sortKey];
    const comparison = typeof first === "number"
      ? first - (second as number)
      : String(first).localeCompare(String(second), undefined, { numeric: true });
    return sortAscending ? comparison : -comparison;
  }), [results, sortAscending, sortKey]);

  const selectedFiles = useMemo(
    () => filterSelectedFiles(results, selected),
    [results, selected],
  );
  const selectedBytes = selectedFiles.reduce((sum, file) => sum + file.size, 0);
  const totalBytes = results.reduce((sum, file) => sum + file.size, 0);

  async function chooseFolder(kind: "source" | "destination") {
    if (!hasNativeBridge()) {
      setFatalError("Folder selection is available in the Siftly desktop window. Start it with ‘pnpm tauri dev’ and do not use the localhost browser tab.");
      return;
    }
    try {
      const folder = await invoke<string | null>("choose_folder", { kind });
      if (folder) {
        (kind === "source" ? setSource : setDestination)(folder);
        if (kind === "source") { setResults([]); setSelected(new Set()); setScanStats(null); }
      }
    } catch (error) {
      setFatalError(`Cannot open the folder picker: ${String(error)}`);
    }
  }

  async function startScan() {
    if (!hasNativeBridge()) {
      setFatalError("Scanning is available only in the Siftly desktop window. Run ‘pnpm tauri dev’ and use that window.");
      return;
    }
    const extensions = normalizeExtensions(extensionInput);
    if (!source || extensions.length === 0) {
      setFatalError("Choose a source folder and enter at least one extension.");
      return;
    }
    const id = operationId("scan");
    setScanId(id);
    setIsScanning(true);
    setScanProgress({ files_checked: 0, files_matched: 0, current_directory: source });
    setFatalError("");
    setErrors([]);
    setCopyResult(null);
    try {
      const result = await runOperation<ScanResult>("scan", id, {
          source,
          extensions,
          date_from: dateFrom ? new Date(`${dateFrom}T00:00:00`).toISOString() : null,
          date_to: dateTo ? new Date(`${dateTo}T23:59:59.999`).toISOString() : null,
        }, (event) => {
          if (event.type === "scan_progress") setScanProgress(event);
      });
      setResults(result.files);
      setSelected(selectAllFiles(result.files));
      setScanStats(result);
      setErrors(result.errors);
      setPage("results");
    } catch (error) {
      setFatalError(String(error));
    } finally {
      setIsScanning(false);
      setScanId("");
    }
  }

  async function cancelOperation(id: string) {
    try {
      if (id) await invoke("cancel_operation", { opId: id });
    } catch (error) {
      setFatalError(`Cannot cancel operation: ${String(error)}`);
    }
  }

  async function startCopy() {
    if (!destination || selectedFiles.length === 0 || isScanning || isCopying) return;
    const id = operationId("copy");
    setCopyId(id);
    setShowConfirm(false);
    setIsCopying(true);
    setCopyResult(null);
    setFatalError("");
    setCopyProgress({ completed: 0, total: selectedFiles.length, bytes_copied: 0, total_bytes: selectedBytes, current_file: "" });
    try {
      const result = await runOperation<CopyResult>("copy", id, { files: selectedFiles, destination }, (event) => {
        if (event.type === "copy_progress") setCopyProgress(event);
      });
      setCopyResult(result);
      setErrors((previous) => [...previous, ...result.errors]);
    } catch (error) {
      setFatalError(String(error));
      setCopyProgress(null);
    } finally {
      setIsCopying(false);
      setCopyId("");
    }
  }

  function toggleSelection(path: string) {
    setSelected((current) => togglePathSelection(current, path));
  }

  function changeSort(nextKey: SortKey) {
    if (sortKey === nextKey) setSortAscending((value) => !value);
    else {
      setSortKey(nextKey);
      setSortAscending(true);
    }
  }

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand"><div className="brand-mark"><FileSearch size={18} /></div><div className="brand-copy"><span>Siftly</span><small>Find. Filter. Copy.</small></div></div>
        <nav aria-label="Main navigation">
          <button className={page === "search" ? "nav-item active" : "nav-item"} onClick={() => setPage("search")}><Search size={16} />Search</button>
          <button className={page === "results" ? "nav-item active" : "nav-item"} onClick={() => setPage("results")} disabled={!scanStats}><Files size={16} />Results{results.length > 0 && <span className="nav-count">{results.length}</span>}</button>
        </nav>
        <div className="sidebar-bottom">
          <button className="theme-toggle" onClick={() => setTheme(theme === "light" ? "dark" : "light")} aria-label={`Switch to ${theme === "light" ? "dark" : "light"} theme`}>
            {theme === "light" ? <Moon size={15} /> : <Sun size={15} />}{theme === "light" ? "Dark" : "Light"} theme
          </button>
          <span>Siftly 0.1.0 · MVP</span>
        </div>
      </aside>

      <main className="workspace">
        {!hasNativeBridge() && (
          <div className="preview-notice">
            <MonitorUp size={17} />
            <span><strong>Browser preview.</strong> Folder selection and scanning work in the Siftly desktop window. Run <code>pnpm tauri dev</code>.</span>
          </div>
        )}
        {fatalError && <div className="alert error"><AlertCircle size={17} /><span>{fatalError}</span><button onClick={() => setFatalError("")} aria-label="Dismiss error"><X size={15} /></button></div>}

        {page === "search" ? (
          <section className="page narrow-page">
            <header className="page-header"><div><p className="eyebrow">New search</p><h1>Find files</h1><p>Choose where to look and define the time range. Scanning never changes your files.</p></div></header>

            <div className="card form-card">
              <FolderField label="Source folder" value={source} placeholder="Choose a folder to scan" onBrowse={() => void chooseFolder("source")} />
              <ExtensionField value={extensionInput} onChange={setExtensionInput} />
              <div className="field-grid">
                <DatePicker id="date-from" label="From" value={dateFrom} max={dateTo || undefined} onChange={setDateFrom} />
                <DatePicker id="date-to" label="To" value={dateTo} min={dateFrom || undefined} align="right" onChange={setDateTo} />
              </div>
              <FolderField label="Destination folder" value={destination} placeholder="Choose where selected files will be copied" onBrowse={() => void chooseFolder("destination")} />
            </div>

            {isScanning && scanProgress ? (
              <div className="operation-card">
                <div className="operation-heading"><div><span className="status-dot pulse" />Scanning files</div><button className="button secondary danger-text" onClick={() => void cancelOperation(scanId)}><Square size={14} />Cancel scan</button></div>
                <div className="progress-indeterminate" />
                <div className="scan-stats"><span><strong>{scanProgress.files_checked.toLocaleString()}</strong> checked</span><span><strong>{scanProgress.files_matched.toLocaleString()}</strong> matched</span></div>
                <p className="current-path" title={scanProgress.current_directory}>{scanProgress.current_directory}</p>
              </div>
            ) : (
              <button className="button primary main-action" onClick={() => void startScan()}><Search size={17} />Scan files</button>
            )}
          </section>
        ) : (
          <section className="page results-page">
            <header className="page-header results-header">
              <div><button className="back-link" onClick={() => setPage("search")}><ChevronLeft size={15} />Search</button><h1>Results</h1><p>{scanStats?.cancelled ? "Scan cancelled — showing partial results." : "Review the matches and choose what to copy."}</p></div>
              <div className="result-actions"><button className="button secondary" onClick={() => setSelected(new Set())}>Select none</button><button className="button secondary" onClick={() => setSelected(new Set(results.map((file) => file.path)))}>Select all</button></div>
            </header>

            <div className="stat-row">
              <Stat label="Found" value={`${results.length.toLocaleString()} files`} />
              <Stat label="Total size" value={formatBytes(totalBytes)} />
              <Stat label="Scanned" value={`${scanStats?.files_checked.toLocaleString() ?? 0} files`} />
              <Stat label="Time" value={`${scanStats?.duration.toFixed(1) ?? "0.0"} sec`} />
            </div>

            <div className="table-card">
              <div className="table-scroll">
                <table>
                  <thead><tr><th className="check-cell"><input type="checkbox" aria-label="Select all files" checked={results.length > 0 && selected.size === results.length} onChange={(event) => setSelected(event.target.checked ? selectAllFiles(results) : new Set())} /></th><SortableHeader label="Name" active={sortKey === "name"} onClick={() => changeSort("name")} /><th>Folder</th><SortableHeader label="Size" active={sortKey === "size"} onClick={() => changeSort("size")} /><SortableHeader label="Modified" active={sortKey === "modified_time"} onClick={() => changeSort("modified_time")} /></tr></thead>
                  <tbody>{sortedResults.map((file) => <tr key={file.path} className={selected.has(file.path) ? "selected-row" : ""}><td className="check-cell"><input type="checkbox" aria-label={`Select ${file.name}`} checked={selected.has(file.path)} onChange={() => toggleSelection(file.path)} /></td><td className="name-cell" title={file.path}><span className="file-icon"><Files size={15} /></span><span>{file.name}</span></td><td className="path-cell" title={file.parent_folder}>{file.parent_folder}</td><td className="size-cell">{formatBytes(file.size)}</td><td className="date-cell">{new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(new Date(file.modified_time))}</td></tr>)}</tbody>
                </table>
                {results.length === 0 && <div className="empty-state"><FileSearch size={28} /><strong>No matching files</strong><span>Adjust the extensions or date range and scan again.</span></div>}
              </div>
            </div>

            {errors.length > 0 && <details className="error-details"><summary>{errors.length} file system {errors.length === 1 ? "error" : "errors"}</summary>{errors.map((error, index) => <div key={`${error.path}-${index}`}><strong>{error.path}</strong><span>{error.message}</span></div>)}</details>}

            <div className="copy-dock">
              <div><strong>{selected.size.toLocaleString()} selected</strong><span>{formatBytes(selectedBytes)}</span></div>
              <button className="button primary" disabled={!destination || selected.size === 0 || isCopying} onClick={() => setShowConfirm(true)}><Copy size={16} />Copy selected</button>
            </div>

            {(isCopying || copyResult) && <CopyPanel progress={copyProgress} result={copyResult} onCancel={() => void cancelOperation(copyId)} />}
          </section>
        )}
      </main>

      {showConfirm && <div className="modal-backdrop" role="presentation"><div className="modal" role="dialog" aria-modal="true" aria-labelledby="copy-title"><div className="modal-icon"><Copy size={20} /></div><h2 id="copy-title">Copy {selectedFiles.length.toLocaleString()} files?</h2><p>Existing files will never be overwritten. Name collisions are resolved with a numeric suffix.</p><dl><div><dt>Destination</dt><dd title={destination}>{destination}</dd></div><div><dt>Total size</dt><dd>{formatBytes(selectedBytes)}</dd></div><div><dt>Preserve dates</dt><dd>Created, modified, accessed</dd></div></dl><div className="modal-actions"><button className="button secondary" onClick={() => setShowConfirm(false)}>Cancel</button><button className="button primary" onClick={() => void startCopy()}>Start copying</button></div></div></div>}
    </div>
  );
}

function FolderField({ label, value, placeholder, onBrowse }: { label: string; value: string; placeholder: string; onBrowse: () => void }) {
  return <div className="field"><label>{label}</label><div className="folder-input"><input value={value} readOnly placeholder={placeholder} title={value} /><button className="button secondary" onClick={onBrowse}><FolderOpen size={16} />Browse</button></div></div>;
}

function Stat({ label, value }: { label: string; value: string }) {
  return <div className="stat-card"><span>{label}</span><strong>{value}</strong></div>;
}

function SortableHeader({ label, active, onClick }: { label: string; active: boolean; onClick: () => void }) {
  return <th><button className={active ? "sort-button active" : "sort-button"} onClick={onClick}>{label}<ArrowDownUp size={13} /></button></th>;
}

function CopyPanel({ progress, result, onCancel }: { progress: CopyProgress | null; result: CopyResult | null; onCancel: () => void }) {
  const { completed, total, bytes, totalBytes, percentage } = copyDisplay(progress, result);
  const hasErrors = Boolean(result?.failed || result?.errors.length);
  return <div className="operation-card copy-panel">
    <div className="operation-heading"><div>
      {result ? <>
        {hasErrors ? <AlertCircle size={17} /> : <Check size={17} className="success-icon" />}
        {result.cancelled ? "Copy cancelled" : hasErrors ? "Copy finished with errors" : "Copy finished"}
      </> : <><span className="status-dot pulse" />Copying files</>}
    </div>{!result && <button className="button secondary danger-text" onClick={onCancel}><Square size={14} />Cancel</button>}</div>
    <div className="progress-track"><div style={{ width: `${percentage}%` }} /></div>
    <div className="scan-stats"><span><strong>{completed} / {total}</strong> processed</span><span><strong>{formatBytes(bytes)} / {formatBytes(totalBytes)}</strong></span><span><strong>{percentage}%</strong></span></div>
    {progress?.current_file && !result && <p className="current-path">{progress.current_file}</p>}
    {result && <p className="copy-summary">Copied {result.copied}. Failed {result.failed}. Skipped {result.skipped}.</p>}
  </div>;
}
