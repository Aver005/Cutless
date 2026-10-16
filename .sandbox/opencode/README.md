# Исполнители DeepSeek для Cutless

Дешёвые агенты через OpenCode (`opencode-go/deepseek-v4.1-flash`), каждый в своём git worktree.
Основа — `E:\Projects\Me\minecraft-copy\.sandbox\opencode`; отличия ниже.

```sh
.sandbox/opencode/wt.sh new <задача>        # worktree ../cutless-wt/<задача>, ветка agent/<задача> от develop
.sandbox/opencode/batch.sh <задания> <отчёты> 4   # <задача>.md работает в своём worktree; _common.md приклеивается
.sandbox/opencode/status.sh <отчёты>
.sandbox/opencode/wt.sh diff <задача>       # что поменял исполнитель
.sandbox/opencode/wt.sh drop <задача>       # убрать worktree и ветку после слияния
```

- Исполнитель получает окружение сборки из `vendor/env.sh` и общую папку `src/target`: `cargo check`
  своего крейта не пересобирает зависимости. Сборка, тесты и clippy — только у интегратора (Claude).
- Исполнители не коммитят и не пушат: интегратор читает diff, сливает, собирает, тестирует, коммитит.
- `.data/` — копия ключей OpenCode, в git не попадает.
