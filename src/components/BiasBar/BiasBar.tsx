import { useState, useMemo } from "react";
import type { BiasCoverage } from "@/types";
import { getActiveBiasSegments } from "@/utils/bias";
import { t } from "@/utils/i18n";
import { useAppStore } from "@/store/useAppStore";

interface BiasBarProps {
  coverage: BiasCoverage;
  compact?: boolean;
}

export function BiasBar({ coverage, compact = false }: BiasBarProps) {
  const lang = useAppStore(s => s.language);
  const [active, setActive] = useState<string | null>(null);
  // Compact mode: tap the bar to reveal/hide the legend inline.
  const [legendOpen, setLegendOpen] = useState(false);
  const segments = useMemo(() => getActiveBiasSegments(coverage), [coverage]);
  if (!segments.length) return null;

  const legend = (
    <div className="flex items-center flex-wrap gap-x-3 gap-y-1 mt-[6px]">
      {segments.map(seg => (
        <div
          key={seg.key}
          className="flex items-center gap-1 transition-opacity duration-200"
          style={{ opacity: active && active !== seg.key ? 0.3 : 1 }}
        >
          <span
            className="w-[6px] h-[6px] rounded-full"
            style={{ backgroundColor: seg.hex, boxShadow: `0 0 4px ${seg.hex}40` }}
          />
          <span className="text-[10px] font-medium" style={{ color: "var(--color-label-tertiary)" }}>
            {t(lang, seg.shortLabelKey)}
          </span>
          <span className="text-[10px] font-bold" style={{ color: seg.hex }}>
            {seg.percentage}%
          </span>
        </div>
      ))}
    </div>
  );

  if (compact) {
    return (
      <div className="w-full">
        {/* Tappable bar — expanded hit area via padding so the 3px bar is reachable */}
        <button
          type="button"
          className="bias-bar-tap"
          onClick={() => setLegendOpen(o => !o)}
          aria-label={t(lang, "showBiasLegend")}
          aria-expanded={legendOpen}
        >
          <div
            className="w-full flex gap-[2px] overflow-hidden h-[3px] rounded-full"
            style={{ background: "var(--color-bg-wash)" }}
          >
            {segments.map(seg => (
              <div
                key={seg.key}
                className="h-full rounded-full"
                style={{ width: `${seg.percentage}%`, backgroundColor: seg.hex }}
              />
            ))}
          </div>
        </button>
        {legendOpen && legend}
      </div>
    );
  }

  return (
    <div className="w-full">
      {/* Bar */}
      <div
        className="w-full flex gap-[2px] overflow-hidden h-[5px] rounded-full"
        style={{ background: "var(--color-bg-wash)" }}
      >
        {segments.map(seg => (
          <div
            key={seg.key}
            className="h-full rounded-full transition-all duration-300 ease-out"
            style={{
              width: `${seg.percentage}%`,
              backgroundColor: seg.hex,
              opacity: active && active !== seg.key ? 0.25 : 1,
              boxShadow: active === seg.key ? `0 0 8px ${seg.hex}60` : "none",
            }}
            onMouseEnter={() => setActive(seg.key)}
            onMouseLeave={() => setActive(null)}
          />
        ))}
      </div>
      {legend}
    </div>
  );
}
