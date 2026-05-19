export function FilterSelect({
  id,
  label,
  value,
  options,
  onChange,
  testid,
}: {
  id: string;
  label: string;
  value: string;
  options: Array<{ value: string; label: string }>;
  onChange: (v: string) => void;
  testid: string;
}) {
  return (
    <label
      htmlFor={id}
      className="flex items-center gap-1.5 text-[10px] uppercase tracking-wide text-muted"
    >
      <span>{label}</span>
      <select
        id={id}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="rounded border border-app bg-surface px-2 py-1 font-mono text-xs text-default focus-visible:border-strong"
        data-testid={testid}
      >
        {options.map((opt) => (
          <option key={opt.value} value={opt.value}>
            {opt.label}
          </option>
        ))}
      </select>
    </label>
  );
}

export function FilterInput({
  id,
  label,
  value,
  placeholder,
  onChange,
  testid,
}: {
  id: string;
  label: string;
  value: string;
  placeholder: string;
  onChange: (v: string) => void;
  testid: string;
}) {
  return (
    <label
      htmlFor={id}
      className="flex items-center gap-1.5 text-[10px] uppercase tracking-wide text-muted"
    >
      <span>{label}</span>
      <input
        id={id}
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        className="rounded border border-app bg-surface px-2 py-1 font-mono text-xs text-default placeholder:text-muted focus-visible:border-strong"
        data-testid={testid}
      />
    </label>
  );
}
