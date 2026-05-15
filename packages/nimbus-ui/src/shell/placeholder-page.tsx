export function PlaceholderPage({
  title,
  summary,
  hint,
}: {
  title: string;
  summary: string;
  hint: string;
}) {
  return (
    <section
      className="mx-auto flex h-full max-w-4xl flex-col gap-4 px-6 py-6"
      data-testid={`page-${title.toLowerCase().replace(/\s+/g, "-")}`}
    >
      <header>
        <h1
          className="text-xl text-default"
          style={{ fontSize: "var(--text-xl)" }}
        >
          {title}
        </h1>
        <p className="text-sm text-muted">{summary}</p>
      </header>
      <div className="rounded-md border bg-surface p-4 text-sm border-app text-muted">
        <div className="text-xs uppercase tracking-wider">Coming next</div>
        <div className="mt-1 text-default">{hint}</div>
      </div>
    </section>
  );
}
