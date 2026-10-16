#!/usr/bin/env bash
# Состояние пачки: status.sh <папка результатов> — размер результатов и хвост логов.
[[ $# -ge 1 ]] || { echo "Формат: status.sh <результаты>" >&2; exit 2; }
for log in "$1"/*.log; do
  [[ -e "$log" ]] || continue
  out="${log%.log}"
  size=$([[ -f "$out" ]] && wc -c < "$out" || echo 0)
  printf '%-32s %8s байт | %s\n' "$(basename "$out")" "$size" "$(tail -n 1 "$log" | cut -c1-80)"
done
