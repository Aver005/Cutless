# Задача: ребрендинг Concat → Cutless (идентичность продукта)

Продукт называется **Cutless** (форк Concat). Пользователь нигде в интерфейсе не должен видеть «Concat» как имя
программы. Иконку и цвета делает интегратор — их не трогай.

## Что сделать

1. `src/crates/concat-host/src/dirs.rs`: `IDENTIFIER` → `"app.cutless.editor"`. Найди и поправь все тесты и
   комментарии в Rust, где ожидается старая строка (`git grep -n "app.concat.editor" -- src`). Комментарии и
   документацию в `docs/` про путь пользовательских локалей (`docs/TRANSLATING.md`) тоже обнови.
2. `src/crates/concat-host/src/updates.rs`: репозиторий обновлений → `Aver005/Cutless` (константа `REPO` и
   `RELEASES_URL`). Логику не меняй. Поправь тесты в этом файле, если они ссылаются на старый репозиторий.
3. `src/crates/concat-host/src/models.rs`: **не менять** (`REPO` моделей остаётся зеркалом апстрима). Добавь над
   константой одну строку комментария: модели пока качаются с зеркала апстрима, своё зеркало — позже.
4. Заголовок окна: `src/crates/concat/ui/app.slint` (`title: "Concat"`) и `src/crates/concat/src/platform.rs`
   (`.set_title("Concat")`) → `"Cutless"`.
5. `src/crates/concat/build.rs`: `ProductName` и `FileDescription` → `"Cutless"`; комментарий рядом поправь по смыслу.
6. `src/crates/concat/src/panes/start.rs`: папка проектов по умолчанию `Desktop/Concat` → `Desktop/Cutless`
   (и для Android `home("Concat")` → `home("Cutless")`). Поправь тест в `src/crates/concat/src/studio.rs`, который
   ожидает `Desktop/Concat` (единственная правка в `studio.rs`, больше его не трогай).
7. Локали `src/crates/concat/locales/*.json`: во **всех** файлах замени имя программы «Concat» на «Cutless» в
   значениях строк (ключи НЕ переименовывай). Включая «Concat API» → «Cutless API», «Concat's mirror» →
   «Cutless's mirror» и переводы этих фраз в других языках (там, где в переводе стоит слово Concat).
   Сохраняй формат файлов: отсортированные ключи, отступ 2 пробела, UTF-8, перевод строки LF.
8. Если в настройках есть раздел «О программе» / версия («Concat {0}» и т.п.) — после замены там будет «Cutless».
   Ничего нового в интерфейс не добавляй.

## Не трогать
Имена крейтов и бинарника, переменные окружения `CONCAT_*`, SPDX-шапки, `docs/upstream/`, Android-пакет
`app.concat.editor` в `src/crates/concat-android/` и `src/scripts/ios-app.sh` (мобильные заморожены), инсталляторы
в `assets/`, workflow в `.github/`, тему `src/crates/concat/ui/theme/`.

## Проверка
Из `src/`: `cargo check -p concat-host --tests` и `cargo check -p concat`. Из корня: `python scripts/locales.py --check`
(должен завершиться с кодом 0). В отчёте перечисли, сколько строк локалей поменял в каждом файле.
