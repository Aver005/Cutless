// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Поля Cutless у клипа: лежат в `clip.extra["cutless"]`, переживают сохранение
//! и не трогают версию документа апстрима.

use serde_json::{Map, Value};

use crate::model::Clip;

/// Ключ объекта Cutless в `Clip::extra`.
const KEY: &str = "cutless";

/// Поле Cutless у клипа, если есть.
pub fn get<'a>(clip: &'a Clip, field: &str) -> Option<&'a Value> {
    clip.extra.get(KEY).and_then(|value| value.get(field))
}

/// Ставит поле. Возвращает true, если значение изменилось.
pub fn set(clip: &mut Clip, field: &str, value: Value) -> bool {
    if get(clip, field) == Some(&value) {
        return false;
    }
    let entry = clip
        .extra
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

/// Убирает поле и пустой объект Cutless. Возвращает true, если поле было.
pub fn remove(clip: &mut Clip, field: &str) -> bool {
    let Some(Value::Object(map)) = clip.extra.get_mut(KEY) else {
        return false;
    };
    let removed = map.remove(field).is_some();
    if map.is_empty() {
        clip.extra.remove(KEY);
    }
    removed
}
