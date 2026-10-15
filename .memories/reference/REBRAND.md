# REBRAND — где зашит Concat
> Чек-лист фазы 1 (`PLANS.md`). Собран grep'ом по апстриму `v0.2.7-21-gef49577`. Last updated: 2026-10-16.

Цель: сборку Cutless можно распространять, не нарушая `TRADEMARK.md`. Она ставится рядом с Concat
и не делит с ним данные. При этом в файлах апстрима меняется **как можно меньше строк**
(`reference/UPSTREAM.md`).

## Что требует TRADEMARK.md апстрима

- Своё имя и своя иконка у любой изменённой сборки.
- Нельзя выдавать сборку за официальный Concat, нельзя брать логотип и похожие до смешения имена.
- Можно и нужно писать «a fork of Concat».
- SPDX-шапки и копирайты апстрима не трогать: свою строку копирайта добавить можно, чужую убрать нельзя.

## Подход: одна точка идентичности

Не править 57 файлов, а завести одну точку, которую читает весь остальной код:
- имя продукта, идентификатор, репозиторий обновлений, репозиторий и релиз моделей;
- домашняя страница, адрес для issues.

Код:
- в Rust — модуль в `concat-host` (рядом с `dirs.rs`), значения задаются при сборке через
  `option_env!` с умолчаниями Concat;
- в Slint — глобал с именем продукта, который выставляет окно.

Тогда в Cutless меняются только значения сборки, а апстрим может принять сам механизм → апстрим.

## Места в коде (Rust и Slint)

| Где | Что | Влияет на |
|---|---|---|
| → `src/crates/concat-host/src/dirs.rs:25` | `IDENTIFIER = "app.concat.editor"` | Пути данных, настроек, кэша, логов, локалей пользователя. **Сменить обязательно**, иначе Cutless и Concat делят настройки и проекты |
| → `src/crates/concat-host/src/updates.rs:37` | `REPO = "jub0t/Concat"` | Автообновление |
| → `src/crates/concat-host/src/updates.rs:45` | `RELEASES_URL` (api.github.com/repos/jub0t/Concat) | Автообновление |
| → `src/crates/concat-host/src/models.rs:30` | `REPO = "jub0t/Concat"`, `RELEASE = "models-v1"` | Откуда качаются модели |
| → `src/crates/concat/ui/app.slint:69` | `title: "Concat"` | Заголовок окна |
| `src/crates/concat/locales/*.json` | 13 упоминаний «Concat» в `en.json`, в остальных локалях — аналоги | Тексты интерфейса |
| `src/crates/concat/Cargo.toml` | бинарник `concat` | Имя exe. Переименовывать бинарник **не обязательно** до релиза: имя видит только CLI. Менять вместе с инсталляторами |
| `src/crates/concat-android/{Cargo.toml,src/lib.rs,java/ConcatFiles.java}` | пакет `app.concat.editor` | Android. По Р3 — не трогать до решения о мобильных |
| `src/scripts/ios-app.sh` | bundle id | iOS. По Р3 — то же |
| Переменные `CONCAT_*` (`CONCAT_LOG`, `CONCAT_API_TOKEN`, `CONCAT_HARDWARE_ENCODE`…) | Имена переменных окружения | **Не менять:** это API для пользователей и скриптов, совместимость с апстримом важнее |

## Упаковка и CI

| Где | Что |
|---|---|
| `assets/windows/concat.iss` | `AppName=Concat`, `AppPublisherURL`, `AppSupportURL`, `AppUpdatesURL` → github.com/jub0t/Concat. Плюс **AppId** (GUID) — сменить обязательно, иначе установщик Cutless «обновит» Concat `[?]` проверить, где он задан |
| `assets/windows/concat.wxs` | `ARPURLINFOABOUT`, UpgradeCode `[?]` — то же требование про уникальный GUID |
| `assets/linux/nfpm.yaml` | `maintainer` (почта апстрима), `homepage`, имя пакета |
| `assets/arch/PKGBUILD` | `url`, имя пакета |
| `.github/workflows/build-app.yml` | `Concat.app`, имя `.dmg` и тома, идентификатор в smoke-скриптах |
| `.github/workflows/release.yml`, `models.yml` | Публикуют релизы и модели. В форке запускать только после смены точек выше |
| `scripts/ci/smoke-android.sh`, `scripts/ci/smoke-*.sh` | Идентификатор и имя приложения |
| `scripts/models.py:39` | `REPO = "jub0t/Concat"` |
| `flake.nix` | `homepage`, имя пакета |
| `CODE_SIGNING.md` | Команда и роли апстрима — заменить своими или убрать из сборок Cutless |

## Ассеты

- Логотипы Concat: `assets/concat-mark*.png`, `assets/concat_logo*`, `assets/new_concat_logo_*.svg`,
  `assets/icons/concat*.{ico,png}`. Свои — сабля (`PRODUCT.md` → Бренд). Генератор размеров —
  `scripts/generate-icons.sh`.
- `assets/proxyon_sponsor_logo.png`, `download_button.svg`, скриншоты `editor*.png` — апстрима.
  В README Cutless не использовать.

## Документация

- `README.md` и 25 переводов в `docs/README.*.md` генерируются `scripts/readme/gen.py` +
  `scripts/readme/lang_*.py`. Ссылки на jub0t, спонсора и сайт concatenate.pages.dev. Решение по
  переводам — Р3. Предложение: свой README Cutless ru/en, файлы апстрима не трогать, чтобы не
  конфликтовать при слияниях.
- `TRANSLATING.md` упоминает `app.concat.editor` в пути пользовательских локалей.

## Порядок работ

1. Точка идентичности (без смены значений) — отдельным PR → апстрим.
2. В `develop`: значения Cutless, иконки, заголовок, локали, инсталляторы.
3. Своё зеркало моделей, затем `models.yml` в форке.
4. Релизный CI в форке, первый неподписанный релиз под Windows.
5. Проверка: Cutless и Concat установлены рядом, у каждого свои настройки, обновления и модели.
