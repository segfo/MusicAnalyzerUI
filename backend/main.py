from __future__ import annotations

import re
import sys
from pathlib import Path

# Allow importing project-level schema.py
sys.path.insert(0, str(Path(__file__).parents[2]))  # MusicAnalyzer root

from fastapi import FastAPI, HTTPException, Request
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import FileResponse, StreamingResponse
from pydantic import BaseModel
from typing import Optional

from music_analyzer.schema import TrackDataset

app = FastAPI(title="MusicAnalyzer API", version="0.1.0")

app.add_middleware(
    CORSMiddleware,
    allow_origins=[
        "http://localhost:8080",
        "http://127.0.0.1:8080",
        "tauri://localhost",
        "https://tauri.localhost",
    ],
    allow_credentials=True,
    allow_methods=["GET"],
    allow_headers=["*"],
)

BASE_DIR = Path(__file__).parents[2]  # MusicAnalyzer/
OUTPUT_DIR = BASE_DIR / "output"
OUTPUT_JA_DIR = BASE_DIR / "output_ja"
MUSIC_DIR = BASE_DIR / "music"
STEMS_DIR = MUSIC_DIR / "stems"  # music/stems/{stem}/{track}.{ext}

STEM_TRACKS = ["vocals", "drums", "bass", "other"]

AUDIO_EXTS = [".mp3", ".flac", ".wav", ".ogg", ".m4a"]

MEDIA_TYPES = {
    ".mp3": "audio/mpeg",
    ".flac": "audio/flac",
    ".wav": "audio/wav",
    ".ogg": "audio/ogg",
    ".m4a": "audio/mp4",
}


def _find_audio_in_music_dir(stem: str) -> Optional[Path]:
    for ext in AUDIO_EXTS:
        candidate = MUSIC_DIR / (stem + ext)
        if candidate.exists():
            return candidate
    return None


def _resolve_json(stem: str) -> Optional[Path]:
    """output_ja/{stem}_ja.json を優先し、なければ output/{stem}.json を返す。"""
    ja = OUTPUT_JA_DIR / (stem + "_ja.json")
    if ja.exists():
        return ja
    default = OUTPUT_DIR / (stem + ".json")
    if default.exists():
        return default
    return None


def _load_track(stem: str) -> TrackDataset:
    json_file = _resolve_json(stem)
    if json_file is None:
        raise HTTPException(status_code=404, detail=f"Track not found: {stem}")
    return TrackDataset.load_json(json_file)


class TrackSummary(BaseModel):
    stem: str
    filename: str
    bpm: Optional[float]
    segment_count: int
    has_audio: bool


@app.get("/api/tracks", response_model=list[TrackSummary])
def list_tracks():
    # output/ にある stem を基準に列挙し、各 stem で _ja.json を優先して読む
    stems = sorted(p.stem for p in OUTPUT_DIR.glob("*.json"))
    results = []
    for stem in stems:
        try:
            json_file = _resolve_json(stem)
            if json_file is None:
                continue
            track = TrackDataset.load_json(json_file)
            audio = _find_audio_in_music_dir(stem)
            if audio is None:
                fallback = Path(track.track_path)
                if fallback.exists():
                    audio = fallback
            results.append(TrackSummary(
                stem=stem,
                filename=track.track_filename,
                bpm=track.bpm,
                segment_count=len(track.segments),
                has_audio=audio is not None,
            ))
        except Exception:
            continue
    return results


@app.get("/api/tracks/{stem:path}")
def get_track(stem: str) -> TrackDataset:
    return _load_track(stem)


@app.get("/api/audio/{stem:path}")
async def serve_audio(stem: str, request: Request):
    audio = _find_audio_in_music_dir(stem)
    if audio is None:
        track = _load_track(stem)
        fallback = Path(track.track_path)
        if fallback.exists():
            audio = fallback
    if audio is None:
        raise HTTPException(status_code=404, detail="Audio file not found")

    file_size = audio.stat().st_size
    ext = audio.suffix.lower()
    media_type = MEDIA_TYPES.get(ext, "application/octet-stream")

    range_header = request.headers.get("range")
    if range_header:
        match = re.match(r"bytes=(\d+)-(\d*)", range_header)
        if match:
            start = int(match.group(1))
            end = int(match.group(2)) if match.group(2) else file_size - 1
            end = min(end, file_size - 1)
            chunk_size = end - start + 1

            def iter_file():
                with open(audio, "rb") as f:
                    f.seek(start)
                    remaining = chunk_size
                    while remaining > 0:
                        data = f.read(min(65536, remaining))
                        if not data:
                            break
                        remaining -= len(data)
                        yield data

            return StreamingResponse(
                iter_file(),
                status_code=206,
                media_type=media_type,
                headers={
                    "Content-Range": f"bytes {start}-{end}/{file_size}",
                    "Accept-Ranges": "bytes",
                    "Content-Length": str(chunk_size),
                },
            )

    return FileResponse(
        path=str(audio),
        media_type=media_type,
        headers={"Accept-Ranges": "bytes", "Content-Length": str(file_size)},
    )


class StemAvailability(BaseModel):
    vocals: bool
    drums: bool
    bass: bool
    other: bool


def _find_stem_file(stem: str, track_name: str) -> Optional[Path]:
    """music/stems/{stem}/{track_name}.{ext} を検索する。"""
    stem_dir = STEMS_DIR / stem
    for ext in AUDIO_EXTS:
        candidate = stem_dir / (track_name + ext)
        if candidate.exists():
            return candidate
    return None


@app.get("/api/stems/{stem:path}/{track_name}")
async def serve_stem(stem: str, track_name: str, request: Request):
    if track_name not in STEM_TRACKS:
        raise HTTPException(status_code=400, detail=f"Invalid track: {track_name}. Must be one of {STEM_TRACKS}")

    audio = _find_stem_file(stem, track_name)
    if audio is None:
        raise HTTPException(status_code=404, detail=f"Stem not found: {stem}/{track_name}")

    file_size = audio.stat().st_size
    ext = audio.suffix.lower()
    media_type = MEDIA_TYPES.get(ext, "application/octet-stream")

    range_header = request.headers.get("range")
    if range_header:
        match = re.match(r"bytes=(\d+)-(\d*)", range_header)
        if match:
            start = int(match.group(1))
            end = int(match.group(2)) if match.group(2) else file_size - 1
            end = min(end, file_size - 1)
            chunk_size = end - start + 1

            def iter_file():
                with open(audio, "rb") as f:
                    f.seek(start)
                    remaining = chunk_size
                    while remaining > 0:
                        data = f.read(min(65536, remaining))
                        if not data:
                            break
                        remaining -= len(data)
                        yield data

            return StreamingResponse(
                iter_file(),
                status_code=206,
                media_type=media_type,
                headers={
                    "Content-Range": f"bytes {start}-{end}/{file_size}",
                    "Accept-Ranges": "bytes",
                    "Content-Length": str(chunk_size),
                },
            )

    return FileResponse(
        path=str(audio),
        media_type=media_type,
        headers={"Accept-Ranges": "bytes", "Content-Length": str(file_size)},
    )


@app.get("/api/stems/{stem:path}", response_model=StemAvailability)
def get_stem_availability(stem: str):
    return StemAvailability(
        vocals=_find_stem_file(stem, "vocals") is not None,
        drums=_find_stem_file(stem, "drums") is not None,
        bass=_find_stem_file(stem, "bass") is not None,
        other=_find_stem_file(stem, "other") is not None,
    )


if __name__ == "__main__":
    import uvicorn
    uvicorn.run("main:app", host="127.0.0.1", port=7777, reload=True)
