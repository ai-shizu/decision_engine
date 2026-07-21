/**
 * GD environment setup + AGENT_PROFILES matrix.
 * Phase-locks Inner Coliseum until INITIALIZE.
 */

import type {
  GdAgentProfile,
  GdArchetype,
  GdSetupConfig,
  GdUserRole,
} from "../../../lib/gdSetupState";
import {
  GD_ARCHETYPE_OPTIONS,
  GD_PARTICIPANT_OPTIONS,
  GD_USER_ROLE_OPTIONS,
  gdSetupReady,
  syncAgentsToParticipants,
} from "../../../lib/gdSetupState";

export type GdSetupPanelProps = {
  config: GdSetupConfig;
  onChange: (next: GdSetupConfig) => void;
  onInitialize: () => void;
};

function patchAgent(
  agents: GdAgentProfile[],
  index: number,
  patch: Partial<GdAgentProfile>,
): GdAgentProfile[] {
  return agents.map((a, i) => (i === index ? { ...a, ...patch } : a));
}

export function GdSetupPanel({ config, onChange, onInitialize }: GdSetupPanelProps) {
  const ready = gdSetupReady(config);

  function setParticipants(n: number) {
    onChange({
      ...config,
      participants: n,
      agents: syncAgentsToParticipants(n, config.agents),
    });
  }

  return (
    <section className="gd-setup magi-rack" aria-label="GD environment setup">
      <div className="magi-mod-head">
        <span>[ GD_SETUP ]</span>
        <span className="micro-tel">CONTEXT · AGENT_MATRIX</span>
      </div>

      <div className="gd-setup-grid">
        <label className="gd-field">
          <span className="gd-field-label">[ THEME ] お題</span>
          <input
            type="text"
            className="gd-input"
            value={config.theme}
            placeholder="例: 売上を2倍にする施策について"
            maxLength={240}
            onChange={(e) => onChange({ ...config, theme: e.target.value })}
          />
        </label>

        <div className="gd-field-row">
          <label className="gd-field">
            <span className="gd-field-label">[ PARTICIPANTS ] 参加人数</span>
            <select
              className="gd-select"
              value={config.participants}
              onChange={(e) => setParticipants(Number(e.target.value))}
            >
              {GD_PARTICIPANT_OPTIONS.map((n) => (
                <option key={n} value={n}>
                  {n}人（自分含む）
                </option>
              ))}
            </select>
          </label>

          <label className="gd-field">
            <span className="gd-field-label">[ TIME_LIMIT ] 制限時間</span>
            <div className="gd-input-unit">
              <input
                type="number"
                className="gd-input"
                min={5}
                max={90}
                step={5}
                value={config.timeLimitMin}
                onChange={(e) =>
                  onChange({
                    ...config,
                    timeLimitMin: Math.max(1, Number(e.target.value) || 1),
                  })
                }
              />
              <span className="gd-unit">分</span>
            </div>
          </label>

          <label className="gd-field">
            <span className="gd-field-label">[ USER_ROLE ] 自身の役割</span>
            <select
              className="gd-select"
              value={config.userRole}
              onChange={(e) =>
                onChange({ ...config, userRole: e.target.value as GdUserRole })
              }
            >
              {GD_USER_ROLE_OPTIONS.map((r) => (
                <option key={r.id} value={r.id}>
                  {r.label}
                </option>
              ))}
            </select>
          </label>
        </div>
      </div>

      <div className="gd-agent-matrix" aria-label="Agent profiles">
        <div className="gd-agent-matrix-head">
          <span className="gd-field-label">[ AGENT_PROFILES ]</span>
          <span className="gd-agent-matrix-meta">
            対戦エージェント ×{config.agents.length}（自分を除く）
          </span>
        </div>

        <div className="gd-agent-rows">
          {config.agents.map((agent, i) => (
            <div key={agent.id} className="gd-agent-row">
              <span className="gd-agent-id">{agent.label}</span>

              <label className="gd-agent-arch">
                <span className="gd-agent-sub">ARCHETYPE</span>
                <select
                  className="gd-select gd-select-led"
                  value={agent.archetype}
                  onChange={(e) =>
                    onChange({
                      ...config,
                      agents: patchAgent(config.agents, i, {
                        archetype: e.target.value as GdArchetype,
                      }),
                    })
                  }
                >
                  {GD_ARCHETYPE_OPTIONS.map((a) => (
                    <option key={a.id} value={a.id}>
                      {a.label}
                    </option>
                  ))}
                </select>
              </label>

              <label className="gd-agent-slider">
                <span className="gd-agent-sub">
                  HOSTILITY <em>{Math.round(agent.hostility * 100)}</em>
                </span>
                <input
                  type="range"
                  className="gd-range"
                  min={0}
                  max={100}
                  value={Math.round(agent.hostility * 100)}
                  onChange={(e) =>
                    onChange({
                      ...config,
                      agents: patchAgent(config.agents, i, {
                        hostility: Number(e.target.value) / 100,
                      }),
                    })
                  }
                />
              </label>

              <label className="gd-agent-slider">
                <span className="gd-agent-sub">
                  COMPETENCE <em>{Math.round(agent.competence * 100)}</em>
                </span>
                <input
                  type="range"
                  className="gd-range"
                  min={0}
                  max={100}
                  value={Math.round(agent.competence * 100)}
                  onChange={(e) =>
                    onChange({
                      ...config,
                      agents: patchAgent(config.agents, i, {
                        competence: Number(e.target.value) / 100,
                      }),
                    })
                  }
                />
              </label>
            </div>
          ))}
        </div>
      </div>

      <div className="gd-setup-actions">
        <button
          type="button"
          className="coliseum-btn-cyan gd-init-btn"
          disabled={!ready}
          onClick={onInitialize}
        >
          [ INITIALIZE GD_ENVIRONMENT ]
        </button>
        {!ready && (
          <span className="gd-setup-hint">お題を入力すると初期化できます</span>
        )}
      </div>
    </section>
  );
}
