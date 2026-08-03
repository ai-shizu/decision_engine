import {
  formatLaneValue,
  formatSufficiency,
  isAbsentProfile,
  type BlackboxProfileView,
} from "../../lib/blackboxProfileView";
import { bxsLatestProfile } from "../../lib/blackboxArena";

type Props = {
  profile: BlackboxProfileView | null;
  loadError: string | null;
  onReload: () => void;
};

/**
 * PROFILE UI outlet for BLACKBOX bias instrument (R-9).
 * Dumb View: parent owns fetch; this only renders honest N/A.
 */
export function BlackboxProfilePanel({ profile, loadError, onReload }: Props) {
  return (
    <section className="blackbox-profile-panel" aria-label="BLACKBOXバイアス計器">
      <header className="blackbox-profile-panel-head">
        <h3>BLACKBOX バイアス計器</h3>
        <button type="button" onClick={onReload}>
          再読込
        </button>
      </header>
      {loadError ? <p className="blackbox-profile-error">{loadError}</p> : null}
      {!profile ? (
        <p>読込中…</p>
      ) : (
        <>
          <p className="blackbox-profile-authority">{profile.authorityNote}</p>
          <dl className="blackbox-profile-meta">
            <div>
              <dt>instrument</dt>
              <dd>{profile.instrument}</dd>
            </div>
            <div>
              <dt>calibration</dt>
              <dd>{profile.calibration}</dd>
            </div>
            <div>
              <dt>pooled_campaigns</dt>
              <dd>{profile.pooledCampaigns}</dd>
            </div>
          </dl>
          {isAbsentProfile(profile) ? (
            <p>Vault にプロファイルなし — 全レーン未測定。</p>
          ) : null}
          <ul className="blackbox-profile-lanes">
            {profile.lanes.map((lane) => (
              <li key={lane.axis}>
                <strong>{lane.labelJa}</strong>
                <span> value={formatLaneValue(lane)}</span>
                <span>
                  {" "}
                  n={lane.nObs} suf={formatSufficiency(lane)}
                </span>
              </li>
            ))}
          </ul>
        </>
      )}
    </section>
  );
}

export async function fetchLatestBlackboxProfile(): Promise<BlackboxProfileView> {
  return bxsLatestProfile();
}
