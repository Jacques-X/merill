import { useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import type { ClustersResponse, RefreshResult, RefreshStatus, Publisher } from "@/types";

export const clusterKeys = {
  all: () => ["clusters"] as const,
  lists: () => [...clusterKeys.all(), "list"] as const,
  list: (f: Record<string, unknown>) => [...clusterKeys.lists(), f] as const,
};

async function fetchClusters(): Promise<ClustersResponse> {
  return invoke<ClustersResponse>("get_clusters", { blindspotsOnly: false });
}

export function useClusters() {
  return useQuery({
    queryKey: clusterKeys.list({}),
    queryFn: fetchClusters,
    staleTime: 1000 * 60 * 15,   // match refetchInterval so background refetches don't re-render
    refetchInterval: 1000 * 60 * 15,
  });
}

export function useSavedStories(enabled = true) {
  return useQuery({
    queryKey: [...clusterKeys.all(), "saved"],
    queryFn: () => invoke<ClustersResponse>("get_saved_stories"),
    enabled,
  });
}

export function usePublishers() {
  return useQuery({
    queryKey: ["publishers"],
    queryFn: () => invoke<Publisher[]>("get_publishers"),
    staleTime: Infinity,
  });
}

export async function refreshFeed(): Promise<RefreshResult> {
  return invoke<RefreshResult>("refresh_feed");
}

export async function searchStories(query: string): Promise<ClustersResponse> {
  return invoke<ClustersResponse>("search_stories", { query });
}

export async function saveStory(storyKey: string, articleIds: string[]): Promise<void> {
  return invoke<void>("save_story", { storyKey, articleIds });
}

export async function unsaveStory(storyKey: string): Promise<void> {
  return invoke<void>("unsave_story", { storyKey });
}

export async function getRefreshStatus(): Promise<RefreshStatus> {
  return invoke<RefreshStatus>("get_refresh_status");
}

export async function addCustomPublisher(url: string, name: string, isGlobal: boolean): Promise<Publisher> {
  return invoke<Publisher>("add_custom_publisher", { url, name, isGlobal });
}

export async function removeCustomPublisher(id: string): Promise<void> {
  return invoke<void>("remove_custom_publisher", { id });
}

export async function splitCluster(articleId: string, headline: string, publishedAt: string): Promise<string> {
  return invoke<string>("split_cluster", { articleId, headline, publishedAt });
}

export async function forceRecluster(): Promise<string> {
  return invoke<string>("force_recluster");
}

export async function wipeAllData(): Promise<void> {
  return invoke<void>("wipe_all_data");
}

export function useRefreshFeed() {
  return useQuery({
    queryKey: ["refresh"],
    queryFn: refreshFeed,
    enabled: false,
  });
}
