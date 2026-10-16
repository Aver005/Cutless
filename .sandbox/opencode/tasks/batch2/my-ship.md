# Задача: «Мой корабль» — сохранённая настройка цвета, применяется одной кнопкой

В Cutless пиратская тема: пресет цвета называется **«Мой корабль»** (англ. «My ship»). Пользователь снимает видео на
фронтальную камеру телефона при домашнем свете, один раз настраивает цвет и дальше применяет его ко всем роликам.

Цвет клипа в Concat — это цепочка эффектов `clip.video_effects` (`Vec<AppliedFilter>` в
`src/crates/concat-project/src/model.rs`), правится командой `UpdateClip { patch: ClipPatch { video_effects, .. } }`
(см. `commands/mod.rs`). Панель цвета в инспекторе — `src/crates/concat/ui/inspector/grading.slint` и
`src/crates/concat/src/grading.rs`. Изучи, какие пакеты считаются «цветом» (поле `category` в
`src/crates/concat-effects/packages/*/effect.toml`: Color, Basic, фильтры `kind = "filter"`), как инспектор применяет
изменения, как устроены другие пресеты в приложении (`src/crates/concat/src/presets.rs`).

## Что сделать
1. Хранилище кораблей: JSON-файл `ships.json` в папке данных приложения (`AppDirs.data`, см. `concat-host/src/dirs.rs`):
   `[{ "id", "name", "created", "effects": [AppliedFilter…] }]`. Модуль `src/crates/concat/src/ships.rs`: загрузить,
   сохранить (атомарно: `.tmp` + rename), добавить, переименовать, удалить. Чистые функции с тестами (без окна).
   В корабль попадают только цветовые эффекты клипа (те, что показывает панель цвета), не геометрия и не звук.
2. Применение: к выделенным клипам, а если выделения нет — ко всем видеоклипам активного таймлайна. Цветовая часть
   цепочки клипа заменяется на корабль, остальные эффекты клипа сохраняются по порядку. Всё — один `Command::Batch`
   (один шаг undo).
3. Интерфейс в панели цвета (`grading.slint`): заголовок «Мой корабль», ряд чипов с сохранёнными кораблями (клик =
   применить), кнопка «Сохранить корабль» (имя по умолчанию «Корабль N», можно переименовать), у чипа меню
   «Переименовать / Удалить». Стиль — существующие примитивы `ui/primitives/*` и токены `Theme.*`, без новых цветов.
4. Строки — через `I18n.t("ships.…")`: английский в `locales/en.json`, русский в `locales/ru.json`, остальным — английский.
5. Без облака: облачная синхронизация — будущая платная функция, сейчас только локально.

## Разрешённые файлы
`src/crates/concat/src/ships.rs` (новый), `src/crates/concat/src/grading.rs`, `src/crates/concat/src/studio.rs`
(точечно: маршрутизация и применение через `Studio::apply`), `src/crates/concat/src/panes/mod.rs` (вариант Msg),
`src/crates/concat/src/lib.rs` (связывание колбэков), `src/crates/concat/ui/inspector/grading.slint`,
`src/crates/concat/ui/app.slint` (пробросы), `src/crates/concat/locales/*.json`.

## Проверка
Из `src/`: `cargo check -p concat --tests`. Из корня: `python scripts/locales.py --check`.
