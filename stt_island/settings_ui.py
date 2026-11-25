"""A small dark settings dialog: hotkey, model, language, paste and sounds."""
from __future__ import annotations

from PySide6.QtCore import Qt, Signal
from PySide6.QtGui import QColor
from PySide6.QtWidgets import (
    QCheckBox,
    QColorDialog,
    QComboBox,
    QDialog,
    QDialogButtonBox,
    QFormLayout,
    QHBoxLayout,
    QLabel,
    QPushButton,
    QVBoxLayout,
    QWidget,
)

from .hotkey import pretty_key

MODELS = ["tiny", "base", "small", "medium", "large-v3", "distil-small.en", "distil-medium.en"]
LANGUAGES = [
    ("Auto detect", ""),
    ("English", "en"),
    ("French", "fr"),
    ("Arabic", "ar"),
    ("Spanish", "es"),
    ("German", "de"),
    ("Italian", "it"),
    ("Portuguese", "pt"),
    ("Dutch", "nl"),
    ("Russian", "ru"),
    ("Turkish", "tr"),
    ("Chinese", "zh"),
    ("Japanese", "ja"),
    ("Korean", "ko"),
]
KEYS = [
    "right ctrl",
    "left ctrl",
    "right alt",
    "caps lock",
    "right shift",
    "f8",
    "f9",
    "f10",
    "backtick",
    "scroll lock",
    "pause",
]

STYLE = """
QDialog { background: #101319; }
QLabel { color: #d7dce6; font-size: 12px; }
QComboBox, QLineEdit {
    background: #1a1f28; color: #eef2f8; border: 1px solid #2b3240;
    border-radius: 6px; padding: 5px 8px; min-height: 20px;
}
QComboBox QAbstractItemView { background: #1a1f28; color: #eef2f8; selection-background-color: #2b3240; }
QCheckBox { color: #d7dce6; font-size: 12px; spacing: 8px; }
QPushButton {
    background: #232a36; color: #eef2f8; border: 1px solid #313a4a;
    border-radius: 6px; padding: 6px 14px;
}
QPushButton:hover { background: #2c3545; }
QPushButton#accent { min-width: 56px; }
"""


class SettingsDialog(QDialog):
    saved = Signal(dict)

    def __init__(self, cfg: dict, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setWindowTitle("STT Island — Settings")
        self.setWindowFlag(Qt.WindowStaysOnTopHint, True)
        self.setStyleSheet(STYLE)
        self.setMinimumWidth(360)
        self._cfg = dict(cfg)
        self._accent = cfg.get("accent", "#5AC8FA")

        layout = QVBoxLayout(self)
        layout.setContentsMargins(20, 18, 20, 16)
        layout.setSpacing(14)

        title = QLabel("Settings")
        title.setStyleSheet("font-size: 16px; font-weight: 600; color: #ffffff;")
        layout.addWidget(title)

        form = QFormLayout()
        form.setSpacing(10)
        form.setLabelAlignment(Qt.AlignLeft)

        self.hotkey = QComboBox()
        self.hotkey.setEditable(True)
        self.hotkey.addItems(KEYS)
        self.hotkey.setCurrentText(pretty_key(cfg.get("hotkey", "right ctrl")).lower())
        self.hotkey.setToolTip("Any single key: right ctrl, caps lock, f8, backtick…")
        form.addRow("Hold to talk", self.hotkey)

        self.model = QComboBox()
        self.model.addItems(MODELS)
        self.model.setCurrentText(cfg.get("model", "small"))
        self.model.setToolTip("Bigger models are more accurate and slower. 'small' is a good default.")
        form.addRow("Whisper model", self.model)

        self.language = QComboBox()
        for label, code in LANGUAGES:
            self.language.addItem(label, code)
        index = max(0, self.language.findData(cfg.get("language", "")))
        self.language.setCurrentIndex(index)
        form.addRow("Language", self.language)

        accent_row = QHBoxLayout()
        self.accent_button = QPushButton()
        self.accent_button.setObjectName("accent")
        self.accent_button.setToolTip("Waveform colour")
        self.accent_button.clicked.connect(self._pick_accent)
        self._paint_accent()
        accent_row.addWidget(self.accent_button)
        accent_row.addStretch(1)
        holder = QWidget()
        holder.setLayout(accent_row)
        form.addRow("Accent", holder)

        layout.addLayout(form)

        self.paste = QCheckBox("Paste the transcript into the focused app")
        self.paste.setChecked(bool(cfg.get("paste_into_app", True)))
        layout.addWidget(self.paste)

        self.sounds = QCheckBox("Play sounds while recording")
        self.sounds.setChecked(bool(cfg.get("play_sounds", True)))
        layout.addWidget(self.sounds)

        hint = QLabel("The transcript is always copied to the clipboard.")
        hint.setStyleSheet("color: #7f8797; font-size: 11px;")
        layout.addWidget(hint)

        buttons = QDialogButtonBox(QDialogButtonBox.Save | QDialogButtonBox.Cancel)
        buttons.accepted.connect(self._save)
        buttons.rejected.connect(self.reject)
        layout.addWidget(buttons)

    def _paint_accent(self) -> None:
        self.accent_button.setText(self._accent.upper())
        self.accent_button.setStyleSheet(
            f"background: {self._accent}; color: #08111a; border: none; border-radius: 6px; padding: 6px 12px; font-weight: 600;"
        )

    def _pick_accent(self) -> None:
        color = QColorDialog.getColor(QColor(self._accent), self, "Accent colour")
        if color.isValid():
            self._accent = color.name()
            self._paint_accent()

    def _save(self) -> None:
        key = self.hotkey.currentText().strip().lower()
        if not key:
            self.hotkey.setFocus()
            return
        cfg = dict(self._cfg)
        cfg.update(
            {
                "hotkey": key,
                "model": self.model.currentText(),
                "language": self.language.currentData() or "",
                "accent": self._accent,
                "paste_into_app": self.paste.isChecked(),
                "play_sounds": self.sounds.isChecked(),
            }
        )
        self.saved.emit(cfg)
        self.accept()
