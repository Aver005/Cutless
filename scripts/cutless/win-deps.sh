#!/usr/bin/env bash
# Раскладывает в vendor/ всё, что нужно для сборки на Windows: FFmpeg, LLVM, ONNX Runtime, sherpa-onnx.
# Запуск из Git Bash в корне репозитория; повторный запуск ничего не качает заново.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
vendor="$root/vendor"
dl="$vendor/dl"
fetch="$root/scripts/ci/fetch.sh"
mkdir -p "$dl"

# В deps.lock апстрима нет портативного архива LLVM, поэтому хэш закреплён здесь.
llvm_url=https://github.com/llvm/llvm-project/releases/download/llvmorg-21.1.0/clang+llvm-21.1.0-x86_64-pc-windows-msvc.tar.xz
llvm_sha256=751aab63f074f041883a5317ad100dbe1e60794693f896df83958824cbc4962a

log() { echo "win-deps: $*"; }
win() { cygpath -w "$1"; }

unzip_once() {
  local archive=$1 into=$2
  [ -d "$into" ] && return 0
  local tmp="$into.part"
  rm -rf "$tmp" && mkdir -p "$tmp"
  unzip -q "$archive" -d "$tmp"
  mv "$tmp" "$into"
}

setup_ffmpeg() {
  "$fetch" ffmpeg-win64 "$dl/ffmpeg.zip"
  unzip_once "$dl/ffmpeg.zip" "$vendor/ffmpeg"
  ffmpeg_dir=$(find "$vendor/ffmpeg" -mindepth 1 -maxdepth 1 -type d | head -1)
  [ -f "$ffmpeg_dir/lib/avcodec.lib" ] || { log "в $ffmpeg_dir нет lib/avcodec.lib"; exit 1; }
}

setup_onnxruntime() {
  "$fetch" onnxruntime-win-x64 "$dl/onnxruntime.zip"
  unzip_once "$dl/onnxruntime.zip" "$vendor/onnxruntime"
  ort_lib=$(find "$vendor/onnxruntime" -mindepth 2 -maxdepth 2 -type d -name lib | head -1)
  [ -f "$ort_lib/onnxruntime.lib" ] || { log "в $ort_lib нет onnxruntime.lib"; exit 1; }
}

# Как в ci.yml: import-библиотеки отдельно, DLL отдельно, без собственного onnxruntime.dll sherpa.
setup_sherpa() {
  local version stem
  version=$(grep -A1 'name = "sherpa-onnx-sys"' "$root/src/Cargo.lock" | sed -n 's/^version = "\(.*\)"/\1/p')
  stem="sherpa-onnx-v$version-win-x64-shared-MT-Release-lib"
  "$fetch" sherpa-onnx-win-x64-shared-MT-Release-lib "$dl/sherpa.tar.bz2"
  if [ ! -d "$vendor/sherpa/libs" ]; then
    rm -rf "$vendor/sherpa" && mkdir -p "$vendor/sherpa/libs" "$vendor/sherpa/dlls"
    tar -xjf "$dl/sherpa.tar.bz2" -C "$vendor/sherpa"
    cp "$vendor/sherpa/$stem/lib/"sherpa-onnx-*.lib "$vendor/sherpa/libs/"
    cp "$vendor/sherpa/$stem/lib/"sherpa-onnx-*.dll "$vendor/sherpa/dlls/"
  fi
}

# Из архива LLVM нужен только libclang.dll для bindgen; остальное не распаковываем.
setup_llvm() {
  local archive="$dl/llvm.tar.xz"
  [ -f "$vendor/llvm/bin/libclang.dll" ] && return 0
  [ -f "$archive" ] || curl -L --fail -sS -o "$archive" "$llvm_url"
  [ "$(sha256sum "$archive" | awk '{print $1}')" = "$llvm_sha256" ] || { log "хэш LLVM не совпал"; rm -f "$archive"; exit 1; }
  mkdir -p "$vendor/llvm/bin"
  tar -xJf "$archive" -C "$vendor/llvm" --strip-components=1 --wildcards '*/bin/libclang.dll'
  [ -f "$vendor/llvm/bin/libclang.dll" ] || { log "libclang.dll не распаковался"; exit 1; }
}

# cmake для whisper.cpp берём из Visual Studio, отдельно не ставим.
find_cmake() {
  local vswhere="/c/Program Files (x86)/Microsoft Visual Studio/Installer/vswhere.exe" install
  while IFS= read -r install; do
    cmake_bin="$(cygpath -u "$install")/Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin"
    [ -x "$cmake_bin/cmake.exe" ] && return 0
  done < <("$vswhere" -products '*' -property installationPath | sed 's/[[:cntrl:]]//g')
  log "cmake из Visual Studio не найден"
  exit 1
}

# Пути машины не попадают в репозиторий: файлы окружения генерируются в vendor/.
write_env() {
  local path_add="$ffmpeg_dir/bin:$ort_lib:$vendor/sherpa/dlls:$vendor/llvm/bin:$cmake_bin"
  cat > "$vendor/env.sh" <<EOF
export FFMPEG_DIR='$(win "$ffmpeg_dir")'
export LIBCLANG_PATH='$(win "$vendor/llvm/bin")'
export ORT_LIB_LOCATION='$(win "$ort_lib")'
export ORT_PREFER_DYNAMIC_LINK=1
# rustc на огромном коде окна, сгенерированном Slint, иначе падает с переполнением стека.
export RUST_MIN_STACK=536870912
export SHERPA_ONNX_LIB_DIR='$(win "$vendor/sherpa/libs")'
export PATH="$path_add:\$PATH"
EOF
  local ps_path
  ps_path=$(cygpath -w -p "$path_add")
  cat > "$vendor/env.ps1" <<EOF
\$env:FFMPEG_DIR = '$(win "$ffmpeg_dir")'
\$env:LIBCLANG_PATH = '$(win "$vendor/llvm/bin")'
\$env:ORT_LIB_LOCATION = '$(win "$ort_lib")'
\$env:ORT_PREFER_DYNAMIC_LINK = '1'
\$env:RUST_MIN_STACK = '536870912'
\$env:SHERPA_ONNX_LIB_DIR = '$(win "$vendor/sherpa/libs")'
\$env:PATH = '$ps_path;' + \$env:PATH
EOF
}

setup_ffmpeg
setup_onnxruntime
setup_sherpa
setup_llvm
find_cmake
write_env
log "готово. Окружение: 'source vendor/env.sh' (Git Bash) или '. vendor/env.ps1' (PowerShell)"
