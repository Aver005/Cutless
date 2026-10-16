#!/usr/bin/env python3
"""Сквозная проверка MCP-сервера Cutless: ведёт себя как агент и проходит сценарии владельца.

Запуск из корня репозитория после `source vendor/env.sh`:
    python scripts/cutless/mcp-smoke.py --speech речь.wav [--cli путь/к/concat-cli.exe] [--keep]
Нужна русская речь в WAV; модель распознавания берётся из найденных на диске (кэш Hugging Face).
"""

import argparse
import json
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_CLI = ROOT / "src" / "target" / "quick" / "concat-cli.exe"
# Подсказка инструмента: работа идёт фоновой задачей, её ждут через job_status.
JOB = re.compile(r'job_status with job_id "([^"]+)"')


class Mcp:
    """Процесс `concat-cli mcp`: JSON-RPC по строкам stdin/stdout."""

    def __init__(self, cli: Path):
        self.proc = subprocess.Popen([str(cli), "mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=subprocess.DEVNULL, text=True, encoding="utf-8")
        self.next_id = 0

    def request(self, method: str, params: dict | None = None) -> dict:
        self.next_id += 1
        message = {"jsonrpc": "2.0", "id": self.next_id, "method": method}
        if params is not None:
            message["params"] = params
        self.proc.stdin.write(json.dumps(message, ensure_ascii=False) + "\n")
        self.proc.stdin.flush()
        while True:
            reply = json.loads(self.proc.stdout.readline())
            if reply.get("id") == self.next_id:
                return reply

    def notify(self, method: str) -> None:
        self.proc.stdin.write(json.dumps({"jsonrpc": "2.0", "method": method}) + "\n")
        self.proc.stdin.flush()

    def tool(self, tool_name: str, /, **arguments) -> str:
        reply = self.request("tools/call", {"name": tool_name, "arguments": arguments})
        if "error" in reply:
            raise RuntimeError(f"{tool_name}: {reply['error']}")
        result = reply["result"]
        texts = [c["text"] for c in result.get("content", []) if c.get("type") == "text"]
        images = [c for c in result.get("content", []) if c.get("type") == "image"]
        text = "\n".join(texts) + (f"\n[картинок: {len(images)}]" if images else "")
        if result.get("isError"):
            raise RuntimeError(f"{tool_name}: {text}")
        return text

    def close(self) -> None:
        self.proc.stdin.close()
        self.proc.wait(timeout=60)


def make_video(folder: Path, speech: Path) -> Path:
    """Вертикальное видео: речь, 1,5 с тишины, снова речь — материал для субтитров и пауз."""
    audio = folder / "voice.wav"
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", str(speech), "-f", "lavfi", "-t", "1.5", "-i",
                    "anullsrc=r=44100:cl=mono", "-i", str(speech), "-filter_complex",
                    "[0:a]aresample=44100,aformat=channel_layouts=mono[a];[2:a]aresample=44100,aformat=channel_layouts=mono[c];"
                    "[a][1:a][c]concat=n=3:v=0:a=1[out]", "-map", "[out]", str(audio)], check=True)
    seconds = float(subprocess.run(["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0",
                                    str(audio)], check=True, capture_output=True, text=True).stdout)
    video = folder / "talk.mp4"
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i",
                    f"testsrc2=size=1080x1920:rate=30:duration={seconds:.2f}", "-i", str(audio),
                    "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest", str(video)], check=True)
    return video


def step(title: str, text: str) -> None:
    print(f"\n=== {title}\n{text.strip()[:1200]}")


def finish(mcp: Mcp, title: str, text: str) -> str:
    """Печатает ответ и, пока он говорит о фоновой задаче, ждёт её через job_status."""
    step(title, text)
    while found := JOB.search(text):
        text = mcp.tool("job_status", job_id=found.group(1), wait_seconds=45)
        step("job_status", text)
    return text


def run(cli: Path, work: Path, speech: Path) -> None:
    video = make_video(work, speech)
    mcp = Mcp(cli)
    try:
        init = mcp.request("initialize", {"protocolVersion": "2025-06-18", "capabilities": {},
                                          "clientInfo": {"name": "mcp-smoke", "version": "1"}})
        step("initialize", json.dumps(init["result"]["serverInfo"], ensure_ascii=False))
        mcp.notify("notifications/initialized")
        tools = mcp.request("tools/list")["result"]["tools"]
        step("tools/list", f"{len(tools)} инструментов: " + ", ".join(t["name"] for t in tools))

        step("project_create", mcp.tool("project_create", location=str(work), name="agent", width=1080, height=1920, fps=30))
        step("media_import", mcp.tool("media_import", file=str(video)))
        media = mcp.tool("media_list")
        step("media_list", media)
        media_id = next(word for word in media.replace(",", " ").split() if word.startswith("m") and word[1:].isdigit())
        step("clip_add", mcp.tool("clip_add", media_id=media_id, start=0))
        summary = mcp.tool("timeline_summary")
        step("timeline_summary", summary)
        clip_id = next(word for word in summary.split() if word.startswith("c") and word[1:].isdigit())

        step("speech_models", mcp.tool("speech_models"))
        finish(mcp, "transcribe", mcp.tool("transcribe", clip_id=clip_id, language="ru"))
        step("transcript_search", mcp.tool("transcript_search", query="фронтальную камеру"))
        step("captions_make", mcp.tool("captions_make", clip_id=clip_id))

        step("pauses_analyze", mcp.tool("pauses_analyze"))
        step("pauses_mark", mcp.tool("pauses_mark"))
        step("pauses_list", mcp.tool("pauses_list"))
        step("audio_clean", mcp.tool("audio_clean", preset="phone_voice"))
        step("frame_at", mcp.tool("frame_at", time=2.0, width=360))
        step("timeline_summary (итог)", mcp.tool("timeline_summary"))
        step("undo", mcp.tool("undo"))
        step("project_save", mcp.tool("project_save"))
        finish(mcp, "export_start", mcp.tool("export_start", output=str(work / "agent.mp4")))
    finally:
        mcp.close()


def main() -> None:
    sys.stdout.reconfigure(encoding="utf-8")
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--cli", type=Path, default=DEFAULT_CLI)
    parser.add_argument("--speech", type=Path, required=True, help="WAV с русской речью")
    parser.add_argument("--keep", action="store_true")
    args = parser.parse_args()
    work = Path(tempfile.mkdtemp(prefix="cutless-mcp-"))
    try:
        run(args.cli, work, args.speech)
        print(f"\nmcp-smoke: всё прошло, папка {work}")
    finally:
        if not args.keep:
            shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
