#!/usr/bin/env bash
# Пачка исполнителей: batch.sh <папка промптов> <папка результатов> [параллельно=4]
# Каждый <имя>.md даёт <результаты>/<имя>.md. _common.md (если есть) приклеивается в начало каждого.
# Рабочая папка: worktree задачи, иначе DS_DIR, иначе папка результатов.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

[[ $# -ge 2 ]] || { echo "Формат: batch.sh <промпты> <результаты> [N]" >&2; exit 2; }
PROMPTS="$(cd "$1" && pwd)"; mkdir -p "$2"; RESULTS="$(cd "$2" && pwd)"; JOBS="${3:-4}"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT

for file in "$PROMPTS"/*.md; do
  name="$(basename "$file")"
  [[ "$name" == _* ]] && continue
  [[ -s "$RESULTS/$name" ]] && { echo "skip $name (уже есть)"; continue; }
  cat "$PROMPTS/_common.md" "$file" 2>/dev/null > "$TMP/$name" || cat "$file" > "$TMP/$name"
  while (( $(jobs -rp | wc -l) >= JOBS )); do wait -n || true; done
  # Задание <задача>.md работает в своём worktree $WT_ROOT/<задача>, если он есть.
  dir="${DS_DIR:-}"; [[ -d "$WT_ROOT/${name%.md}" ]] && dir="$WT_ROOT/${name%.md}"
  "$SANDBOX/run.sh" "$TMP/$name" "$RESULTS/$name" ${dir:+"$dir"} &
done
wait
