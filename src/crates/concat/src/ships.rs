// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! «Мой корабль»: сохранённый цвет, который ложится на клип одной кнопкой.
//!
//! Корабль — это имя и цепочка цветовых эффектов клипа. Корабли живут в
//! `ships.json` в папке данных приложения и пишутся атомарно. Здесь только
//! чтение, запись и арифметика над цепочкой — без окна и без облака.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use concat_effects::Catalogue;
use concat_effects::manifest::Kind;
use concat_host::AppDirs;
use concat_project::model::AppliedFilter;
use serde::{Deserialize, Serialize};

/// Имя файла кораблей в папке данных.
pub const FILE: &str = "ships.json";

/// Один сохранённый корабль: имя и цветовая цепочка.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ship {
    /// Уникальный и неизменный, чтобы чип знал свой корабль.
    pub id: String,
    /// Как корабль называется у пользователя.
    pub name: String,
    /// Когда сохранён, в миллисекундах эпохи.
    #[serde(default)]
    pub created: u64,
    /// Только цветовые эффекты, в порядке цепочки клипа.
    #[serde(default)]
    pub effects: Vec<AppliedFilter>,
}

/// Почему корабли не записались.
#[derive(Debug)]
pub enum ShipError {
    /// Цепочку не удалось превратить в JSON.
    Encode(serde_json::Error),
    /// Файл не удалось записать.
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl std::fmt::Display for ShipError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShipError::Encode(error) => write!(formatter, "could not encode the ships: {error}"),
            ShipError::Write { path, source } => {
                write!(formatter, "could not write {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for ShipError {}

/// Где лежит файл кораблей.
pub fn path(dirs: &AppDirs) -> PathBuf {
    dirs.data.join(FILE)
}

/// Читает корабли. Нет файла или он битый — пустой список, не ошибка.
pub fn load(dirs: &AppDirs) -> Vec<Ship> {
    std::fs::read(path(dirs))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Пишет корабли во временный файл и переименовывает его на место: save
/// именем, чтобы падение посередине не оставило обрезанный файл.
pub fn save(dirs: &AppDirs, ships: &[Ship]) -> Result<(), ShipError> {
    let target = path(dirs);
    let temporary = target.with_extension("json.saving");
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|source| ShipError::Write {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let encoded = serde_json::to_vec_pretty(ships).map_err(ShipError::Encode)?;
    std::fs::write(&temporary, &encoded).map_err(|source| ShipError::Write {
        path: temporary.clone(),
        source,
    })?;
    std::fs::rename(&temporary, &target).map_err(|source| ShipError::Write {
        path: target.clone(),
        source,
    })
}

/// Миллисекунды с эпохи; на часах до эпохи — ноль.
fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// Свободный id: время создания плюс счётчик, если такой уже есть.
fn fresh_id(ships: &[Ship], created: u64) -> String {
    let base = format!("ship-{created}");
    if !ships.iter().any(|ship| ship.id == base) {
        return base;
    }
    (1..)
        .map(|n| format!("{base}-{n}"))
        .find(|candidate| !ships.iter().any(|ship| &ship.id == candidate))
        .unwrap_or_else(|| base.clone())
}

/// Какой по счёту корабль предложить по умолчанию, считая и удалённые.
pub fn next_number(ships: &[Ship]) -> usize {
    ships.len() + 1
}

/// Добавляет корабль и отдаёт его id. Имя подрезается; пустое становится
/// «Корабль».
pub fn add(ships: &mut Vec<Ship>, name: &str, effects: Vec<AppliedFilter>) -> String {
    let created = now_millis();
    let id = fresh_id(ships, created);
    let name = match name.trim() {
        "" => "Ship".to_owned(),
        trimmed => trimmed.to_owned(),
    };
    ships.push(Ship {
        id: id.clone(),
        name,
        created,
        effects,
    });
    id
}

/// Переименовывает корабль. Пустое имя оставляет прежнее. True, если имя
/// изменилось.
pub fn rename(ships: &mut [Ship], id: &str, name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return false;
    }
    let Some(ship) = ships.iter_mut().find(|ship| ship.id == id) else {
        return false;
    };
    if ship.name == trimmed {
        return false;
    }
    ship.name = trimmed.to_owned();
    true
}

/// Убирает корабль. True, если он был.
pub fn remove(ships: &mut Vec<Ship>, id: &str) -> bool {
    let before = ships.len();
    ships.retain(|ship| ship.id != id);
    ships.len() != before
}

/// Цветовой ли пакет: фильтр любого вида, либо эффект из марок Color и
/// Basic. Незнакомый пакет цветом не считается — его лучше сохранить.
pub fn is_colour(link: &AppliedFilter) -> bool {
    let catalogue = Catalogue::builtin();
    catalogue
        .packages()
        .find(|package| package.answers_to(&link.id))
        .is_some_and(|package| match package.kind() {
            Kind::Filter => true,
            Kind::Effect => {
                let shelf = package.category();
                shelf.eq_ignore_ascii_case("Color") || shelf.eq_ignore_ascii_case("Basic")
            }
            _ => false,
        })
}

/// Только цветовые звенья цепочки, по порядку.
pub fn colour_chain(chain: &[AppliedFilter]) -> Vec<AppliedFilter> {
    chain
        .iter()
        .filter(|link| is_colour(link))
        .cloned()
        .collect()
}

/// Цепочка с цветом, заменённым на корабль: прежний цвет уходит, корабль
/// встаёт в начало, остальные звенья сохраняют свой порядок.
pub fn with_ship(chain: &[AppliedFilter], ship: &Ship) -> Vec<AppliedFilter> {
    let mut out = ship.effects.clone();
    out.extend(chain.iter().filter(|link| !is_colour(link)).cloned());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(id: &str) -> AppliedFilter {
        AppliedFilter::new(id)
    }

    fn ship(name: &str, effects: Vec<AppliedFilter>) -> Ship {
        Ship {
            id: name.to_owned(),
            name: name.to_owned(),
            created: 0,
            effects,
        }
    }

    /// Корабли переживают запись и чтение, и файл лежит там, где сказано.
    #[test]
    fn ships_round_trip_through_the_file() {
        let root = std::env::temp_dir().join(format!("cutless-ships-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dirs = AppDirs::under(&root);
        let mut ships = Vec::new();
        add(&mut ships, "Утро", vec![link("concat.warm")]);
        save(&dirs, &ships).expect("saves");
        assert!(path(&dirs).is_file());
        assert_eq!(load(&dirs), ships);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Нет файла — пустой список, а не ошибка.
    #[test]
    fn no_file_is_no_ships() {
        let root = std::env::temp_dir().join(format!("cutless-ships-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        assert!(load(&AppDirs::under(&root)).is_empty());
    }

    /// Добавление даёт id, переименование меняет имя, удаление убирает.
    #[test]
    fn ships_are_added_renamed_and_removed() {
        let mut ships = Vec::new();
        let id = add(&mut ships, "  Первый  ", Vec::new());
        assert_eq!(ships[0].name, "Первый");
        assert_eq!(ships[0].id, id);
        assert!(!add(&mut ships, "Первый", Vec::new()).is_empty());
        assert_ne!(ships[0].id, ships[1].id, "ids stay unique");
        assert_eq!(next_number(&ships), 3);

        assert!(rename(&mut ships, &id, "Утро"));
        assert_eq!(ships[0].name, "Утро");
        assert!(
            !rename(&mut ships, &id, "  "),
            "a blank name changes nothing"
        );
        assert!(!rename(&mut ships, "missing", "X"));

        assert!(remove(&mut ships, &id));
        assert_eq!(ships.len(), 1);
        assert!(!remove(&mut ships, &id));
    }

    /// Цвет — фильтры любого вида и эффекты из марок Color и Basic; геометрия,
    /// звук и незнакомые пакеты — нет.
    #[test]
    fn colour_is_filters_and_the_colour_and_basic_shelves() {
        assert!(is_colour(&link("concat.adjust")), "a filter");
        assert!(is_colour(&link("concat.warm")), "a colour filter");
        assert!(is_colour(&link("concat.saturation")), "a Color effect");
        assert!(is_colour(&link("concat.sharpen")), "a Basic effect");
        assert!(
            !is_colour(&link("concat.vignette")),
            "Stylize is not colour"
        );
        assert!(
            !is_colour(&link("concat.gaussian-blur")),
            "Blur is not colour"
        );
        assert!(!is_colour(&link("cutless.unknown")), "unknown is kept");
    }

    /// Из цепочки берутся только цветовые звенья, по порядку.
    #[test]
    fn a_ships_chain_keeps_only_colour() {
        let chain = vec![
            link("concat.saturation"),
            link("concat.vignette"),
            link("concat.warm"),
            link("concat.sharpen"),
            link("concat.gaussian-blur"),
        ];
        let colour: Vec<String> = colour_chain(&chain)
            .into_iter()
            .map(|link| link.id)
            .collect();
        assert_eq!(
            colour,
            ["concat.saturation", "concat.warm", "concat.sharpen"]
        );
    }

    /// Корабль заменяет цвет клипа и не трогает остальные звенья.
    #[test]
    fn applying_a_ship_replaces_the_colour_and_keeps_the_rest() {
        let chain = vec![
            link("concat.vignette"),
            link("concat.saturation"),
            link("concat.gaussian-blur"),
            link("concat.warm"),
        ];
        let saved = ship("Утро", vec![link("concat.warm"), link("concat.saturation")]);
        let out: Vec<String> = with_ship(&chain, &saved)
            .into_iter()
            .map(|link| link.id)
            .collect();
        assert_eq!(
            out,
            [
                "concat.warm",
                "concat.saturation",
                "concat.vignette",
                "concat.gaussian-blur",
            ]
        );
    }

    /// У клипа без цвета корабль просто встаёт в начало.
    #[test]
    fn a_colourless_clip_gains_the_ship_at_the_head() {
        let chain = vec![link("concat.gaussian-blur")];
        let saved = ship("Утро", vec![link("concat.warm")]);
        let out: Vec<String> = with_ship(&chain, &saved)
            .into_iter()
            .map(|link| link.id)
            .collect();
        assert_eq!(out, ["concat.warm", "concat.gaussian-blur"]);
    }
}
