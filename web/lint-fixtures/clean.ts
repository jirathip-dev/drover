export const modes = ["on", "off"] as const;
export type Mode = (typeof modes)[number];
export const pick = (m: Mode, x?: number): number => x ?? (m === "on" ? 1 : 0);
export const ok = (s: string): boolean => s !== "" && !s.startsWith("x");
