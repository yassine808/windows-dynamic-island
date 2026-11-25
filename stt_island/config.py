"""Settings live in config/config.json next to the app; missing keys fall back to DEFAULTS."""
from __future__ import annotations

import json
import logging
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CONFIG_DIR = ROOT / "config"
CONFIG_PATH = CONFIG_DIR / "config.json"
MODEL_DIR = ROOT / "models"
LOG_PATH = CONFIG_DIR / "stt-island.log"

DEFAULTS: dict = {
    "hotkey": "right ctrl",
    "model": "small",
    "language": "",          # "" = auto detect
    "beam_size": 1,
    "paste_into_app": True,
    "play_sounds": True,
    "accent": "#5AC8FA",
    "min_seconds": 0.35,
    "max_seconds": 180,
    "show_hint": True,
}


def load() -> dict:
    CONFIG_DIR.mkdir(parents=True, exist_ok=True)
    data = dict(DEFAULTS)
    try:
        if CONFIG_PATH.exists():
            raw = json.loads(CONFIG_PATH.read_text(encoding="utf-8"))
            if isinstance(raw, dict):
                data.update({k: v for k, v in raw.items() if k in DEFAULTS})
    except (OSError, ValueError) as exc:
        logging.getLogger(__name__).warning("config read failed (%s), using defaults", exc)
    if not CONFIG_PATH.exists():
        save(data)
    return data


def save(cfg: dict) -> None:
    CONFIG_DIR.mkdir(parents=True, exist_ok=True)
    tmp = CONFIG_PATH.with_suffix(".tmp")
    tmp.write_text(json.dumps(cfg, indent=2), encoding="utf-8")
    tmp.replace(CONFIG_PATH)


def setup_logging() -> None:
    CONFIG_DIR.mkdir(parents=True, exist_ok=True)
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s %(levelname)-7s %(name)s: %(message)s",
        handlers=[
            logging.FileHandler(LOG_PATH, encoding="utf-8"),
            logging.StreamHandler(),
        ],
    )
