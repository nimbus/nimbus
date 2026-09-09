import { cn } from "@/lib/utils";

/**
 * A settings page section: a titled header rule plus its content.
 *
 * DESIGN.md's do-not list keeps page sections out of decorative cards — cards
 * are for repeated items, small metrics, modals, and genuinely framed tools.
 * A section therefore separates itself with a header rule, not a box, so the
 * tables, lists, and repeated cards *inside* it are the only frames the eye
 * has to read.
 *
 * `framed` is the exception, not the default: pass it only where the frame
 * carries meaning, as the danger zone's hazard boundary does around the
 * shutdown and token-rotation controls.
 */
export function PageSection({
  title,
  description,
  testid,
  tone,
  framed = false,
  children,
}: {
  title: string;
  description?: React.ReactNode;
  testid: string;
  tone?: "default" | "danger";
  framed?: boolean;
  children: React.ReactNode;
}) {
  const danger = tone === "danger";
  const ruleClass = danger ? "border-error/40" : "border-border-2";
  return (
    <section
      data-testid={testid}
      className={cn(
        "flex flex-col gap-3",
        framed && `rounded-md border p-4 ${ruleClass}`,
      )}
    >
      <header className={cn("border-b pb-2", ruleClass)}>
        <h2
          className={cn("text-sm", danger ? "text-error" : "text-text-1")}
          style={{ fontSize: "var(--text-base)" }}
        >
          {title}
        </h2>
        {description ? (
          <p className="text-xs text-text-3">{description}</p>
        ) : null}
      </header>
      {children}
    </section>
  );
}

export function DefinitionList({
  children,
  compact,
}: {
  children: React.ReactNode;
  compact?: boolean;
}) {
  return (
    <dl
      className={`grid grid-cols-1 gap-x-4 gap-y-2 sm:grid-cols-2 ${compact ? "" : "lg:grid-cols-3"}`}
    >
      {children}
    </dl>
  );
}

export function Definition({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-0.5">
      <dt className="text-xs font-medium text-text-3">{label}</dt>
      <dd className="text-sm text-text-1">{children}</dd>
    </div>
  );
}

export function Cell({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1 bg-bg-panel px-3 py-2">
      <span className="text-xs font-medium text-text-3">{label}</span>
      <span className="text-sm">{children}</span>
    </div>
  );
}
