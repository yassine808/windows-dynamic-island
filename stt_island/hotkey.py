"""Push-to-talk key, read with GetAsyncKeyState so it works over any window without a hook."""
from __future__ import annotations

import ctypes
from ctypes import wintypes

from PySide6.QtCore import QObject, QTimer, Signal

user32 = ctypes.WinDLL("user32", use_last_error=True)
user32.GetAsyncKeyState.argtypes = [ctypes.c_int]
user32.GetAsyncKeyState.restype = ctypes.c_short

VK_NAMES: dict[str, int] = {
    "left ctrl": 0xA2,
    "right ctrl": 0xA3,
    "ctrl": 0xA2,
    "left alt": 0xA4,
    "right alt": 0xA5,
    "alt gr": 0xA5,
    "alt": 0xA4,
    "left shift": 0xA0,
    "right shift": 0xA1,
    "shift": 0xA0,
    "left win": 0x5B,
    "right win": 0x5C,
    "caps lock": 0x14,
    "capslock": 0x14,
    "space": 0x20,
    "tab": 0x09,
    "enter": 0x0D,
    "backtick": 0xC0,
    "`": 0xC0,
    "grave": 0xC0,
    "scroll lock": 0x91,
    "pause": 0x13,
    "insert": 0x2D,
    "home": 0x24,
    "end": 0x23,
    "page up": 0x21,
    "page down": 0x22,
    "delete": 0x2E,
    "apps": 0x5D,
}
for _n in range(1, 25):
    VK_NAMES[f"f{_n}"] = 0x6F + _n


def resolve_key(name: str) -> int | None:
    key = (name or "").strip().lower()
    if not key:
        return None
    if key in VK_NAMES:
        return VK_NAMES[key]
    if len(key) == 1 and (key.isalpha() or key.isdigit()):
        return ord(key.upper())
    return None


def pretty_key(name: str) -> str:
    key = (name or "").strip()
    small = key.lower()
    if small.startswith("f") and small[1:].isdigit():
        return small.upper()
    if len(small) == 1:
        return small.upper()
    return " ".join(word.capitalize() for word in small.replace("_", " ").split())


class HoldKey(QObject):
    """Emits pressed once when the key goes down and released once when it comes back up."""

    pressed = Signal()
    released = Signal()
    problem = Signal(str)

    def __init__(self, key_name: str, interval_ms: int = 16, parent=None) -> None:
        super().__init__(parent)
        self.key_name = key_name
        self._vk = resolve_key(key_name)
        self._down = False
        self._timer = QTimer(self)
        self._timer.setInterval(interval_ms)
        self._timer.timeout.connect(self._poll)

    def start(self) -> None:
        if self._vk is None:
            self.problem.emit(f"Unknown key '{self.key_name}'")
            return
        self._down = False
        self._timer.start()

    def stop(self) -> None:
        self._timer.stop()
        self._down = False

    def rebind(self, key_name: str) -> bool:
        vk = resolve_key(key_name)
        if vk is None:
            return False
        self._down = False
        self.key_name = key_name
        self._vk = vk
        return True

    @property
    def down(self) -> bool:
        return self._down

    def _poll(self) -> None:
        if self._vk is None:
            return
        down = bool(user32.GetAsyncKeyState(self._vk) & 0x8000)
        if down and not self._down:
            self._down = True
            self.pressed.emit()
        elif not down and self._down:
            self._down = False
            self.released.emit()
