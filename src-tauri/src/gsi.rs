use crate::builds::{self, HeroBuild, ItemInfo};
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

pub const EVENT: &str = "coach://state";

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct NextItem {
    pub key: String,
    pub name: String,
    pub cost: i64,
    pub component_value: i64,
    pub gold_needed: i64,
    pub target_time: Option<i64>,
    pub eta_seconds: Option<i64>,
    pub games: u64,
    pub winrate: f64,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlanRow {
    pub key: String,
    pub name: String,
    pub cost: i64,
    pub target_time: Option<i64>,
    pub owned: bool,
    pub next: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub connected: bool,
    pub phase: String,
    pub hero: String,
    pub hero_id: Option<u32>,
    pub game_time: i64,
    pub gold: i64,
    pub net_worth: i64,
    pub gpm: i64,
    pub kills: i64,
    pub deaths: i64,
    pub assists: i64,
    pub level: i64,
    pub next_item: Option<NextItem>,
    pub upcoming: Vec<NextItem>,
    pub plan: Vec<PlanRow>,
    pub owned: Vec<String>,
    pub start_items: Vec<String>,
    pub plan_source: String,
    pub advice: Vec<String>,
    pub updated_ms: u64,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            connected: false,
            phase: "idle".to_string(),
            hero: String::new(),
            hero_id: None,
            game_time: 0,
            gold: 0,
            net_worth: 0,
            gpm: 0,
            kills: 0,
            deaths: 0,
            assists: 0,
            level: 0,
            next_item: None,
            upcoming: Vec::new(),
            plan: Vec::new(),
            owned: Vec::new(),
            start_items: Vec::new(),
            plan_source: String::new(),
            advice: Vec::new(),
            updated_ms: 0,
        }
    }
}

pub struct GsiHandle {
    pub port: u16,
    pub token: String,
    pub snapshot: Arc<Mutex<Snapshot>>,
    pub builds: Arc<Mutex<HashMap<u32, HeroBuild>>>,
}

impl GsiHandle {
    fn build_for(&self, hero_id: u32) -> Option<HeroBuild> {
        if let Some(b) = self.builds.lock().ok()?.get(&hero_id) {
            return Some(b.clone());
        }
        match builds::get_build(hero_id) {
            Ok(b) => {
                if let Ok(mut map) = self.builds.lock() {
                    map.insert(hero_id, b.clone());
                }
                Some(b)
            }
            Err(e) => {
                eprintln!("[builds] {e}");
                None
            }
        }
    }

    /// Принимает тело GSI-запроса, обновляет состояние и рассылает его в окна.
    pub fn ingest(&self, app: &AppHandle, body: &str) -> Result<Snapshot, String> {
        if body.len() > 4 * 1024 * 1024 {
            return Err("Тело запроса слишком большое".to_string());
        }
        let value: Value = serde_json::from_str(body).map_err(|e| format!("Некорректный JSON: {e}"))?;

        let token = value
            .pointer("/auth/token")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if token != self.token {
            return Err("Неверный токен GSI".to_string());
        }

        let hero_id = value
            .pointer("/hero/id")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);

        let build = hero_id.and_then(|id| self.build_for(id));
        let snapshot = build_snapshot(&value, build.as_ref());

        if let Ok(mut guard) = self.snapshot.lock() {
            *guard = snapshot.clone();
        }
        let _ = app.emit(EVENT, &snapshot);
        Ok(snapshot)
    }
}

pub fn start(app: AppHandle, token: String, port: u16) -> Result<GsiHandle, String> {
    let server = tiny_http::Server::http(("127.0.0.1", port))
        .map_err(|e| format!("Не удалось занять порт {port}: {e}"))?;

    let snapshot = Arc::new(Mutex::new(Snapshot::default()));
    let builds: Arc<Mutex<HashMap<u32, HeroBuild>>> = Arc::new(Mutex::new(HashMap::new()));

    let handle = GsiHandle {
        port,
        token,
        snapshot: snapshot.clone(),
        builds,
    };

    let server_handle = GsiHandle {
        port: handle.port,
        token: handle.token.clone(),
        snapshot: handle.snapshot.clone(),
        builds: handle.builds.clone(),
    };

    let app_for_server = app.clone();
    thread::spawn(move || {
        for request in server.incoming_requests() {
            let mut request = request;
            let mut body = String::new();
            let _ = request.as_reader().take(4 * 1024 * 1024).read_to_string(&mut body);

            let url = request.url().to_string();
            let response = if request.method() == &tiny_http::Method::Post && url.starts_with("/gsi") {
                match server_handle.ingest(&app_for_server, &body) {
                    Ok(_) => tiny_http::Response::from_string("ok"),
                    Err(e) => {
                        eprintln!("[gsi] {e}");
                        tiny_http::Response::from_string(e).with_status_code(400)
                    }
                }
            } else if url == "/health" {
                tiny_http::Response::from_string("D2Mentor GSI")
            } else if url == "/state" {
                match server_handle.snapshot.lock() {
                    Ok(snap) => {
                        let body = serde_json::to_string(&*snap).unwrap_or_else(|_| "{}".to_string());
                        tiny_http::Response::from_string(body)
                    }
                    Err(_) => tiny_http::Response::from_string("{}").with_status_code(500),
                }
            } else {
                tiny_http::Response::from_string("not found").with_status_code(404)
            };
            let _ = request.respond(response);
        }
    });

    // Если Dota перестала присылать данные — показываем "нет соединения".
    let watchdog_app = app.clone();
    let watchdog_snapshot = snapshot.clone();
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(5));
        let mut changed = false;
        if let Ok(mut snap) = watchdog_snapshot.lock() {
            if snap.connected {
                let now = now_ms();
                if now.saturating_sub(snap.updated_ms) > 15_000 {
                    snap.connected = false;
                    snap.phase = "idle".to_string();
                    snap.next_item = None;
                    snap.upcoming.clear();
                    snap.plan.clear();
                    snap.advice = vec!["Dota 2 не присылает данные. Запусти игру или перезайди в матч.".to_string()];
                    changed = true;
                }
            }
        }
        if changed {
            if let Ok(snap) = watchdog_snapshot.lock() {
                let _ = watchdog_app.emit(EVENT, &*snap);
            }
        }
    });

    Ok(handle)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn clock(seconds: i64) -> String {
    let sign = if seconds < 0 { "-" } else { "" };
    let s = seconds.abs();
    format!("{sign}{}:{:02}", s / 60, s % 60)
}

fn item_key(raw: &str) -> Option<String> {
    let key = raw.strip_prefix("item_").unwrap_or(raw);
    if key.is_empty() || key == "empty" || key.starts_with("recipe") {
        return None;
    }
    Some(key.to_string())
}

fn plan(build: &HeroBuild) -> Vec<ItemInfo> {
    let mut out: Vec<ItemInfo> = build
        .timings
        .iter()
        .map(|t| ItemInfo {
            key: t.key.clone(),
            name: t.name.clone(),
            cost: t.cost,
            components: t.components.clone(),
        })
        .collect();

    // Если таймингов мало — добиваем стадиями, но только готовыми предметами,
    // чтобы в плане не мелькали отдельные компоненты.
    for list in [&build.early, &build.mid, &build.late] {
        for item in list {
            if item.cost < 1000 {
                continue;
            }
            if out.iter().any(|i| i.key == item.key) {
                continue;
            }
            out.push(item.clone());
        }
    }
    out
}

fn component_credit(item: &ItemInfo, owned: &HashSet<&str>) -> i64 {
    item.components
        .iter()
        .filter(|c| owned.contains(c.key.as_str()))
        .map(|c| c.cost)
        .sum()
}

fn build_snapshot(value: &Value, build: Option<&HeroBuild>) -> Snapshot {
    let map = value.get("map");
    let game_state = map
        .and_then(|m| m.get("game_state"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let phase = if game_state.contains("HERO_SELECTION") {
        "pick"
    } else if game_state.contains("STRATEGY_TIME") || game_state.contains("PRE_GAME") {
        "pregame"
    } else if game_state.contains("GAME_IN_PROGRESS") {
        "game"
    } else if game_state.contains("POST_GAME") {
        "post"
    } else if map.is_some() {
        "other"
    } else {
        "idle"
    }
    .to_string();

    let game_time = map
        .and_then(|m| m.get("clock_time").or_else(|| m.get("game_time")))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    let hero_id = value
        .get("hero")
        .and_then(|h| h.get("id"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    let player = value.get("player");
    let gold = player
        .and_then(|p| p.get("gold"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let net_worth = player
        .and_then(|p| p.get("net_worth"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let gpm = player
        .and_then(|p| p.get("gpm"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let kills = player
        .and_then(|p| p.get("kill_count"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let deaths = player
        .and_then(|p| p.get("death_count"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let assists = player
        .and_then(|p| p.get("assist_count"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    let level = value
        .get("hero")
        .and_then(|h| h.get("level"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    let mut owned: Vec<String> = Vec::new();
    if let Some(items) = value.get("items").and_then(|v| v.as_object()) {
        for (_, raw) in items {
            if let Some(raw) = raw.as_str() {
                if let Some(key) = item_key(raw) {
                    if !owned.contains(&key) {
                        owned.push(key);
                    }
                }
            }
        }
    }
    let owned_set: HashSet<&str> = owned.iter().map(|s| s.as_str()).collect();

    let mut snapshot = Snapshot {
        connected: true,
        phase: phase.clone(),
        hero: build.map(|b| b.localized_name.clone()).unwrap_or_default(),
        hero_id,
        game_time,
        gold,
        net_worth,
        gpm,
        kills,
        deaths,
        assists,
        level,
        owned: owned.clone(),
        updated_ms: now_ms(),
        ..Default::default()
    };

    let Some(build) = build else {
        snapshot.advice = vec![if hero_id.is_some() {
            "Не удалось загрузить билд героя: нет сети или OpenDota недоступна.".to_string()
        } else {
            "Ожидаю выбор героя...".to_string()
        }];
        return snapshot;
    };

    snapshot.start_items = build.start.iter().map(|i| i.name.clone()).collect();
    snapshot.plan_source = if build.timings.is_empty() {
        "OpenDota: популярность предметов".to_string()
    } else {
        "OpenDota: тайминги покупок".to_string()
    };

    let order = plan(build);
    let timing: HashMap<&str, f64> = build
        .timings
        .iter()
        .map(|t| (t.key.as_str(), t.avg_time))
        .collect();
    let samples: HashMap<&str, (u64, f64)> = build
        .timings
        .iter()
        .map(|t| (t.key.as_str(), (t.games, t.winrate)))
        .collect();

    let mut upcoming: Vec<NextItem> = Vec::new();
    for item in &order {
        if owned_set.contains(item.key.as_str()) {
            continue;
        }
        let target = timing.get(item.key.as_str()).map(|t| *t as i64);
        let (games, winrate) = samples
            .get(item.key.as_str())
            .copied()
            .unwrap_or((0, 0.0));
        let component_value = component_credit(item, &owned_set);
        let remaining_cost = (item.cost - component_value).max(0);
        let gold_needed = (remaining_cost - gold).max(0);
        let eta_seconds = if item.cost > 0 && gold_needed > 0 && gpm > 0 {
            Some((gold_needed as f64 * 60.0 / gpm as f64).round() as i64)
        } else if gold_needed == 0 {
            Some(0)
        } else {
            None
        };
        upcoming.push(NextItem {
            key: item.key.clone(),
            name: item.name.clone(),
            cost: item.cost,
            component_value,
            gold_needed,
            target_time: target,
            eta_seconds,
            games,
            winrate,
        });
        if upcoming.len() >= 4 {
            break;
        }
    }

    snapshot.next_item = upcoming.first().cloned();
    snapshot.upcoming = upcoming;

    let next_key = snapshot.next_item.as_ref().map(|n| n.key.clone());
    snapshot.plan = order
        .iter()
        .take(8)
        .map(|item| PlanRow {
            key: item.key.clone(),
            name: item.name.clone(),
            cost: item.cost,
            target_time: timing.get(item.key.as_str()).map(|t| *t as i64),
            owned: owned_set.contains(item.key.as_str()),
            next: next_key.as_deref() == Some(item.key.as_str()),
        })
        .collect();

    let mut advice: Vec<String> = Vec::new();
    match phase.as_str() {
        "pick" => {
            if !snapshot.start_items.is_empty() {
                advice.push(format!("Идёт пик. Стартовый закуп: {}.", snapshot.start_items.join(", ")));
            } else {
                advice.push("Идёт пик героев.".to_string());
            }
        }
        "game" => {
            if let Some(next) = snapshot.next_item.clone() {
                let mut line = if next.cost > 0 {
                    let parts = if next.component_value > 0 {
                        format!(
                            "Дальше: {} — {} золота (собрано на {}, есть {}, не хватает {}).",
                            next.name, next.cost, next.component_value, gold, next.gold_needed
                        )
                    } else {
                        format!(
                            "Дальше: {} — {} золота (есть {}, не хватает {}).",
                            next.name, next.cost, gold, next.gold_needed
                        )
                    };
                    parts
                } else {
                    format!("Дальше: {}.", next.name)
                };
                if let Some(eta) = next.eta_seconds {
                    if eta > 0 {
                        line.push_str(&format!(" Хватит примерно через {}.", clock(eta)));
                    } else if next.gold_needed == 0 && next.cost > 0 {
                        line.push_str(" Золота уже хватает — закупайся.");
                    }
                }
                advice.push(line);

                if let Some(target) = next.target_time {
                    let diff = game_time - target;
                    if diff > 120 {
                        advice.push(format!(
                            "Ты позади среднего тайминга на {} (обычно к {}).",
                            clock(diff),
                            clock(target)
                        ));
                    } else if diff < -120 && game_time > 0 {
                        advice.push(format!(
                            "По темпу опережаешь статистику: обычно к {}, у тебя есть {}.",
                            clock(target),
                            clock(-diff)
                        ));
                    }
                }
                if next.games >= 30 {
                    advice.push(format!(
                        "С этим предметом винрейт {:.0}% ({} игр по OpenDota).",
                        next.winrate * 100.0,
                        next.games
                    ));
                }
            } else {
                advice.push("Все предметы из плана уже куплены. Доигрывай по ситуации.".to_string());
            }

            if deaths >= 5 && game_time > 600 {
                advice.push(format!(
                    "Смертей уже {} — играй от вардов и не ходи в одиночку по чужой части карты.",
                    deaths
                ));
            }
            if game_time > 0 && game_time < 600 && !owned_set.contains("tango") {
                advice.push("Не забудь реген на линии: танго/флакон.".to_string());
            }
        }
        "post" => advice.push("Матч закончился.".to_string()),
        _ => advice.push("Ожидаю начала матча...".to_string()),
    }
    advice.truncate(4);
    snapshot.advice = advice;

    snapshot
}

/// Фейковый пакет GSI для проверки без Dota 2.
pub fn simulate_body(hero_id: u32, game_time: i64, gold: i64, gpm: i64) -> String {
    format!(
        r#"{{
  "auth": {{ "token": "__TOKEN__" }},
  "provider": {{ "name": "Dota 2", "appid": 570, "version": 41, "timestamp": 0 }},
  "map": {{ "name": "start", "matchid": "0", "game_time": {game_time}, "clock_time": {game_time}, "game_state": "DOTA_GAMERULES_STATE_GAME_IN_PROGRESS" }},
  "player": {{ "steamid": "0", "name": "test", "activity": "playing", "kills": 3, "deaths": 1, "assists": 2, "kill_count": 3, "death_count": 1, "assist_count": 2, "gold": {gold}, "gold_reliable": 0, "gold_unreliable": {gold}, "gpm": {gpm}, "xpm": 500, "net_worth": 8000, "last_hits": 120, "denies": 5 }},
  "hero": {{ "id": {hero_id}, "name": "npc_dota_hero_antimage", "level": 11, "alive": true, "buyback_cost": 0, "buyback_cooldown": 0 }},
  "items": {{ "item0": "item_tango", "item1": "item_power_treads", "item2": "item_battlefury", "item3": "item_empty", "item4": "item_empty", "item5": "item_empty", "item_neutral": "item_empty" }}
}}"#
    )
}
