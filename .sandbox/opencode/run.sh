#!/usr/bin/env bash
# Один исполнитель: run.sh <промпт.md> <результат.md> [рабочая папка]
# В промпте {{OUT}} заменяется на абсолютный путь результата. Лог — рядом, <результат>.log.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

[[ $# -ge 2 ]] || { echo "Формат: run.sh <промпт.md> <результат.md> [папка]" >&2; exit 2; }
PROMPT_FILE="$1"
OUT="$(native_path "$(cd "$(dirname "$2")" && pwd)")/$(basename "$2")"
DIR="${3:-$(dirname "$OUT")}"

prompt="$(cat "$PROMPT_FILE")"
prompt="${prompt//\{\{OUT\}\}/$OUT}"
started=$(date +%s)
if ds_exec "$DIR" "$prompt" "$OUT.log"; then code=0; else code=$?; fi
status="ok"; [[ $code -eq 0 && -s "$OUT" ]] || status="fail"
printf '%s %s %ss exit=%s\n' "$status" "$(basename "$OUT")" "$(( $(date +%s) - started ))" "$code"
