import {
  CalendarDays,
  ChevronLeft,
  ChevronRight,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { formatDateValue, isDateOutsideRange, parseDateValue } from "../lib/date";

export { formatDateValue } from "../lib/date";

type DatePickerProps = {
  id: string;
  label: string;
  value: string;
  min?: string;
  max?: string;
  align?: "left" | "right";
  onChange: (value: string) => void;
};

const calendarFormatter = new Intl.DateTimeFormat(undefined, {
  month: "long",
  year: "numeric",
});
const displayFormatter = new Intl.DateTimeFormat(undefined, {
  day: "2-digit",
  month: "short",
  year: "numeric",
});
const weekdayFormatter = new Intl.DateTimeFormat(undefined, { weekday: "short" });

function startOfMonth(value: Date): Date {
  return new Date(value.getFullYear(), value.getMonth(), 1);
}

function addMonths(value: Date, amount: number): Date {
  return new Date(value.getFullYear(), value.getMonth() + amount, 1);
}

export function DatePicker({
  id,
  label,
  value,
  min,
  max,
  align = "left",
  onChange,
}: DatePickerProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const selectedDate = parseDateValue(value);
  const [isOpen, setIsOpen] = useState(false);
  const [visibleMonth, setVisibleMonth] = useState(() =>
    startOfMonth(selectedDate ?? new Date()),
  );

  useEffect(() => {
    if (selectedDate) setVisibleMonth(startOfMonth(selectedDate));
  }, [value]);

  useEffect(() => {
    if (!isOpen) return;
    const handlePointerDown = (event: PointerEvent) => {
      if (!containerRef.current?.contains(event.target as Node)) setIsOpen(false);
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setIsOpen(false);
    };
    document.addEventListener("pointerdown", handlePointerDown);
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("pointerdown", handlePointerDown);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [isOpen]);

  const weekdays = useMemo(() => {
    const monday = new Date(2024, 0, 1);
    return Array.from({ length: 7 }, (_, index) => {
      const date = new Date(monday);
      date.setDate(monday.getDate() + index);
      return weekdayFormatter.format(date).replace(".", "");
    });
  }, []);

  const days = useMemo(() => {
    const firstWeekday = (visibleMonth.getDay() + 6) % 7;
    const gridStart = new Date(
      visibleMonth.getFullYear(),
      visibleMonth.getMonth(),
      1 - firstWeekday,
    );
    return Array.from({ length: 42 }, (_, index) => {
      const date = new Date(gridStart);
      date.setDate(gridStart.getDate() + index);
      return date;
    });
  }, [visibleMonth]);

  const todayValue = formatDateValue(new Date());

  function chooseDate(date: Date) {
    const nextValue = formatDateValue(date);
    if (isDateOutsideRange(nextValue, min, max)) return;
    onChange(nextValue);
    setIsOpen(false);
  }

  function chooseToday() {
    if (isDateOutsideRange(todayValue, min, max)) return;
    onChange(todayValue);
    setVisibleMonth(startOfMonth(new Date()));
    setIsOpen(false);
  }

  return (
    <div className="field date-picker" ref={containerRef}>
      <label id={`${id}-label`}>{label}</label>
      <button
        id={id}
        type="button"
        className={value ? "date-trigger" : "date-trigger placeholder"}
        aria-labelledby={`${id}-label`}
        aria-expanded={isOpen}
        aria-haspopup="dialog"
        onClick={() => setIsOpen((current) => !current)}
      >
        <span>{selectedDate ? displayFormatter.format(selectedDate) : "Any date"}</span>
        <CalendarDays size={16} />
      </button>

      {isOpen && (
        <div
          className={`calendar-popover ${align === "right" ? "align-right" : ""}`}
          role="dialog"
          aria-label={`${label} calendar`}
        >
          <div className="calendar-header">
            <button
              type="button"
              aria-label="Previous month"
              onClick={() => setVisibleMonth((current) => addMonths(current, -1))}
            >
              <ChevronLeft size={16} />
            </button>
            <strong>{calendarFormatter.format(visibleMonth)}</strong>
            <button
              type="button"
              aria-label="Next month"
              onClick={() => setVisibleMonth((current) => addMonths(current, 1))}
            >
              <ChevronRight size={16} />
            </button>
          </div>

          <div className="calendar-weekdays" aria-hidden="true">
            {weekdays.map((weekday, index) => <span key={`${weekday}-${index}`}>{weekday}</span>)}
          </div>
          <div className="calendar-grid">
            {days.map((date) => {
              const dateValue = formatDateValue(date);
              const isMuted = date.getMonth() !== visibleMonth.getMonth();
              const isSelected = dateValue === value;
              const isToday = dateValue === todayValue;
              const isDisabled = isDateOutsideRange(dateValue, min, max);
              return (
                <button
                  type="button"
                  key={dateValue}
                  className={[
                    "calendar-day",
                    isMuted ? "muted" : "",
                    isToday ? "today" : "",
                    isSelected ? "selected" : "",
                  ].filter(Boolean).join(" ")}
                  disabled={isDisabled}
                  aria-pressed={isSelected}
                  aria-label={displayFormatter.format(date)}
                  onClick={() => chooseDate(date)}
                >
                  {date.getDate()}
                </button>
              );
            })}
          </div>

          <div className="calendar-footer">
            <button type="button" onClick={() => { onChange(""); setIsOpen(false); }}>Clear</button>
            <button type="button" disabled={isDateOutsideRange(todayValue, min, max)} onClick={chooseToday}>Today</button>
          </div>
        </div>
      )}
    </div>
  );
}
