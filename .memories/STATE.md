# STATE
> Снимок состояния. Last updated: 2026-10-16 (фаза 0 пройдена: сборка, гейт, тесты, сценарий API).

## Сейчас в фокусе

Фаза 0 пройдена, кроме двух мелочей владельца (ветка по умолчанию, CI форка). Дальше на выбор:
первый PR в апстрим (`vn.json`), ребрендинг (Ф1) или начало агентного API (Ф2).

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
- `cargo fmt --check` и `cargo clippy --all-targets -D warnings` чистые (clippy — 4 мин 37 с с нуля).
  Pre-commit hook `.githooks/pre-commit` включён (Ф0.6).
- `cargo test --workspace` (без `every_edit_still_exports`): 636 + 91 (`concat-render`) зелёных, 1 пропущен.
  Падения — не наши: `vn.json` апстрима и две сессии ONNX в одном процессе (`BUGS.md`).
- Сценарий API `scripts/cutless/api-smoke.py` (Ф0.5): 10 шагов за 5 с, на выходе MP4 1080×1920, 30 fps,
  H.264 + AAC, 5,000 с. Кадр проверен глазами: титр на месте, исходник 16:9 вписан с полями.

## Состояние репозитория

- Форк на GitHub: https://github.com/Aver005/Cutless (настоящий форк `jub0t/concat`). `main` = апстрим
  `ef49577`, `develop` = `9ad9ac2` (база знаний).
- Remotes: `origin` = Aver005/Cutless, `upstream` = jub0t/concat (пуш выключен). Локальная `main` следит
  за `upstream/main`.
- Git-личность и подпись — локальные в `.git/config` (`LEARNINGS.md` → Окружение и инструменты).
- Код движка и окна совпадает с апстримом `v0.2.7-21-gef49577`, своих правок в коде нет.
- `.trae/` (правило коммитов владельца) теперь в `.gitignore`. Само правило продублировано в `CONVENTIONS.md`.

## Не проверено `[?]`

- Окно собрано, но не запускалось.
- `concat-cli serve` (сокет) вживую не проверялся; `api` через stdin — проверен.
- Апстрим продвинулся на два мержа (`0ea476a`, `0f786c4`) — синхронизацию ещё не делали.
