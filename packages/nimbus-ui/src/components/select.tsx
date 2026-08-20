import { ChevronDown } from "lucide-react";
import {
  type KeyboardEvent,
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
} from "react";

import { cn } from "../lib/cn";

export type SelectOption<T extends string> = {
  value: T;
  label: string;
};

export type SelectProps<T extends string> = {
  label: string;
  value: T;
  options: ReadonlyArray<SelectOption<T>>;
  onChange: (value: T) => void;
  placeholder?: string;
  testid?: string;
};

export function Select<T extends string>({
  label,
  value,
  options,
  onChange,
  placeholder,
  testid,
}: SelectProps<T>) {
  const [open, setOpen] = useState(false);
  const [focusIndex, setFocusIndex] = useState(0);
  const buttonRef = useRef<HTMLButtonElement | null>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const typeaheadRef = useRef<{ buffer: string; timer: number | null }>({
    buffer: "",
    timer: null,
  });
  const buttonId = useId();
  const menuId = useId();

  const currentIndex = options.findIndex((o) => o.value === value);
  const selected = currentIndex >= 0 ? options[currentIndex] : undefined;

  useEffect(() => {
    if (open) {
      setFocusIndex(currentIndex >= 0 ? currentIndex : 0);
      menuRef.current?.focus();
    }
  }, [open, currentIndex]);

  useEffect(() => {
    if (!open) return;
    function onDocClick(e: MouseEvent) {
      const target = e.target as Node;
      if (
        menuRef.current?.contains(target) ||
        buttonRef.current?.contains(target)
      ) {
        return;
      }
      setOpen(false);
    }
    document.addEventListener("mousedown", onDocClick);
    return () => document.removeEventListener("mousedown", onDocClick);
  }, [open]);

  const applySelection = useCallback(
    (next: T) => {
      onChange(next);
      setOpen(false);
      queueMicrotask(() => buttonRef.current?.focus());
    },
    [onChange],
  );

  const onTypeahead = useCallback(
    (key: string) => {
      const state = typeaheadRef.current;
      if (state.timer !== null) window.clearTimeout(state.timer);
      state.buffer = (state.buffer + key).toLowerCase();
      const match = options.findIndex((o) =>
        o.label.toLowerCase().startsWith(state.buffer),
      );
      if (match >= 0) setFocusIndex(match);
      state.timer = window.setTimeout(() => {
        state.buffer = "";
        state.timer = null;
      }, 500);
    },
    [options],
  );

  const onKeyDown = useCallback(
    (e: KeyboardEvent<HTMLDivElement>) => {
      if (options.length === 0) return;
      switch (e.key) {
        case "ArrowDown":
          e.preventDefault();
          setFocusIndex((i) => (i + 1) % options.length);
          break;
        case "ArrowUp":
          e.preventDefault();
          setFocusIndex((i) => (i - 1 + options.length) % options.length);
          break;
        case "Home":
          e.preventDefault();
          setFocusIndex(0);
          break;
        case "End":
          e.preventDefault();
          setFocusIndex(options.length - 1);
          break;
        case "Enter":
        case " ":
          e.preventDefault();
          applySelection(options[focusIndex].value);
          break;
        case "Escape":
          e.preventDefault();
          setOpen(false);
          queueMicrotask(() => buttonRef.current?.focus());
          break;
        default:
          if (e.key.length === 1 && !e.metaKey && !e.ctrlKey && !e.altKey) {
            onTypeahead(e.key);
          }
      }
    },
    [applySelection, options, focusIndex, onTypeahead],
  );

  const onTriggerKeyDown = useCallback(
    (e: KeyboardEvent<HTMLButtonElement>) => {
      if (e.key === "ArrowDown" || e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        setOpen(true);
      }
    },
    [],
  );

  const triggerLabel = selected?.label ?? placeholder ?? "Select…";

  return (
    <label
      htmlFor={buttonId}
      className="flex items-center gap-1.5 text-xs uppercase tracking-wide text-muted"
    >
      <span>{label}</span>
      <span className="relative inline-flex">
        <button
          id={buttonId}
          ref={buttonRef}
          type="button"
          aria-haspopup="listbox"
          aria-expanded={open}
          aria-controls={open ? menuId : undefined}
          onClick={() => setOpen((v) => !v)}
          onKeyDown={onTriggerKeyDown}
          data-testid={testid}
          className={cn(
            "flex h-[26px] items-center gap-1.5 rounded border border-app bg-surface px-2 font-mono text-xs text-default focus-visible:border-strong",
            open && "border-strong",
          )}
        >
          <span className="truncate normal-case tracking-normal">
            {triggerLabel}
          </span>
          <ChevronDown
            size={12}
            aria-hidden
            className={cn(
              "shrink-0 text-muted transition-transform",
              open && "rotate-180",
            )}
          />
        </button>
        {open ? (
          <div
            ref={menuRef}
            id={menuId}
            role="listbox"
            tabIndex={-1}
            aria-label={label}
            aria-activedescendant={
              focusIndex >= 0 ? `${menuId}-opt-${focusIndex}` : undefined
            }
            onKeyDown={onKeyDown}
            data-testid={testid ? `${testid}-menu` : undefined}
            // z-30 is the console's popover band, above the z-20 sticky table
            // heads this menu opens over. At z-20 it tied with them, and CSS
            // resolves an equal z-index by tree order: the storage query bar
            // renders before the documents table, so the table's opaque head
            // painted over the first option and swallowed its clicks (hitting
            // a column header, which sorts and dismisses the menu). The
            // sibling ColumnChooser in the same bar already sits at z-30.
            className="absolute left-0 top-full z-30 mt-1 max-h-72 min-w-full overflow-auto rounded-md border border-app bg-surface shadow-lg focus:outline-none"
          >
            <ul className="flex flex-col gap-px py-1">
              {options.map((option, idx) => {
                const isActive = option.value === value;
                const isFocused = idx === focusIndex;
                return (
                  <li key={option.value}>
                    <button
                      id={`${menuId}-opt-${idx}`}
                      type="button"
                      role="option"
                      aria-selected={isActive}
                      data-testid={
                        testid ? `${testid}-option-${option.value}` : undefined
                      }
                      data-active={isActive ? "true" : "false"}
                      data-focused={isFocused ? "true" : "false"}
                      onMouseEnter={() => setFocusIndex(idx)}
                      onClick={() => applySelection(option.value)}
                      className={cn(
                        "flex h-8 w-full items-center justify-between gap-2 px-3 text-left normal-case tracking-normal font-mono text-xs",
                        isFocused
                          ? "bg-surface-2 text-default"
                          : "text-muted hover:bg-surface-2 hover:text-default",
                        isActive && "text-default",
                      )}
                    >
                      <span className="flex-1 truncate">{option.label}</span>
                      {isActive ? (
                        // Carries the active row's own tone, not `text-brand`:
                        // the brand hue rendered this 11px glyph at 2.48:1 on
                        // --surface in the warm palette, under both the 4.5:1
                        // text floor and the 3:1 non-text floor it qualifies
                        // for as an aria-hidden state marker.
                        <span
                          aria-hidden
                          className="font-mono text-xs text-default"
                        >
                          ●
                        </span>
                      ) : null}
                    </button>
                  </li>
                );
              })}
            </ul>
          </div>
        ) : null}
      </span>
    </label>
  );
}
