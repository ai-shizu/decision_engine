/**
 * Ambient flavor display — visually distinct from Fact slots (FLV-I-11 / SPEC §6).
 * Frozen class literals (LAW-23 / 関所 F): `bxs-flavor`, `bxs-flavor-mark`.
 * Absence (`null`) draws nothing — not an error, not an empty shell.
 */

export interface BlackboxFlavorSlotProps {
  /** Verified prose from Rust, or null when no ambient candidate. */
  text: string | null;
}

export function BlackboxFlavorSlot({ text }: BlackboxFlavorSlotProps) {
  if (text === null) {
    return null;
  }
  return (
    <p className="bxs-flavor" aria-label="Ambient flavor">
      <span className="bxs-flavor-mark" aria-hidden="true">
        ※
      </span>
      {text}
    </p>
  );
}
