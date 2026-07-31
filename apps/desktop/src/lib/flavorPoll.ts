/**
 * Bounded pull for ambient arena flavor (Tier 3 §13.4).
 *
 * Generation is queued off the turn path (A-4 / 関所 G), so the value lands
 * *after* `bxs_advance` returns — measured at ~267ms on device. A single pull
 * at T+0 therefore always finds an empty slot, and the next turn's pull
 * carries an incremented tick that correlation-mismatches and is discarded
 * (FLV-I-13). Every generated value was lost: too early for the pull that just
 * happened, too stale for the next one. Deterministic, not a race.
 *
 * This fixes it from the caller's side only. The correlation contract is
 * deliberately untouched: polls run inside the tick the value belongs to, so
 * `take` still matches exactly and Rust still discards genuinely stale results
 * (P-2-2…5). Loosening the tick check was the other available repair and is
 * the more dangerous one — it would trade a display bug for the possibility of
 * showing prose from a superseded turn.
 *
 * Never awaited on the turn path: the caller fires this and returns, so the
 * arena stays responsive and A-4's "never waits on the model" still holds.
 */

export interface FlavorPollDeps {
  /** One `bxs_take_flavor` call: prose, or null when the slot is empty/stale. */
  take: () => Promise<string | null>;
  sleep: (ms: number) => Promise<void>;
  /** True once this poll is superseded — new turn, abort, or unmount. */
  cancelled: () => boolean;
}

/** ~2.2s of wall clock against a measured ~267ms generation. */
export const FLAVOR_POLL_ATTEMPTS = 12;
export const FLAVOR_POLL_INTERVAL_MS = 200;

/**
 * Pull until prose arrives, the budget runs out, or the poll is superseded.
 * Returns null on the latter two — indistinguishable to the caller by design,
 * since both mean "draw nothing for this tick".
 */
export async function pollForFlavor(
  deps: FlavorPollDeps,
  attempts: number = FLAVOR_POLL_ATTEMPTS,
  intervalMs: number = FLAVOR_POLL_INTERVAL_MS,
): Promise<string | null> {
  for (let i = 0; i < attempts; i += 1) {
    // Checked before the call as well as after: a poll superseded while the
    // previous await was in flight must not issue another `take`, because
    // `take` consumes — a stray pull would swallow the next tick's value.
    if (deps.cancelled()) return null;
    const prose = await deps.take();
    if (deps.cancelled()) return null;
    if (prose !== null) return prose;
    if (i + 1 < attempts) await deps.sleep(intervalMs);
  }
  return null;
}
