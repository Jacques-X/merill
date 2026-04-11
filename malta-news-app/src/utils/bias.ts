import type { Article, BiasCoverage, BiasCategory } from "@/types";
import type { LangKey } from "@/utils/i18n";

export function computeBiasCoverage(articles: Article[], overrides: Record<string, BiasCategory> = {}): BiasCoverage {
  const zero = { state_owned:0,party_owned_pl:0,party_owned_pn:0,church_owned:0,commercial_independent:0,investigative_independent:0,left:0,centre:0,right:0,total_articles:0 };
  if (!articles.length) return zero;

  const counts: Record<BiasCategory, number> = { state_owned:0,party_owned_pl:0,party_owned_pn:0,church_owned:0,commercial_independent:0,investigative_independent:0,left:0,centre:0,right:0 };
  for (const a of articles) {
    const c = overrides[a.publisher_id] ?? (a.publisher.is_global ? "centre" : a.publisher.bias_category);
    if (c in counts) counts[c as BiasCategory]++;
  }
  const total = articles.length;

  // All 9 categories — local and global — go through the same largest-remainder pass
  // so the bar always sums to 100 and global (left/centre/right) segments render correctly.
  const keys: BiasCategory[] = ["state_owned","party_owned_pl","party_owned_pn","church_owned","commercial_independent","investigative_independent","left","centre","right"];
  const exact = keys.map(k => counts[k] / total * 100);
  const floored = exact.map(Math.floor);
  const remainder = 100 - floored.reduce((a, b) => a + b, 0);
  const fractions = exact.map((v, i) => ({ i, frac: v - floored[i] })).sort((a, b) => b.frac - a.frac);
  for (let j = 0; j < remainder && j < fractions.length; j++) floored[fractions[j].i]++;
  const [state_owned,party_owned_pl,party_owned_pn,church_owned,commercial_independent,investigative_independent,left,centre,right] = floored;

  return { state_owned,party_owned_pl,party_owned_pn,church_owned,commercial_independent,investigative_independent,left,centre,right,total_articles:total };
}

export interface BiasMeta { key: BiasCategory; labelKey: LangKey; shortLabelKey: LangKey; color: string; hex: string; }
export const BIAS_META: BiasMeta[] = [
  { key:"state_owned",               labelKey:"biasState",         shortLabelKey:"biasStateShort",         color:"var(--bias-state)",         hex:"#8B5CF6" },
  { key:"party_owned_pl",            labelKey:"biasPl",            shortLabelKey:"biasPlShort",            color:"var(--bias-party-pl)",      hex:"#EF4444" },
  { key:"party_owned_pn",            labelKey:"biasPn",            shortLabelKey:"biasPnShort",            color:"var(--bias-party-pn)",      hex:"#3B82F6" },
  { key:"church_owned",              labelKey:"biasChurch",        shortLabelKey:"biasChurchShort",        color:"var(--bias-church)",        hex:"#F59E0B" },
  { key:"commercial_independent",    labelKey:"biasIndependent",   shortLabelKey:"biasIndependentShort",   color:"var(--bias-commercial)",    hex:"#10B981" },
  { key:"investigative_independent", labelKey:"biasInvestigative", shortLabelKey:"biasInvestigativeShort", color:"var(--bias-investigative)", hex:"#06B6D4" },
  { key:"left",                      labelKey:"biasLeft",          shortLabelKey:"biasLeftShort",          color:"#EF4444",                   hex:"#EF4444" },
  { key:"centre",                    labelKey:"biasCentre",        shortLabelKey:"biasCentreShort",        color:"#8E8E93",                   hex:"#8E8E93" },
  { key:"right",                     labelKey:"biasRight",         shortLabelKey:"biasRightShort",         color:"#3B82F6",                   hex:"#3B82F6" },
];

export function getActiveBiasSegments(coverage: BiasCoverage) {
  return BIAS_META.map(m => ({ ...m, percentage: coverage[m.key] })).filter(s => s.percentage > 0).sort((a,b) => b.percentage - a.percentage);
}
