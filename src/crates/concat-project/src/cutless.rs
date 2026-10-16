// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Поля Cutless у клипа и медиа: лежат в `extra["cutless"]`, переживают сохранение
//! и не трогают версию документа апстрима.

use serde_json::{Map, Value};

use crate::model::Clip;

/// Ключ объекта Cutless в `extra`.
const KEY: &str = "cutless";

/// Поле Cutless у клипа, если есть.
pub fn get<'a>(clip: &'a Clip, field: &str) -> Option<&'a Value> {
    get_in(&clip.extra, field)
}

/// Ставит поле. Возвращает true, если значение изменилось.
pub fn set(clip: &mut Clip, field: &str, value: Value) -> bool {
    set_in(&mut clip.extra, field, value)
}

/// Убирает поле и пустой объект Cutless. Возвращает true, если поле было.
pub fn remove(clip: &mut Clip, field: &str) -> bool {
    remove_in(&mut clip.extra, field)
}

/// Поле Cutless в любой карте `extra` (клипа, медиа).
pub fn get_in<'a>(extra: &'a Map<String, Value>, field: &str) -> Option<&'a Value> {
    extra.get(KEY).and_then(|value| value.get(field))
}

/// Ставит поле Cutless в карте `extra`. Возвращает true, если значение изменилось.
pub fn set_in(extra: &mut Map<String, Value>, field: &str, value: Value) -> bool {
    if get_in(extra, field) == Some(&value) {
        return false;
    }
    let entry = extra
        .entry(KEY.to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    if let Value::Object(map) = entry {
        map.insert(field.to_owned(), value);
    }
    true
}

/// Убирает поле Cutless и пустой объект из карты `extra`. Возвращает true, если поле было.
pub fn remove_in(extra: &mut Map<String, Value>, field: &str) -> bool {
    let Some(Value::Object(map)) = extra.get_mut(KEY) else {
        return false;
    };
    let removed = map.remove(field).is_some();
    if map.is_empty() {
        extra.remove(KEY);
    }
    removed
}
