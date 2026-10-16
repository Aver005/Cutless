# Задача: окно «В файле несколько звуковых дорожек» при постановке видео на таймлайн

Сейчас видео ставится на таймлайн тремя путями: перетаскивание из медиатеки (`Studio::place`), двойной клик /
активация (`Studio::place_at_playhead`) и «добавить выделенное» в медиатеке (`src/crates/concat/src/panes/media_bin.rs`,
цикл с `AddClipAtFirstFree`). Нужно: если ставится **видео**, у медиа `audio_tracks.len() > 1`, — спросить человека.

## Что сделать
1. `prefs.rs` (`Preferences`): `split_audio_quietly: bool` (по умолчанию false; `#[serde(default)]` уже на структуре).
   true — окно больше не показывать и всегда разделять. Тест: старый файл читается с false.
2. Подтверждение (`enum Confirm` в `studio.rs`, `confirm_accepted`, `confirm_data`; UI —
   `src/crates/concat/ui/dialogs/confirm.slint`, монтирование в `ui/app.slint`, колбэки в `src/crates/concat/src/lib.rs`):
   - расширь `ConfirmData` полями `check-label: string` (пусто — галочки нет), `checked: bool`, `danger: bool`
     (сейчас кнопка всегда «опасная» и значок корзины — для нового окна нужна обычная кнопка и значок звука/инфо;
     старые подтверждения выглядят как раньше) и колбэком `check-toggled(bool)`. Галочка — компонент `Check` из
     `ui/inspector/controls.slint` (переиспользуй, не копируй).
   - Новый вариант `Confirm::SplitAudio { … }` — хранит, что и куда ставить (для всех трёх путей; для «добавить
     выделенное» — список), и сколько дорожек. Заголовок «В файле несколько звуковых дорожек», текст «Найдено
     дорожек: N. Видео встанет на таймлайн вместе с N звуковыми клипами — по одному на дорожку, связанными в
     группу.», кнопка «Разделить», галочка «Больше не спрашивать».
3. Логика: если `split_audio_quietly` — сразу ставим и разделяем; иначе — `self.confirm = Some(Confirm::SplitAudio…)`.
   ОК: поставить клип (той же командой, что и сейчас) и `DetachAudioLinked` на созданный id — **одним шагом undo**
   (общий жест, затем `end_gesture`), выделить всю группу; если галочка стояла — `split_audio_quietly = true` и
   сохранить настройки. Отмена — ничего не ставить (это уже поведение `confirm = None`). Вынеси решение «нужно ли
   спрашивать» и постановку с разделением в отдельные функции, чтобы три пути вызывали одно и то же.
   Медиа без `audio_tracks` (старые проекты) или с одной дорожкой — как раньше, без окна.
4. Сброс: в настройках, где сбрасывается онбординг (поищи `onboarding_done` в `panes/settings.rs`), — если там
   есть место для «сбросить подсказки», сбрось и `split_audio_quietly`; если нет — не придумывай UI, опиши в отчёте.
5. Строки: ключи `audioTracks.title`, `audioTracks.message` (с `{0}`), `audioTracks.split`, `audioTracks.dontAsk`
   в `src/crates/concat/locales/en.json` и **во всех 25** `locales/*.json` (ru — по-русски, как выше; остальные —
   английский). Файлы пиши тем же форматом, что `scripts/locales.py` (сортировка ключей, отступ 2, `"_"` первым).
   Проверка: `python scripts/locales.py --check` из корня.

## Разрешённые файлы
`src/crates/concat/src/studio.rs`, `src/crates/concat/src/prefs.rs`, `src/crates/concat/src/lib.rs`,
`src/crates/concat/src/panes/media_bin.rs`, `src/crates/concat/src/panes/settings.rs`,
`src/crates/concat/ui/dialogs/confirm.slint`, `src/crates/concat/ui/app.slint`, `src/crates/concat/locales/*.json`.
Не трогай перетаскивание клипов по таймлайну, резку и удаление — это делает другой исполнитель.

## Проверка
Из `src/`: `cargo check -p concat --tests` — без ошибок и предупреждений.
