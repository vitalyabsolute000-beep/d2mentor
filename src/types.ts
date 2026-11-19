export interface NextItem {
  key: string;
  name: string;
  cost: number;
  goldNeeded: number;
  targetTime: number | null;
  etaSeconds: number | null;
  games: number;
  winrate: number;
}

export interface Snapshot {
  connected: boolean;
  phase: string;
  hero: string;
  heroId: number | null;
  gameTime: number;
  gold: number;
  netWorth: number;
  gpm: number;
  kills: number;
  deaths: number;
  assists: number;
  level: number;
  nextItem: NextItem | null;
  upcoming: NextItem[];
  owned: string[];
  startItems: string[];
  planSource: string;
  advice: string[];
  updatedMs: number;
}

export interface Status {
  dotaFound: boolean;
  cfgPath: string | null;
  cfgInstalled: boolean;
  port: number;
  tokenPrefix: string;
  dataDir: string;
  serverOk: boolean;
  serverError: string | null;
  snapshot: Snapshot;
}

export function clock(seconds: number): string {
  const sign = seconds < 0 ? "-" : "";
  const abs = Math.abs(Math.round(seconds));
  const m = Math.floor(abs / 60);
  const s = abs % 60;
  return `${sign}${m}:${String(s).padStart(2, "0")}`;
}

export function gold(value: number): string {
  return value.toLocaleString("ru-RU");
}
