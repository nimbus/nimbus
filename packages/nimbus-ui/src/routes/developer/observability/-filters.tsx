import { Select } from "../../../components/select";

export function FilterSelect({
  label,
  value,
  options,
  onChange,
  testid,
}: {
  id?: string;
  label: string;
  value: string;
  options: Array<{ value: string; label: string }>;
  onChange: (v: string) => void;
  testid: string;
}) {
  return (
    <Select
      label={label}
      value={value}
      options={options}
      onChange={onChange}
      testid={testid}
    />
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
