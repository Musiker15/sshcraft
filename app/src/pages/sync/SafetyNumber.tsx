/** A safety number, large and in groups, so two people can read it to each other. */
export function SafetyNumber({ value }: { value: string }) {
  const groups = value.split(" ").filter(Boolean);
  return (
    <div className="safety-number" aria-label={value}>
      {groups.map((group, i) => (
        <span key={i} className="safety-group">
          {group}
        </span>
      ))}
    </div>
  );
}
