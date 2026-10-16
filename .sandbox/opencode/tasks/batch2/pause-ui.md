# Задача: интерфейс вырезателя пауз (окно, разметка на таймлайне, полоса, режим «без пауз»)

Ядро уже есть и влито:
- крейт анализа `src/crates/cutless-pauses` — `analyze_file(&FileRequest, &PauseSettings) -> Result<Analysis, PauseError>`
  (паузы — отрезки в секундах от начала окна клипа, уже с отступами);
- команды документа в `src/crates/concat-project/src/commands/pauses.rs`: `MarkPauses { clip_id, ranges }`
  (секунды таймлайна), `SetClipPause { clip_ids, on }`, `RestorePauses { clip_ids }`, `ConfirmPauses { clip_ids }`
  и `pub fn is_pause(&Clip) -> bool`.

Прочитай ПЕРЕД работой: `.memories/reference/PAUSES.md` (проект и решения владельца), `pauses.rs`,
`cutless-pauses/src/lib.rs`, образец окна-инструмента `src/crates/concat/src/panes/captions.rs` +
`src/crates/concat/ui/dialogs/captions.slint` (как кнопка в `ui/timeline/tray.slint` открывает окно, как свойства и
колбэки пробрасываются в `ui/app.slint`, как вариант `Msg` в `src/panes/mod.rs` попадает в `Studio`), отрисовку клипов
`ui/timeline/model.slint` (`ClipData`), `ui/timeline/lanes.slint` (там «вуаль» заблокированного клипа — образец заливки)
и `Studio::publish_lanes` в `src/studio.rs`, воспроизведение `Studio::play_toggle` в `src/studio.rs`.

## Решения владельца (не менять)
- Пороги по умолчанию: автопорог из звука; минимальная пауза **0,3 с**; отступ **1/3 с** с каждого края.
- Анализируются выделенные клипы, а если ничего не выделено — все клипы со звуком, кроме музыки
  (музыка = клип вида `Audio` без `detached_from`).
- Ничего не удаляется молча: «Разметить» только режет и помечает (`MarkPauses`).
- Клик по помеченному куску — вернуть паузу (`RestorePauses`, кусок склеивается обратно); клик по куску, который
  был паузой и возвращён, не нужен — достаточно вернуть.
- Полоса над таймлайном (не модальная!): «N пауз · M,M с · Подтвердить все · Вернуть все · Закрыть».
  «Подтвердить все» = `ConfirmPauses { clip_ids: None }`.
- Режим плеера «Оригинал / Без пауз»: во втором режиме при воспроизведении помеченные отрезки перепрыгиваются
  (в таймере `play_toggle`: если позиция попала в помеченный отрезок — `seek` на его конец).

## Что сделать
1. Новая панель `src/crates/concat/src/panes/pauses.rs` по образцу `captions.rs`: состояние окна, `PausesMsg`,
   `update`, `data()`. Анализ — на рабочем потоке (как в captions), результат → `MarkPauses` для каждого клипа
   одним шагом undo (`Command::Batch`). Перевод времени: отрезок от начала окна клипа → секунды таймлайна
   (`clip.start + (t - 0) / speed` при `source_start` как начале окна; клипы с `speed_curve` пропускать с сообщением).
2. Окно параметров `src/crates/concat/ui/dialogs/pauses.slint`: переключатель «порог авто / вручную» + поле порога (дБ),
   минимальная пауза (с), отступ (с), кнопка «Разметить». Полоса с итогом и кнопками — отдельный компонент в том же
   файле, показывается над таймлайном, пока на таймлайне есть помеченные куски.
3. Таймлайн: поле `pause: bool` в `ClipData`, заполнение в `publish_lanes` через `concat_project::commands::pauses::is_pause`
   (проверь, как модуль экспортирован; при необходимости сделай реэкспорт в `concat-project`), отрисовка в `lanes.slint`:
   красная вуаль `Theme.danger-soft`, рамка `Theme.danger`, косая линия зачёркивания. НЕ использовать `opacity` и
   слои — см. комментарий в `lanes.slint` про лимит текстуры GPU. Клик по помеченному клипу → `RestorePauses`.
4. Кнопка «Паузы» в `ui/timeline/tray.slint` рядом с субтитрами; переключатель «Оригинал / Без пауз» в
   `ui/workspace/preview-pane.slint` (рядом с селекторами качества).
5. Все строки интерфейса — через `I18n.t("pauses.…")` с английским в `locales/en.json` и русским в `locales/ru.json`;
   в остальные локали положи английский текст (как сделано для других новых строк). Проверка `python scripts/locales.py --check`.
6. `src/crates/concat/Cargo.toml` — добавить зависимость на `cutless-pauses` (путь `../cutless-pauses`).

## Разрешённые файлы
`src/crates/concat/src/panes/pauses.rs` (новый), `src/crates/concat/src/panes/mod.rs`, `src/crates/concat/src/studio.rs`
(только точечные вставки: публикация флага, маршрутизация Msg, прыжок в таймере), `src/crates/concat/src/lib.rs`
(если там связываются колбэки), `src/crates/concat/ui/dialogs/pauses.slint` (новый), `ui/app.slint`, `ui/timeline/model.slint`,
`ui/timeline/lanes.slint`, `ui/timeline/tray.slint`, `ui/workspace/preview-pane.slint`, `src/crates/concat/locales/*.json`,
`src/crates/concat/Cargo.toml`, `src/crates/concat-project/src/lib.rs` или `commands/mod.rs` — только реэкспорт `is_pause`.
`src/Cargo.lock` обновится сам.

## Проверка
Из `src/`: `cargo check -p concat` (Slint скомпилируется в составе). Из корня: `python scripts/locales.py --check`.
Окно не запускай — его посмотрит интегратор. В отчёте опиши, где в интерфейсе что появилось.
