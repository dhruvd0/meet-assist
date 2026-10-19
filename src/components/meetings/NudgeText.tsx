const LABELED = /^([A-Z][A-Za-z ]{1,15}):\s*(.+)$/;

/** A nudge's text. Loop checklists ("Circle: …", "Decide now: …") render
 * as labeled rows; anything else as plain lines. */
export function NudgeText({
  text,
  className = "",
}: {
  text: string;
  className?: string;
}) {
  const lines = text.split("\n").filter((l) => l.trim());
  const rows = lines.map((l) => LABELED.exec(l.trim()));
  if (lines.length < 2 || rows.some((r) => !r)) {
    return <p className={`whitespace-pre-line ${className}`}>{text}</p>;
  }
  return (
    <div className={`flex flex-col gap-1 ${className}`}>
      {rows.map((r, i) => (
        <p key={i}>
          <span className="me-1.5 text-[0.8em] font-medium text-muted">
            {r?.[1]}
          </span>
          {r?.[2]}
        </p>
      ))}
    </div>
  );
}
