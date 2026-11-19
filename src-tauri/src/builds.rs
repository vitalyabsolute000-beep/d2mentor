use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const CACHE_TTL: u64 = 12 * 3600;
const META_VERSION: u32 = 2;
const BUILD_VERSION: u32 = 2;
const ITEMS_URL: &str = "https://raw.githubusercontent.com/odota/dotaconstants/master/build/items.json";
const ITEM_IDS_URL: &str =
    "https://raw.githubusercontent.com/odota/dotaconstants/master/build/item_ids.json";
const HEROES_URL: &str = "https://api.opendota.com/api/heroes";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Component {
    pub key: String,
    pub cost: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ItemInfo {
    pub key: String,
    pub name: String,
    pub cost: i64,
    #[serde(default)]
    pub components: Vec<Component>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TimedItem {
    pub key: String,
    pub name: String,
    pub cost: i64,
    pub avg_time: f64,
    pub games: u64,
    pub winrate: f64,
    #[serde(default)]
    pub components: Vec<Component>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct HeroBuild {
    pub version: u32,
    pub hero_id: u32,
    pub hero_name: String,
    pub localized_name: String,
    pub start: Vec<ItemInfo>,
    pub early: Vec<ItemInfo>,
    pub mid: Vec<ItemInfo>,
    pub late: Vec<ItemInfo>,
    pub timings: Vec<TimedItem>,
    pub updated_at: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Dicts {
    #[serde(default)]
    pub version: u32,
    pub fetched_at: u64,
    pub items: HashMap<String, ItemInfo>,
    pub id_to_key: HashMap<String, String>,
    pub heroes: HashMap<String, HeroMeta>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct HeroMeta {
    pub name: String,
    pub localized_name: String,
}

pub fn data_dir() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(base).join("D2Mentor")
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn get_json(url: &str) -> Result<serde_json::Value, String> {
    let resp = ureq::get(url)
        .timeout(Duration::from_secs(25))
        .call()
        .map_err(|e| format!("{url}: {e}"))?;
    resp.into_json::<serde_json::Value>()
        .map_err(|e| format!("{url}: ответ не JSON: {e}"))
}

fn read_fresh<T: for<'de> Deserialize<'de>>(path: &std::path::Path, ttl: u64) -> Option<T> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    let age = SystemTime::now().duration_since(modified).ok()?.as_secs();
    if age > ttl {
        return None;
    }
    let data = std::fs::read(path).ok()?;
    serde_json::from_slice(&data).ok()
}

fn write_json<T: Serialize>(path: &std::path::Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let data = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    std::fs::write(path, data).map_err(|e| format!("{}: {e}", path.display()))
}

/// Справочники предметов и героев. Кэш на 12 часов.
pub fn dicts(force: bool) -> Result<Dicts, String> {
    let path = data_dir().join("meta.json");
    if !force {
        if let Some(d) = read_fresh::<Dicts>(&path, CACHE_TTL) {
            if d.version == META_VERSION && !d.items.is_empty() && !d.heroes.is_empty() {
                return Ok(d);
            }
        }
    }

    let mut d = Dicts {
        version: META_VERSION,
        fetched_at: now(),
        ..Default::default()
    };

    let items = get_json(ITEMS_URL)?;
    let mut raw_components: HashMap<String, Vec<String>> = HashMap::new();
    if let Some(obj) = items.as_object() {
        for (key, value) in obj {
            let name = value
                .get("dname")
                .and_then(|v| v.as_str())
                .unwrap_or(key)
                .to_string();
            let cost = value.get("cost").and_then(|v| v.as_i64()).unwrap_or(0);
            d.items.insert(
                key.clone(),
                ItemInfo {
                    key: key.clone(),
                    name,
                    cost,
                    components: Vec::new(),
                },
            );
            if let Some(arr) = value.get("components").and_then(|v| v.as_array()) {
                let keys: Vec<String> = arr
                    .iter()
                    .filter_map(|v| v.as_str())
                    .filter(|k| !k.starts_with("recipe"))
                    .map(|k| k.to_string())
                    .collect();
                if !keys.is_empty() {
                    raw_components.insert(key.clone(), keys);
                }
            }
        }
    }
    for (key, component_keys) in raw_components {
        let components: Vec<Component> = component_keys
            .iter()
            .filter_map(|k| d.items.get(k))
            .map(|info| Component {
                key: info.key.clone(),
                cost: info.cost,
            })
            .collect();
        if let Some(item) = d.items.get_mut(&key) {
            item.components = components;
        }
    }
    if d.items.is_empty() {
        return Err("Справочник предметов пуст".to_string());
    }

    let ids = get_json(ITEM_IDS_URL)?;
    if let Some(obj) = ids.as_object() {
        for (id, key) in obj {
            if let Some(key) = key.as_str() {
                d.id_to_key.insert(id.clone(), key.to_string());
            }
        }
    }

    let heroes = get_json(HEROES_URL)?;
    if let Some(arr) = heroes.as_array() {
        for hero in arr {
            let id = hero.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let name = hero
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let localized_name = hero
                .get("localized_name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            d.heroes.insert(
                id.to_string(),
                HeroMeta {
                    name,
                    localized_name,
                },
            );
        }
    }

    if let Err(e) = write_json(&path, &d) {
        eprintln!("не удалось сохранить справочники: {e}");
    }
    Ok(d)
}

pub fn get_build(hero_id: u32) -> Result<HeroBuild, String> {
    let path = data_dir().join("builds").join(format!("{hero_id}.json"));
    if let Some(b) = read_fresh::<HeroBuild>(&path, CACHE_TTL) {
        if b.version == BUILD_VERSION
            && b.hero_id == hero_id
            && (!b.early.is_empty() || !b.timings.is_empty())
        {
            return Ok(b);
        }
    }

    let dicts = dicts(false)?;
    let build = fetch_build(hero_id, &dicts)?;
    if let Err(e) = write_json(&path, &build) {
        eprintln!("не удалось сохранить билд: {e}");
    }
    Ok(build)
}

fn stage_list(
    popularity: &serde_json::Value,
    stage: &str,
    dicts: &Dicts,
    limit: usize,
) -> Vec<ItemInfo> {
    let Some(obj) = popularity.get(stage).and_then(|v| v.as_object()) else {
        return Vec::new();
    };

    let mut rows: Vec<(ItemInfo, u64)> = Vec::new();
    for (id, count) in obj {
        let count = count.as_u64().unwrap_or(0);
        let Some(key) = dicts.id_to_key.get(id) else {
            continue;
        };
        if key.starts_with("recipe") || key == "ability_base" {
            continue;
        }
        let Some(info) = dicts.items.get(key) else {
            continue;
        };
        rows.push((info.clone(), count));
    }

    rows.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.cost.cmp(&a.0.cost)));
    rows.into_iter().take(limit).map(|(i, _)| i).collect()
}

fn flex_u64(value: &serde_json::Value) -> u64 {
    if let Some(n) = value.as_u64() {
        return n;
    }
    value
        .as_str()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0)
}

fn fetch_build(hero_id: u32, dicts: &Dicts) -> Result<HeroBuild, String> {
    let hero = dicts
        .heroes
        .get(&hero_id.to_string())
        .cloned()
        .unwrap_or_default();

    let popularity = get_json(&format!(
        "https://api.opendota.com/api/heroes/{hero_id}/itemPopularity"
    ))?;

    let mut build = HeroBuild {
        version: BUILD_VERSION,
        hero_id,
        hero_name: hero.name,
        localized_name: if hero.localized_name.is_empty() {
            format!("Hero {hero_id}")
        } else {
            hero.localized_name
        },
        start: stage_list(&popularity, "start_game_items", dicts, 8),
        early: stage_list(&popularity, "early_game_items", dicts, 10),
        mid: stage_list(&popularity, "mid_game_items", dicts, 12),
        late: stage_list(&popularity, "late_game_items", dicts, 12),
        updated_at: now(),
        ..Default::default()
    };

    if let Ok(timings) = get_json(&format!(
        "https://api.opendota.com/api/scenarios/itemTimings?hero_id={hero_id}"
    )) {
        if let Some(rows) = timings.as_array() {
            let mut agg: HashMap<String, (u64, u64, f64)> = HashMap::new();
            for row in rows {
                let Some(key) = row.get("item").and_then(|v| v.as_str()) else {
                    continue;
                };
                if key.starts_with("recipe") {
                    continue;
                }
                let Some(info) = dicts.items.get(key) else {
                    continue;
                };
                let games = flex_u64(row.get("games").unwrap_or(&serde_json::Value::Null));
                let wins = flex_u64(row.get("wins").unwrap_or(&serde_json::Value::Null));
                let time = row
                    .get("time")
                    .and_then(|v| v.as_f64())
                    .or_else(|| row.get("time").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()))
                    .unwrap_or(0.0);
                if games == 0 {
                    continue;
                }
                let entry = agg
                    .entry(key.to_string())
                    .or_insert((0, 0, 0.0));
                entry.0 += games;
                entry.1 += wins;
                entry.2 += time * games as f64;
                let _ = info;
            }

            let mut list: Vec<TimedItem> = agg
                .into_iter()
                .filter_map(|(key, (games, wins, weighted))| {
                    let info = dicts.items.get(&key)?;
                    Some(TimedItem {
                        key,
                        name: info.name.clone(),
                        cost: info.cost,
                        avg_time: weighted / games as f64,
                        games,
                        winrate: wins as f64 / games as f64,
                        components: info.components.clone(),
                    })
                })
                .collect();

            // Отсекаем ситуативные предметы с крошечной выборкой.
            let max_games = list.iter().map(|t| t.games).max().unwrap_or(0);
            let threshold = ((max_games as f64) * 0.03).max(50.0) as u64;
            list.retain(|t| t.games >= threshold);

            list.sort_by(|a, b| a.avg_time.partial_cmp(&b.avg_time).unwrap_or(std::cmp::Ordering::Equal));
            list.truncate(30);
            build.timings = list;
        }
    }

    if build.early.is_empty() && build.mid.is_empty() && build.late.is_empty() && build.timings.is_empty() {
        return Err(format!("OpenDota не отдал данные для героя {hero_id}"));
    }

    Ok(build)
}
