#!/usr/bin/env bash
# Общие настройки исполнителей DeepSeek: модель, изолированные данные OpenCode, окружение сборки Cutless.
# Исполнитель видит FFmpeg и libclang из vendor/ и проверяет код в общей папке сборки — без пересборки зависимостей.
set -euo pipefail

SANDBOX="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$SANDBOX/../.." && pwd)"
DS_MODEL="${DS_MODEL:-opencode-go/deepseek-v4.1-flash}"
DS_TIMEOUT="${DS_TIMEOUT:-2700}"
DS_DATA="${DS_DATA:-$SANDBOX/.data}"
GLOBAL_AUTH="${GLOBAL_AUTH:-$HOME/.local/share/opencode/auth.json}"
# Worktree'и исполнителей лежат рядом с репозиторием, а не внутри него.
WT_ROOT="${WT_ROOT:-$(cd "$REPO/.." && pwd)/cutless-wt}"

# Путь, понятный OpenCode под Windows: /e/x из Git Bash он превращает в E:\e\x.
native_path() {
  if command -v cygpath >/dev/null; then cygpath -m "$1"; else printf '%s' "$1"; fi
}

# Готовит изолированную папку данных с копией ключей провайдеров.
ds_prepare() {
  mkdir -p "$DS_DATA/opencode"
  if [[ ! -f "$DS_DATA/opencode/auth.json" ]]; then
    [[ -f "$GLOBAL_AUTH" ]] || { echo "Нет $GLOBAL_AUTH: выполните opencode auth login" >&2; exit 1; }
    cp "$GLOBAL_AUTH" "$DS_DATA/opencode/auth.json"
  fi
}

# Окружение сборки: FFmpeg, libclang, ONNX Runtime из vendor/ и одна папка target на всех.
ds_build_env() {
  [[ -f "$REPO/vendor/env.sh" ]] && source "$REPO/vendor/env.sh"
  export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$(native_path "$REPO/src/target")}"
  export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"
  export CARGO_PROFILE_DEV_DEBUG=0
}

# Запускает один промпт: ds_exec <рабочая папка> <текст промпта> <лог>.
ds_exec() {
  local dir="$1" prompt="$2" log="$3"
  ds_prepare
  ds_build_env
  # Свой конфиг отключает браузеры и прочие MCP из глобальных настроек: исполнителю они не нужны.
  (cd "$dir" && XDG_DATA_HOME="$DS_DATA" OPENCODE_CONFIG="${DS_CONFIG:-$SANDBOX/opencode.json}" \
    OPENCODE_ENABLE_EXA=1 timeout "$DS_TIMEOUT" \
    opencode run -m "$DS_MODEL" --dangerously-skip-permissions "$prompt") > "$log" 2>&1
}
