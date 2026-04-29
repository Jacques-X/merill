import type { BiasCategory } from "@/types";

export const BIAS_LABELS: Record<BiasCategory, string> = {
  state_owned: "State",
  party_owned_pl: "Labour · PL",
  party_owned_pn: "Nationalist · PN",
  church_owned: "Church",
  commercial_independent: "Independent",
  investigative_independent: "Investigative",
  left: "Left",
  centre: "Centre",
  right: "Right",
};

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

export const BIAS_COLORS: Record<BiasCategory, string> = {
  state_owned: "#8B5CF6",
  party_owned_pl: "#EF4444",
  party_owned_pn: "#3B82F6",
  church_owned: "#F59E0B",
  commercial_independent: "#10B981",
  investigative_independent: "#06B6D4",
  left: "#EF4444",
  centre: "#8E8E93",
  right: "#3B82F6",
};
