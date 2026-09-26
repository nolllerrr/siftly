import { Check, ChevronDown } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { extensionPresets, matchingPreset } from "../lib/extensions";

export function ExtensionField({ value, onChange }: { value: string; onChange: (value: string) => void }) {
  const [expanded, setExpanded] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const active = matchingPreset(value);

  useEffect(() => {
    if (!expanded) return;
    function closeOutside(event: PointerEvent) {
      if (!root.current?.contains(event.target as Node)) setExpanded(false);
    }
    document.addEventListener("pointerdown", closeOutside);
    return () => document.removeEventListener("pointerdown", closeOutside);
  }, [expanded]);

  return <div className="field">
    <label htmlFor="extensions">Extensions</label>
    <div className="extension-row">
      <input id="extensions" value={value} onChange={(event) => onChange(event.target.value)} placeholder=".mp4, .mkv, .avi" aria-describedby="extensions-hint" />
      <div className="extension-picker" ref={root} onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setExpanded(false);
      }} onKeyDown={(event) => {
        if (event.key === "Escape") {
          setExpanded(false);
          trigger.current?.focus();
        }
      }}>
        <button type="button" ref={trigger} className="button secondary preset-trigger" aria-expanded={expanded} aria-controls="extension-presets" onClick={() => setExpanded(!expanded)}>
          {active?.label ?? "Choose preset"}<ChevronDown size={14} />
        </button>
        {expanded && <div id="extension-presets" className="preset-popover" role="group" aria-label="Extension presets">
          <p>Choose a file type <span>Replaces the current extensions</span></p>
          {extensionPresets.map((preset) => <button type="button" className="preset-option" key={preset.id} aria-pressed={active?.id === preset.id} onClick={() => {
            onChange(preset.extensions);
            setExpanded(false);
            trigger.current?.focus();
          }}>
            <span><strong>{preset.label}</strong><small>{preset.extensions}</small></span>
            {active?.id === preset.id && <Check size={15} />}
          </button>)}
        </div>}
      </div>
    </div>
    <small id="extensions-hint">Choose a preset or enter extensions separated by commas. You can edit any preset.</small>
  </div>;
}
