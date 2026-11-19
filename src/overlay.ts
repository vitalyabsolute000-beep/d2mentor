import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { clock, gold, type Snapshot, type Status } from "./types";

const el = <T extends HTMLElement>(id: string): T =>
  document.getElementById(id) as T;

function render(snapshot: Snapshot) {
  const panel = el("panel");
  panel.classList.toggle("offline", !snapshot.connected);

  el("hero").textContent = snapshot.connected
    ? snapshot.hero || "Герой не выбран"
    : "Dota не запущена";

  el("clock").textContent = snapshot.connected ? clock(snapshot.gameTime) : "--:--";

  el("gold").textContent = snapshot.connected ? `${gold(snapshot.gold)} g` : "";
  el("nw").textContent = snapshot.connected
    ? `NW ${gold(snapshot.netWorth)}`
    : "";
  el("kda").textContent = snapshot.connected
    ? `${snapshot.kills}/${snapshot.deaths}/${snapshot.assists} · ${snapshot.gpm} GPM`
    : "";

  const next = snapshot.nextItem;
  const bar = el<HTMLDivElement>("next-bar");

  if (next && snapshot.connected) {
    el("next-name").textContent = next.name;
    const parts: string[] = [];
    if (next.goldNeeded > 0) {
      parts.push(`не хватает ${gold(next.goldNeeded)} g`);
    } else if (next.cost > 0) {
      parts.push("можно покупать");
    }
    if (next.etaSeconds && next.etaSeconds > 0) {
      parts.push(`ETA ${clock(next.etaSeconds)}`);
    }
    if (next.targetTime != null) {
      parts.push(`медиана ${clock(next.targetTime)}`);
    }
    el("next-meta").textContent = parts.join(" · ");
    const progress =
      next.cost > 0 ? Math.max(0, Math.min(1, snapshot.gold / next.cost)) : 0;
    bar.style.width = `${(progress * 100).toFixed(1)}%`;
  } else {
    el("next-name").textContent = "—";
    el("next-meta").textContent = snapshot.connected ? "" : "Нет данных";
    bar.style.width = "0%";
  }

  const advice = el<HTMLUListElement>("advice");
  advice.replaceChildren(
    ...snapshot.advice.slice(0, 4).map((line) => {
      const li = document.createElement("li");
      li.textContent = line;
      return li;
    }),
  );

  el("source").textContent = snapshot.connected
    ? snapshot.planSource || "OpenDota"
    : "GSI · OpenDota";
}

window.addEventListener("DOMContentLoaded", () => {
  listen<Snapshot>("coach://state", (event) => render(event.payload)).catch(
    (error) => console.error(error),
  );

  invoke<Status>("get_status")
    .then((status) => render(status.snapshot))
    .catch((error) => console.error(error));

  invoke<void>("overlay_ready").catch(() => {});
});
