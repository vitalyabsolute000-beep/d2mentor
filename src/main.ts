import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { clock, gold, type Snapshot, type Status } from "./types";

const el = <T extends HTMLElement>(id: string): T =>
  document.getElementById(id) as T;

const logEl = el<HTMLPreElement>("log");
const logLines: string[] = [];

function log(message: string) {
  const time = new Date().toLocaleTimeString("ru-RU", { hour12: false });
  logLines.push(`[${time}] ${message}`);
  if (logLines.length > 200) logLines.shift();
  logEl.textContent = logLines.join("\n");
  logEl.scrollTop = logEl.scrollHeight;
}

function setBadge(element: HTMLElement, kind: "ok" | "warn" | "bad") {
  element.dataset.state = kind;
}

function renderStatus(status: Status) {
  const dot = el("brand-dot");
  setBadge(dot, status.serverOk ? "ok" : "bad");

  const serverValue = el("server-value");
  serverValue.textContent = status.serverOk
    ? `127.0.0.1:${status.port}`
    : "Не запущен";
  el("server-hint").textContent = status.serverOk
    ? `Токен ${status.tokenPrefix}…`
    : status.serverError ?? "";

  el("dota-value").textContent = status.dotaFound ? "Найдена" : "Не найдена";
  el("cfg-hint").textContent = status.dotaFound
    ? status.cfgInstalled
      ? `Конфиг установлен: ${status.cfgPath ?? ""}`
      : "GSI-конфиг ещё не установлен"
    : "Укажи папку Steam вручную (пока не реализовано)";

  const snapshot = status.snapshot;
  el("conn-value").textContent = snapshot.connected
    ? "Данные идут"
    : "Нет данных";
  el("conn-hint").textContent = snapshot.connected
    ? `${snapshot.phase === "game" ? "Матч" : snapshot.phase} · ${snapshot.hero || "герой не выбран"}`
    : "Запусти Dota 2 и зайди в матч";

  renderSnapshot(snapshot);
}

function renderSnapshot(snapshot: Snapshot) {
  el("live-hero").textContent = snapshot.hero || "Ожидание данных";
  el("live-clock").textContent = snapshot.connected
    ? clock(snapshot.gameTime)
    : "--:--";

  el("live-gold").textContent = snapshot.connected
    ? `Золото ${gold(snapshot.gold)}`
    : "";
  el("live-nw").textContent = snapshot.connected
    ? `Нетворс ${gold(snapshot.netWorth)}`
    : "";
  el("live-kda").textContent = snapshot.connected
    ? `${snapshot.kills}/${snapshot.deaths}/${snapshot.assists} · ${snapshot.gpm} GPM`
    : "";

  const next = snapshot.nextItem;
  const nextName = el("next-name");
  const nextMeta = el("next-meta");
  const bar = el<HTMLDivElement>("next-bar");

  if (next) {
    nextName.textContent = next.name;
    const parts: string[] = [];
    if (next.cost > 0) {
      parts.push(`${gold(next.cost)} g`);
      if (next.goldNeeded > 0) {
        parts.push(`не хватает ${gold(next.goldNeeded)}`);
      } else {
        parts.push("можно покупать");
      }
    }
    if (next.etaSeconds && next.etaSeconds > 0) {
      parts.push(`ETA ${clock(next.etaSeconds)}`);
    }
    if (next.targetTime != null) {
      parts.push(`медиана ${clock(next.targetTime)}`);
    }
    nextMeta.textContent = parts.join(" · ");
    const progress =
      next.cost > 0 ? Math.max(0, Math.min(1, snapshot.gold / next.cost)) : 0;
    bar.style.width = `${(progress * 100).toFixed(1)}%`;
  } else {
    nextName.textContent = snapshot.connected ? "План пуст" : "—";
    nextMeta.textContent = snapshot.connected ? "" : "Нет данных из игры";
    bar.style.width = "0%";
  }

  const advice = el<HTMLUListElement>("live-advice");
  advice.replaceChildren(
    ...snapshot.advice.map((line) => {
      const li = document.createElement("li");
      li.textContent = line;
      return li;
    }),
  );
}

async function refresh() {
  try {
    const status = await invoke<Status>("get_status");
    renderStatus(status);
  } catch (error) {
    log(`get_status: ${String(error)}`);
  }
}

async function action<T>(label: string, run: () => Promise<T>) {
  try {
    const result = await run();
    if (typeof result === "string" && result.length > 0) {
      log(`${label}: ${result}`);
    } else {
      log(`${label}: готово`);
    }
    await refresh();
  } catch (error) {
    log(`${label}: ${String(error)}`);
  }
}

window.addEventListener("DOMContentLoaded", () => {
  el("btn-install").addEventListener("click", () =>
    action("Установка GSI-конфига", () => invoke<string>("install_gsi")),
  );

  el("btn-remove").addEventListener("click", () =>
    action("Удаление GSI-конфига", () => invoke<void>("remove_gsi")),
  );

  el("btn-overlay").addEventListener("click", () =>
    action("Оверлей", () => invoke<void>("toggle_overlay_cmd")),
  );

  el("btn-sim").addEventListener("click", () =>
    action("Тест без Dota", async () => {
      const snapshot = await invoke<Snapshot>("simulate");
      renderSnapshot(snapshot);
      return "фейковый матч отправлен в оверлей";
    }),
  );

  el("btn-data").addEventListener("click", () =>
    action("Папка данных", () => invoke<void>("open_data_dir")),
  );

  listen<Snapshot>("coach://state", (event) => {
    renderSnapshot(event.payload);
  }).catch((error) => log(`listen: ${String(error)}`));

  refresh();
  setInterval(refresh, 3000);
  log("D2Mentor запущен");
});
