import type { Article, BiasCoverage, BiasCategory } from "@/types";
import type { LangKey } from "@/utils/i18n";

export function computeBiasCoverage(articles: Article[]): BiasCoverage {
  if (!articles.length) return { state_owned:0,party_owned_pl:0,party_owned_pn:0,church_owned:0,commercial_independent:0,investigative_independent:0,total_articles:0 };
  const counts: Record<BiasCategory, number> = { state_owned:0,party_owned_pl:0,party_owned_pn:0,church_owned:0,commercial_independent:0,investigative_independent:0 };
  for (const a of articles) { const c = a.publisher.bias_category; if (c in counts) counts[c]++; }
  const total = articles.length;
  return { state_owned:Math.round(counts.state_owned/total*100), party_owned_pl:Math.round(counts.party_owned_pl/total*100), party_owned_pn:Math.round(counts.party_owned_pn/total*100), church_owned:Math.round(counts.church_owned/total*100), commercial_independent:Math.round(counts.commercial_independent/total*100), investigative_independent:Math.round(counts.investigative_independent/total*100), total_articles:total };
}

export interface BiasMeta { key: BiasCategory; labelKey: LangKey; shortLabelKey: LangKey; color: string; hex: string; }
export const BIAS_META: BiasMeta[] = [
  { key:"state_owned",               labelKey:"biasState",         shortLabelKey:"biasStateShort",         color:"var(--bias-state)",         hex:"#8B5CF6" },
  { key:"party_owned_pl",            labelKey:"biasPl",            shortLabelKey:"biasPlShort",            color:"var(--bias-party-pl)",      hex:"#EF4444" },
  { key:"party_owned_pn",            labelKey:"biasPn",            shortLabelKey:"biasPnShort",            color:"var(--bias-party-pn)",      hex:"#3B82F6" },
  { key:"church_owned",              labelKey:"biasChurch",        shortLabelKey:"biasChurchShort",        color:"var(--bias-church)",        hex:"#F59E0B" },
  { key:"commercial_independent",    labelKey:"biasIndependent",   shortLabelKey:"biasIndependentShort",   color:"var(--bias-commercial)",    hex:"#10B981" },
  { key:"investigative_independent", labelKey:"biasInvestigative", shortLabelKey:"biasInvestigativeShort", color:"var(--bias-investigative)", hex:"#06B6D4" },
];

export function getActiveBiasSegments(coverage: BiasCoverage) {
  return BIAS_META.map(m => ({ ...m, percentage: coverage[m.key] })).filter(s => s.percentage > 0).sort((a,b) => b.percentage - a.percentage);
}
