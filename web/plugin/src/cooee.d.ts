// The `cooee` global cooee's plugin frame defines before the module runs (cooee docs/plugins.md).
declare global {
  const cooee: {
    fetch(
      path: string,
      init?: { method?: "GET" | "POST"; type?: string; body?: string },
    ): Promise<{ status: number; ok: boolean; type: string; text: string }>;
    events(path: string, on: (e: { event: string; data: string }) => void): () => void;
  };
}

export {};
