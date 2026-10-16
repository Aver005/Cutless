# Задача: переключатель «Агенты могут править открытый проект» в настройках окна

Страница Remote в настройках (`src/crates/concat/ui/dialogs/settings.slint`, логика — `src/crates/concat/src/panes/settings.rs`,
настройки — `src/crates/concat/src/prefs.rs`, `ServerPrefs`) включает встроенный сервер API. Владелец решил: отдельный
переключатель для агентов, **выключен по умолчанию**; включение заодно включает сервер на 127.0.0.1 с токеном.

## Что сделать
1. `ServerPrefs`: поле `agents: bool` (по умолчанию false; `#[serde(default)]` уже на структуре). Doc-комментарий
   по-английски, как у соседей.
2. В `settings.slint` на странице Remote, над блоком сервера: переключатель с подписью и пояснением (стиль соседних
   переключателей этой страницы). Свойство `agents-enabled: bool` в данных страницы, колбэк `agents-changed(bool)`.
3. В `settings.rs`: сообщение для колбэка (по образцу `server-enabled-changed`). Включение: `agents = true`,
   `server.enabled = true`, если `server.token` пуст — `new_token()`; если `listen` не loopback-адрес — вернуть
   `listen` к `DEFAULT_LISTEN`. Затем сохранить настройки и `apply_server` (как делает переключатель сервера).
   Выключение: `agents = false`, сервер не трогать. Выключение самого сервера выключает и `agents`.
   Привязку окна к API (`link_window`) делает интегратор — внутрь замыкания в `apply_server` не лезь.
4. Строки: ключи `settings.agentsEdit` («Agents can edit the open project») и `settings.agentsEditHint` (одно короткое
   предложение: агент через MCP правит проект на экране, Ctrl+Z отменяет его серию правок) в
   `src/crates/concat/locales/en.json` и **во всех 25 файлах** `locales/*.json` (в `ru.json` — по-русски:
   «Агенты могут править открытый проект» и пояснение; в остальных — английский текст). Проверка — `python
   scripts/locales.py --check` из корня репозитория (можно запускать).
5. Тест в `settings.rs` или `prefs.rs`: старый `settings.json` без `agents` читается с `agents == false`; функция,
   которая включает агентов, выставляет `enabled`, токен и loopback-адрес (вынеси её чистой функцией над `ServerPrefs`).

## Разрешённые файлы
`src/crates/concat/src/prefs.rs`, `src/crates/concat/src/panes/settings.rs`, `src/crates/concat/ui/dialogs/settings.slint`,
`src/crates/concat/locales/*.json`.

## Проверка
Из `src/`: `cargo check -p concat --tests`; из корня: `python scripts/locales.py --check`.
