# STATE
> Снимок состояния. Last updated: 2026-10-16 (окно собрано на Windows; скрипт зависимостей и pre-commit hook).

## Сейчас в фокусе

Фаза 0 (`PLANS.md`): гейт (clippy, `cargo test`), затем ручная цепочка через `concat-cli api` (Ф0.5).
Следом — ребрендинг (фаза 1).

## Сделано `[DONE]`

- Исследование четырёх проектов (concat, ki-video, econcar, pooprusteek) и сравнение —
  `reference/RESEARCH-2026-10-16.md`.
- Приняты базовые решения: остаёмся на Slint, одно приложение с несколькими окнами, мягкий форк,
  ИИ-слой в своих крейтах — `DECISIONS.md`.
- Дорожная карта по фазам — `PLANS.md`. Проект ИИ-слоя — `reference/AI-LAYER.md`.
- База знаний `.memories/` и мост `CLAUDE.md`.
- Имя Cutless, форк на GitHub, remotes, ветка `develop`, `.gitignore` для следов агентов (Ф0.1, Ф0.2).
- Сборка окна на Windows (Ф0.3): `cargo build --profile quick --bin concat` — 10 мин 23 с с нуля,
  0 предупреждений, `concat.exe` 165 МБ, `src/target` 6,7 ГБ. Машина: 12 потоков, 32 ГБ RAM.
  Зависимости — `scripts/cutless/win-deps.sh` (26 с).
- `cargo fmt --check` чистый. Pre-commit hook `.githooks/pre-commit` включён (Ф0.6).

## Состояние репозитория

- Форк на GitHub: https://github.com/Aver005/Cutless (настоящий форк `jub0t/concat`). `main` = апстрим
  `ef49577`, `develop` = `9ad9ac2` (база знаний).
- Remotes: `origin` = Aver005/Cutless, `upstream` = jub0t/concat (пуш выключен). Локальная `main` следит
  за `upstream/main`.
- Git-личность и подпись — локальные в `.git/config` (`LEARNINGS.md` → Окружение и инструменты).
- Код движка и окна совпадает с апстримом `v0.2.7-21-gef49577`, своих правок в коде нет.
- `.trae/` (правило коммитов владельца) теперь в `.gitignore`. Само правило продублировано в `CONVENTIONS.md`.

## Не проверено `[?]`

- Окно собрано, но не запускалось. Тесты не прогонялись.
- Работа `concat-cli api` и `serve` выведена из кода, вживую не проверялась.
- Апстрим заявляет 745 зелёных тестов (аудит 2026-10-12). Сами не прогоняли.
