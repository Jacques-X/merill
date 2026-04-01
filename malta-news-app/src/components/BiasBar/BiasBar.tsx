import { useState } from "react";
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
  const segments = getActiveBiasSegments(coverage);
  if (!segments.length) return null;

  return (
    <div className="w-full">
      {/* Bar */}
      <div
        className={`w-full flex gap-[2px] overflow-hidden ${compact ? "h-[3px] rounded-full" : "h-[5px] rounded-full"}`}
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

      {/* Legend */}
      {!compact && (
        <div className="flex items-center flex-wrap gap-x-3 gap-y-1 mt-[6px]">
          {segments.map(seg => (
            <div
              key={seg.key}
              className="flex items-center gap-1 transition-opacity duration-200"
              style={{ opacity: active && active !== seg.key ? 0.3 : 1 }}
            >
              <span
                className="w-[6px] h-[6px] rounded-full"
                style={{
                  backgroundColor: seg.hex,
                  boxShadow: `0 0 4px ${seg.hex}40`,
                }}
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
      )}
    </div>
  );
}
