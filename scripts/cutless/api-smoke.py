#!/usr/bin/env python3
"""Сквозная проверка API Concat через `concat-cli api`: проект, импорт, правки, кадр, экспорт.

Запуск из корня репозитория после `source vendor/env.sh`:
    python scripts/cutless/api-smoke.py [--cli путь/к/concat-cli.exe] [--keep]
"""

import argparse
import json
import queue
import shutil
import subprocess
import sys
import tempfile
import threading
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_CLI = ROOT / "src" / "target" / "quick" / "concat-cli.exe"
WAIT_REPLY = 60
WAIT_EXPORT = 600


class Api:
    """Процесс `concat-cli api`: запрос строкой в stdin, ответы и события строками из stdout."""

    def __init__(self, cli: Path):
        self.proc = subprocess.Popen(
            [str(cli), "api"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=sys.stderr,
            text=True,
            encoding="utf-8",
        )
        self.lines: "queue.Queue[dict]" = queue.Queue()
        self.events: list[dict] = []
        self.next_id = 0
        threading.Thread(target=self._read, daemon=True).start()

    def _read(self) -> None:
        for line in self.proc.stdout:
            if line.strip():
                self.lines.put(json.loads(line))

    def call(self, method: str, **params) -> dict:
        self.next_id += 1
        call_id = self.next_id
        message = {"jsonrpc": "2.0", "id": call_id, "method": method, "params": params}
        self.proc.stdin.write(json.dumps(message) + "\n")
        self.proc.stdin.flush()
        while True:
            line = self.lines.get(timeout=WAIT_REPLY)
            if line.get("id") != call_id:
                self.events.append(line)
                continue
            if "error" in line:
                raise RuntimeError(f"{method}: {line['error']}")
            return line["result"]

    def wait_event(self, names: set[str], job: str) -> dict:
        while True:
            pending = [e for e in self.events if e.get("method") in names and e["params"].get("job") == job]
            if pending:
                return pending[0]
            self.events.append(self.lines.get(timeout=WAIT_EXPORT))

    def close(self) -> None:
        self.proc.stdin.close()
        self.proc.wait(timeout=WAIT_REPLY)


def make_media(folder: Path) -> Path:
    """Пятисекундный синтетический ролик 1920x1080 со звуком: не зависит от файлов владельца."""
    out = folder / "source.mp4"
    subprocess.run(
        ["ffmpeg", "-v", "error", "-y",
         "-f", "lavfi", "-i", "testsrc2=size=1920x1080:rate=30:duration=5",
         "-f", "lavfi", "-i", "sine=frequency=440:duration=5",
         "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest", str(out)],
        check=True,
    )
    return out


def step(name: str, detail: str = "") -> None:
    print(f"ok  {name}{'  ' + detail if detail else ''}")


def run(cli: Path, work: Path) -> None:
    media = make_media(work)
    api = Api(cli)
    try:
        version = api.call("version")
        step("version", str(version.get("apiVersion", version)))

        vertical = {"width": 1080, "height": 1920, "rateNum": 30, "rateDen": 1}
        view = api.call("project.create", location=str(work), name="smoke", video=vertical)
        project = str(work / "smoke")
        step("project.create", project)

        view = api.call("media.import", path=project, file=str(media))
        media_id = view["project"]["media"][0]["id"]
        step("media.import", media_id)

        view = api.call("edit.apply", path=project, command={"op": "addClipAtFirstFree", "mediaId": media_id, "start": 0})
        clip_id = view["createdId"]
        step("addClipAtFirstFree", clip_id)

        view = api.call("edit.apply", path=project, command={"op": "splitClips", "clipIds": [clip_id], "time": 2.0})
        step("splitClips", f"clips: {len(active_clips(view))}")

        text = {"op": "addTextClip", "above": True, "start": 0.5, "duration": 2.0, "style": {"content": "Cutless"}}
        view = api.call("edit.apply", path=project, command=text)
        step("addTextClip", view["createdId"])

        before = len(active_clips(view))
        undone = len(active_clips(api.call("edit.undo", path=project)))
        redone = len(active_clips(api.call("edit.redo", path=project)))
        assert (undone, redone) == (before - 1, before), (before, undone, redone)
        step("undo/redo", f"{before} -> {undone} -> {redone}")

        frame = work / "frame.png"
        api.call("preview.frame", path=project, time=1.0, output=str(frame))
        assert frame.stat().st_size > 0
        step("preview.frame", f"{frame} ({frame.stat().st_size} байт)")

        out = work / "out.mp4"
        started = api.call("export.run", path=project, output=str(out))
        job = started["job"]
        end = api.wait_event({"export.done", "export.failed"}, job)
        if end["method"] == "export.failed":
            raise RuntimeError(f"export: {end['params']}")
        step("export.run", f"{out} ({out.stat().st_size} байт)")

        api.call("project.close", path=project, save=True)
        step("project.close")
    finally:
        api.close()


def active_clips(view: dict) -> list:
    project = view["project"]
    active = project["activeTimelineId"]
    timeline = next(t for t in project["timelines"] if t["id"] == active)
    return timeline["clips"]


def main() -> None:
    # Консоль Windows может быть в cp1251: печатаем в UTF-8, чтобы вывод не падал.
    sys.stdout.reconfigure(encoding="utf-8")
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--cli", type=Path, default=DEFAULT_CLI)
    parser.add_argument("--keep", action="store_true", help="не удалять рабочую папку")
    args = parser.parse_args()

    work = Path(tempfile.mkdtemp(prefix="cutless-smoke-"))
    try:
        run(args.cli, work)
        print(f"api-smoke: всё прошло, папка {work}")
    finally:
        if not args.keep:
            shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
