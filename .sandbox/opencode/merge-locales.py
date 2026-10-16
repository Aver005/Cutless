#!/usr/bin/env python3
"""Сливает переводы исполнителей в локали окна.

    python .sandbox/opencode/merge-locales.py <фрагмент.json> [...]

Фрагмент: {"ключ": {"en": "...", "ru": "...", ...}}. Ключ, который уже есть в локали, не трогается;
у ключа из двух фрагментов побеждает первый. Файлы пишутся отсортированными, как их держит проект.
"""

import json
import pathlib
import sys

LOCALES = pathlib.Path(__file__).resolve().parents[2] / "src" / "crates" / "concat" / "locales"


def main() -> None:
    sys.stdout.reconfigure(encoding="utf-8")
    merged: dict[str, dict[str, str]] = {}
    for name in sys.argv[1:]:
        for key, words in json.loads(pathlib.Path(name).read_text(encoding="utf-8")).items():
            merged.setdefault(key, words)
    for path in sorted(LOCALES.glob("*.json")):
        code = path.stem
        data = json.loads(path.read_text(encoding="utf-8"))
        added = [key for key, words in merged.items() if key not in data and code in words]
        missing = [key for key, words in merged.items() if key not in data and code not in words]
        for key in added:
            data[key] = merged[key][code]
        ordered = {key: data[key] for key in sorted(data)}
        path.write_text(json.dumps(ordered, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        print(f"{code}: +{len(added)}" + (f", нет перевода: {', '.join(missing)}" if missing else ""))


if __name__ == "__main__":
    main()
