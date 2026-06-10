import type { BiasCategory } from "@/types";
import { BIAS_META } from "@/utils/bias";

// Options shown in the dropdown for local (Malta) publishers
export const LOCAL_BIAS_OPTIONS: [BiasCategory, string][] = [
  ["state_owned", "State"],
  ["party_owned_pl", "Labour · PL"],
  ["party_owned_pn", "Nationalist · PN"],
  ["church_owned", "Church"],
  ["commercial_independent", "Independent"],
  ["investigative_independent", "Investigative"],
];

// Options shown in the dropdown for global publishers
export const GLOBAL_BIAS_OPTIONS: [BiasCategory, string][] = [
  ["left", "Left"],
  ["centre", "Centre"],
  ["right", "Right"],
];

// Derived from BIAS_META so the hex values are never duplicated.
export const BIAS_COLORS = Object.fromEntries(
  BIAS_META.map(m => [m.key, m.hex])
) as Record<BiasCategory, string>;
