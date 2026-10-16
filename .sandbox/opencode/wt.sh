#!/usr/bin/env bash
# Worktree на задачу: wt.sh new <задача> — ветка agent/<задача> от develop в $WT_ROOT/<задача>;
# wt.sh diff <задача> — что исполнитель поменял; wt.sh drop <задача> — убрать worktree и ветку.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cmd="${1:-}"; task="${2:-}"
[[ -n "$cmd" && -n "$task" ]] || { echo "Формат: wt.sh new|diff|drop <задача>" >&2; exit 2; }
dir="$WT_ROOT/$task"
case "$cmd" in
  new)
    mkdir -p "$WT_ROOT"
    git -C "$REPO" worktree add -q -b "agent/$task" "$dir" develop
    echo "$(native_path "$dir")"
    ;;
  diff)
    git -C "$dir" add -A -N . && git -C "$dir" diff --stat && git -C "$dir" status --short
    ;;
  drop)
    git -C "$REPO" worktree remove --force "$dir"
    git -C "$REPO" branch -D "agent/$task"
    ;;
  *) echo "Неизвестная команда: $cmd" >&2; exit 2 ;;
esac
