# UPSTREAM — как жить рядом с апстримом
> Remotes, ветки, синхронизация, PR в Concat. Last updated: 2026-10-16.

Апстрим: https://github.com/jub0t/concat. Очень быстрый: около 600 коммитов в сентябре 2026, почти
каждый день новые. Почти весь код пишет один мейнтейнер (`@jub0t`). Поэтому Cutless — **мягкий
форк** (`DECISIONS.md`): регулярно забираем апстрим, своё держим в отдельных крейтах, общее отдаём в PR.

## Репозиторий на GitHub

Нужен именно **форк через кнопку Fork**, а не новый пустой репозиторий с залитой историей.
PR в чужой репозиторий открывается только из репозитория той же сети форков. Плюс форк даёт кнопку
«Sync fork».

1. На https://github.com/jub0t/concat нажать **Fork**.
2. Owner — `Aver005`, **Repository name — `Cutless`** (сделано 2026-10-16).
3. Галку **«Copy the main branch only» поставить**: чужие ветки не нужны.
4. **Create fork.** Репозиторий будет публичным с подписью «forked from jub0t/concat». Это честно и
   нужно по `docs/upstream/TRADEMARK.md`.
5. **Settings → Actions:** у форков Actions по умолчанию выключены, включать осознанно. Workflow
   апстрима (`release.yml`, `models.yml`) завязаны на чужие секреты и зеркало моделей — до фазы 1
   их не запускать (`reference/REBRAND.md`).
6. После первого пуша `develop` — **Settings → General → Default branch = `develop`.**

Если однажды понадобится полностью отделиться: Settings → Danger Zone → **Leave fork network**.
Это необратимо: пропадают issues, PR и звёзды, а PR в апстрим из этого репозитория больше открыть
нельзя. Делать только при жёстком расхождении с апстримом.

## Remotes в локальной копии

Сделано 2026-10-16 (форк — https://github.com/Aver005/Cutless). Команды для другой машины:

```
git remote rename origin upstream
git remote add origin https://github.com/Aver005/Cutless.git
git remote set-url --push upstream DISABLED   # защита от случайного пуша в чужой репозиторий
git fetch --all
```

## Ветки

| Ветка | Что в ней | Правила |
|---|---|---|
| `main` | Чистое зеркало `upstream/main` | Своих коммитов **никогда**. Обновляется fast-forward или кнопкой «Sync fork» |
| `develop` | **Cutless**: ребрендинг, `.memories/`, `CLAUDE.md`, свои крейты, интеграция | Ветка по умолчанию. Апстрим вливается сюда merge'ем, не rebase: история публичная |
| `feat/*`, `fix/*` | Работа Cutless | От `develop`, обратно в `develop` |
| `up/*` | Будущий PR в апстрим | **Только от `upstream/main`**, без ребрендинга и без файлов Cutless |

## Синхронизация с апстримом (раз в несколько дней)

```
git fetch upstream
git switch main && git merge --ff-only upstream/main && git push origin main
git switch develop && git merge main      # конфликты решать здесь
cd src && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

Чем реже синхронизация, тем больнее конфликты: апстрим каждый день трогает `studio.rs` и `ui/`.
Записывать в `JOURNAL/` тяжёлые слияния и то, что сломалось.

## Как отправить PR в апстрим

1. Перед крупной работой — issue или Discord апстрима: так требует их `.github/CONTRIBUTING.md`.
2. Ветка от свежего апстрима:
   ```
   git fetch upstream
   git switch -c up/mcp-server upstream/main
   ```
3. Коммиты в **стиле апстрима**: `feat(api): ...`, без эмодзи и без тела Keep a Changelog.
   Каждый коммит с подписью `-s`: CLA апстрима требует строку
   `Signed-off-by: Имя <почта>`, без неё PR не примут.
   ```
   git commit -s -m "feat(api): an MCP transport over the API"
   ```
4. Гейты апстрима: `cargo fmt --check`, `clippy -D warnings`, `cargo test`, `scripts/locales.py --check`.
5. `git push origin up/mcp-server`, затем на GitHub: **Compare & pull request**, база —
   `jub0t/concat:main`.
6. Не ждать мержа: влить `up/mcp-server` в `develop`. Когда апстрим примет PR (даже squash'ем), тот
   же код придёт через `main`. Git обычно сводит одинаковые правки без конфликта.

## Что никогда не уходит в апстрим

`.memories/`, `CLAUDE.md`, `.trae/`, ребрендинг, монетизация, код подписки, всё про Cutless как продукт.

## Как писать код, чтобы мерж был лёгким

- Новое — в **своих крейтах** (`cutless-*`). В файлах апстрима — точечные вставки: регистрация
  панели, строка в `match`, вызов хука.
- В файлах апстрима комментарии и стиль — как у апстрима (английский). Так правку можно отдать в PR.
- Ребрендинг — через **одну точку** (`reference/REBRAND.md`), а не правкой 57 файлов.
  Саму точку — предложить апстриму как «identity for forks» → апстрим.
- Переформатирование и «попутные» правки в чужих файлах запрещены: каждая лишняя строка — будущий конфликт.

## Кандидаты на PR → апстрим

| Что | Почему апстрим примет |
|---|---|
| Убрать лишний `vn.json` (или внести в `BUILT_IN`) | Чинит красный CI `main`: `every_locale_file_ships_and_each_once` (`BUGS.md`) |
| Две сессии ONNX в одном процессе | Падение, которое nextest прячет (`BUGS.md`); сначала воспроизвести в окне |
| MCP-транспорт над `concat-api` | Обещан в их README, место оставлено в `concat-api/src/lib.rs` |
| Глаголы `captions.*`, `transcript.get`, `speech.*` в API | Сейчас доступны только из окна |
| `accessible-label` на контролах | Доступность, в их §15 это известная дыра |
| Импорт и экспорт SRT | Частый запрос, в коде нет |
| Настраиваемая идентичность для форков | Их `docs/upstream/TRADEMARK.md` требует от форков переименования |
| Адресация эффектов по id и проверка `revision` | Чинит гонки при внешних правках |
