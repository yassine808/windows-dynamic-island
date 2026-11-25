"""stt-island: hold a key, speak, and the text lands in the app you were typing in."""
from __future__ import annotations

import argparse
import ctypes
import logging
import math
import os
import sys
import time
import winreg
from pathlib import Path

from PySide6.QtCore import QRectF, Qt, QTimer
from PySide6.QtGui import QColor, QGuiApplication, QIcon, QPainter, QPen, QPixmap
from PySide6.QtWidgets import QApplication, QMenu, QSystemTrayIcon

from stt_island import config, inject, sounds
from stt_island.audio import AudioError, Recorder
from stt_island.hotkey import HoldKey, pretty_key
from stt_island.island import Island
from stt_island.settings_ui import SettingsDialog
from stt_island.transcribe import Transcriber

log = logging.getLogger("stt-island")

RUN_KEY = r"Software\Microsoft\Windows\CurrentVersion\Run"
RUN_NAME = "stt-island"
MUTEX_NAME = "Local\\stt-island-single"

MENU_STYLE = """
QMenu { background: #14181f; color: #e8edf5; border: 1px solid #2a313d; padding: 6px; }
QMenu::item { padding: 6px 22px 6px 18px; border-radius: 5px; }
QMenu::item:selected { background: #232b38; }
QMenu::item:disabled { color: #79808f; }
QMenu::separator { height: 1px; background: #262d38; margin: 5px 8px; }
"""


def make_icon(accent: str) -> QIcon:
    pixmap = QPixmap(64, 64)
    pixmap.fill(Qt.transparent)
    painter = QPainter(pixmap)
    painter.setRenderHint(QPainter.Antialiasing, True)
    painter.setBrush(QColor("#0d1015"))
    painter.setPen(QPen(QColor(accent), 3))
    painter.drawRoundedRect(QRectF(3, 3, 58, 58), 18, 18)
    painter.setPen(Qt.NoPen)
    painter.setBrush(QColor(accent))
    painter.drawRoundedRect(QRectF(27, 15, 10, 22), 5, 5)
    painter.setBrush(Qt.NoBrush)
    painter.setPen(QPen(QColor(accent), 3, Qt.SolidLine, Qt.RoundCap))
    painter.drawArc(QRectF(21, 21, 22, 22), 200 * 16, 140 * 16)
    painter.drawLine(32, 44, 32, 49)
    painter.end()
    return QIcon(pixmap)


def autostart_command() -> str:
    python = Path(sys.executable)
    windowed = python.with_name("pythonw.exe")
    exe = windowed if windowed.exists() else python
    return f'"{exe}" "{config.ROOT / "main.py"}"'


def autostart_enabled() -> bool:
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, RUN_KEY) as key:
            value, _ = winreg.QueryValueEx(key, RUN_NAME)
            return bool(value)
    except OSError:
        return False


def set_autostart(enabled: bool) -> None:
    with winreg.OpenKey(winreg.HKEY_CURRENT_USER, RUN_KEY, 0, winreg.KEY_SET_VALUE) as key:
        if enabled:
            winreg.SetValueEx(key, RUN_NAME, 0, winreg.REG_SZ, autostart_command())
        else:
            try:
                winreg.DeleteValue(key, RUN_NAME)
            except OSError:
                pass


def acquire_single_instance() -> bool:
    kernel32 = ctypes.windll.kernel32
    kernel32.CreateMutexW.restype = ctypes.c_void_p
    handle = kernel32.CreateMutexW(None, False, MUTEX_NAME)
    if not handle:
        return True
    if kernel32.GetLastError() == 183:  # ERROR_ALREADY_EXISTS
        return False
    acquire_single_instance.handle = handle
    return True


class SttIslandApp:
    def __init__(self, qt_app: QApplication, cfg: dict, smoke: bool = False, preview: bool = False) -> None:
        self.qt = qt_app
        self.cfg = cfg
        self.smoke = smoke or preview
        self.preview = preview
        self.last_text = ""

        self.island = Island(cfg["accent"], pretty_key(cfg["hotkey"]))
        self.island.copy_requested.connect(self.copy_last)
        self.island.menu_requested.connect(self.show_menu)

        self.recorder = Recorder()
        self.transcriber = Transcriber(cfg["model"], cfg["language"], cfg["beam_size"])
        self.transcriber.loaded.connect(self._on_model_loaded)
        self.transcriber.load_failed.connect(self._on_model_failed)
        self.transcriber.finished.connect(self._on_transcript)
        self.transcriber.failed.connect(self._on_transcribe_failed)

        self.key = HoldKey(cfg["hotkey"])
        self.key.pressed.connect(self._on_press)
        self.key.released.connect(self._on_release)
        self.key.problem.connect(self.island.error)

        self.level_timer = QTimer()
        self.level_timer.setInterval(50)
        self.level_timer.timeout.connect(self._update_level)

        self.tray = QSystemTrayIcon(make_icon(cfg["accent"]))
        self.tray.setToolTip("STT Island")
        self.tray.activated.connect(self._on_tray_activated)
        self._build_menu()
        self.tray.show()

        self.qt.aboutToQuit.connect(self._shutdown)

    # ---------- lifecycle ----------

    def start(self) -> None:
        self.island.show_without_focus()
        if self.smoke:
            self._run_demo(self.preview)
            return
        self.key.start()
        self.island.status(f"Loading {self.cfg['model']} model…")
        self.transcriber.load_async()
        QTimer.singleShot(1200, lambda: self.tray.showMessage(
            "STT Island",
            f"Hold {pretty_key(self.cfg['hotkey'])} and speak. Release to paste.",
            QSystemTrayIcon.Information,
            3000,
        ))

    def _shutdown(self) -> None:
        self.key.stop()
        self.level_timer.stop()
        self.recorder.cancel()

    def quit(self) -> None:
        self.tray.hide()
        self.island.close()
        self.qt.quit()

    # ---------- menu ----------

    def _build_menu(self) -> None:
        menu = QMenu()
        menu.setStyleSheet(MENU_STYLE)
        label = pretty_key(self.cfg["hotkey"])
        header = menu.addAction(f"Hold {label} to dictate")
        header.setEnabled(False)
        menu.addSeparator()

        copy = menu.addAction("Copy last transcript")
        copy.triggered.connect(self.copy_last)
        menu.addAction("Settings…").triggered.connect(self.open_settings)
        menu.addSeparator()

        startup = menu.addAction("Start with Windows")
        startup.setCheckable(True)
        startup.setChecked(autostart_enabled())
        startup.triggered.connect(self._toggle_autostart)

        menu.addAction("Open config folder").triggered.connect(
            lambda: os.startfile(str(config.CONFIG_DIR))
        )
        menu.addSeparator()
        menu.addAction("Quit").triggered.connect(self.quit)
        self.menu = menu
        self.tray.setContextMenu(menu)

    def show_menu(self) -> None:
        self.menu.popup(self.island.mapToGlobal(self.island.rect().center()))

    def _on_tray_activated(self, reason) -> None:
        if reason == QSystemTrayIcon.DoubleClick:
            self.island.show_without_focus()

    def _toggle_autostart(self, checked: bool) -> None:
        try:
            set_autostart(checked)
        except OSError as exc:
            log.warning("autostart change failed: %s", exc)
            self.island.error("Could not change startup setting")

    def copy_last(self) -> None:
        if not self.last_text:
            self.island.error("Nothing to copy yet")
            return
        QGuiApplication.clipboard().setText(self.last_text)
        self.island.result(self.last_text, "Copied", linger_ms=1800)

    # ---------- recording ----------

    def _on_press(self) -> None:
        if self.recorder.active:
            return
        if not self.transcriber.ready:
            self.island.error("Model is still loading…")
            return
        if self.transcriber.busy:
            self.island.error("Still transcribing the last one…")
            return
        try:
            self.recorder.start()
        except AudioError as exc:
            log.error("%s", exc)
            self.island.error(str(exc))
            return
        self.island.listening()
        self.island.set_timer(0)
        self.level_timer.start()
        if self.cfg["play_sounds"]:
            sounds.start()

    def _update_level(self) -> None:
        if not self.recorder.active:
            self.level_timer.stop()
            return
        self.island.feed(self.recorder.level * 6.0)
        self.island.set_timer(self.recorder.elapsed())
        if self.recorder.elapsed() >= float(self.cfg["max_seconds"]):
            self._on_release()

    def _on_release(self) -> None:
        if not self.recorder.active:
            return
        elapsed = self.recorder.elapsed()
        audio = self.recorder.stop()
        self.level_timer.stop()
        if elapsed < float(self.cfg["min_seconds"]) or audio.size == 0:
            self.island.idle()
            return
        if self.cfg["play_sounds"]:
            sounds.stop()
        self.island.thinking()
        self.transcriber.transcribe(audio)

    # ---------- transcription ----------

    def _on_model_loaded(self, name: str) -> None:
        log.info("model ready: %s", name)
        self.tray.setToolTip(f"STT Island — {name} ready")
        if self.island.state == "status":
            self.island.idle()

    def _on_model_failed(self, message: str) -> None:
        log.error("model load failed: %s", message)
        self.island.error("Model failed to load — see config/stt-island.log")

    def _on_transcribe_failed(self, message: str) -> None:
        log.info("no transcript: %s", message)
        self.island.error(message or "Transcription failed")
        if self.cfg["play_sounds"]:
            sounds.error()

    def _on_transcript(self, text: str, took: float) -> None:
        self.last_text = text
        QGuiApplication.clipboard().setText(text)
        pasted = False
        if self.cfg["paste_into_app"]:
            try:
                if inject.foreground_window() != int(self.island.winId()):
                    inject.paste()
                    pasted = True
            except Exception:  # noqa: BLE001
                log.exception("paste failed")
        sub = f"{'Pasted' if pasted else 'Copied'} · {took:.1f}s"
        self.island.result(text, sub)
        if self.cfg["play_sounds"]:
            sounds.done()
        log.info("transcript in %.1fs: %s", took, text)

    # ---------- settings ----------

    def open_settings(self) -> None:
        dialog = SettingsDialog(self.cfg)
        dialog.saved.connect(self._apply_settings)
        dialog.exec()

    def _apply_settings(self, cfg: dict) -> None:
        previous = self.cfg
        if cfg["hotkey"] != previous["hotkey"]:
            wanted = cfg["hotkey"]
            if not self.key.rebind(wanted):
                cfg = dict(cfg, hotkey=previous["hotkey"])
                self.island.error(f"Unknown key '{wanted}'")
            else:
                self.island.set_key_label(pretty_key(wanted))
        self.cfg = cfg
        config.save(cfg)
        self.island.set_accent(cfg["accent"])
        self.transcriber.language = cfg["language"]
        self.transcriber.beam_size = int(cfg["beam_size"])
        if cfg["model"] != previous["model"]:
            self.island.status(f"Loading {cfg['model']} model…")
            self.transcriber.switch_model(cfg["model"])
        self._build_menu()

    # ---------- demo / smoke ----------

    def _run_demo(self, preview: bool) -> None:
        self.demo_started = time.monotonic()
        self.demo_schedule = (
            [(0.0, "idle"), (2.5, "listen"), (6.0, "think"), (8.0, "result"), (11.5, "quit")]
            if preview
            else [(0.0, "status"), (0.5, "listen"), (2.0, "think"), (2.9, "result"), (4.4, "quit")]
        )
        self.demo_index = 0
        self.demo_timer = QTimer()
        self.demo_timer.setInterval(40)
        self.demo_timer.timeout.connect(self._demo_tick)
        self.demo_timer.start()

    def _demo_tick(self) -> None:
        elapsed = time.monotonic() - self.demo_started
        while self.demo_index < len(self.demo_schedule) and elapsed >= self.demo_schedule[self.demo_index][0]:
            action = self.demo_schedule[self.demo_index][1]
            self.demo_index += 1
            if action == "status":
                self.island.status("Loading model…")
            elif action == "idle":
                self.island.idle()
            elif action == "listen":
                self.island.listening()
            elif action == "think":
                self.island.thinking()
            elif action == "result":
                self.island.result(
                    "Smoke test: the island grows, records and shows a transcript.",
                    "Copied · 0.1s",
                    linger_ms=60_000,
                )
            elif action == "quit":
                self.demo_timer.stop()
                print("[demo] ok: idle -> listen -> think -> result")
                self.qt.quit()
                return
        if self.island.state == "listen":
            self.island.feed(0.30 + 0.35 * abs(math.sin(elapsed * 2.4)))
            self.island.set_timer(elapsed)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Hold-to-talk speech to text island")
    parser.add_argument("--smoke", action="store_true", help="cycle the UI states quickly and exit")
    parser.add_argument("--preview", action="store_true", help="cycle the UI states slowly and exit")
    args = parser.parse_args(argv)

    config.setup_logging()
    if not acquire_single_instance():
        print("STT Island is already running.")
        return 0

    qt_app = QApplication(sys.argv[:1])
    qt_app.setApplicationName("STT Island")
    qt_app.setQuitOnLastWindowClosed(False)

    cfg = config.load()
    app = SttIslandApp(qt_app, cfg, smoke=args.smoke, preview=args.preview)
    qt_app.setWindowIcon(make_icon(cfg["accent"]))

    def handle_exception(exc_type, exc, tb):
        log.critical("unhandled", exc_info=(exc_type, exc, tb))
        app.island.error("Unexpected error — see the log")
        sys.__excepthook__(exc_type, exc, tb)

    sys.excepthook = handle_exception
    app.start()
    return qt_app.exec()


if __name__ == "__main__":
    raise SystemExit(main())
