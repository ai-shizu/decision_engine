/** Market ticker rail — numbers shown as-is with unit labels (no conversion). */

import type {
  ArenaLimitsView,
  MarketTickView,
} from "../../../lib/parseBlackboxArena";

export interface BlackboxMarketRailProps {
  market: MarketTickView;
  turnsCompleted: number;
  limits: ArenaLimitsView;
}

export function BlackboxMarketRail({
  market,
  turnsCompleted,
  limits,
}: BlackboxMarketRailProps) {
  return (
    <section
      className="bxs-market"
      data-regime={market.regime}
      aria-label="Market tick"
    >
      <div className="bxs-market-row">
        <span className="bxs-k">TICK</span>
        <span className="bxs-v">
          {market.tick} / {limits.campaignTicks}
        </span>
        <span className="bxs-k">TURNS</span>
        <span className="bxs-v">{turnsCompleted}</span>
        <span className="bxs-k">REGIME</span>
        <span className="bxs-v bxs-regime">{market.regime}</span>
        <span className="bxs-k">JUMP</span>
        <span className="bxs-v">{market.jumpOccurred ? "YES" : "—"}</span>
      </div>
      <div className="bxs-market-row">
        <span className="bxs-k">COMMODITY</span>
        <span className="bxs-v">{market.commodityPriceMinor} minor</span>
        <span className="bxs-k">EQUITY</span>
        <span className="bxs-v">{market.equityIndexCenti} centi</span>
        <span className="bxs-k">DEMAND</span>
        <span className="bxs-v">{market.demandIndexMicro} micro</span>
        <span className="bxs-k">RATE</span>
        <span className="bxs-v">{market.rateBp} bp</span>
      </div>
    </section>
  );
}
