"""The island: a pure-black capsule in ReUI's design language.

Design tokens are taken from ocopus-labs/ReUI's DESIGN.md / tokens.json:
black shape, no borders or shadows, one accent per state, Segoe UI Variable,
three levels of hierarchy. The compact pill is 37 px tall and grows sideways
only, so it reads as a status strip rather than a window.
"""
from __future__ import annotations

import logging
import math
from collections import deque

from PySide6.QtCore import (
    QEasingCurve,
    QPointF,
    QPropertyAnimation,
    QRect,
    QRectF,
    Qt,
    QTimer,
    Signal,
)
from PySide6.QtGui import QColor, QFont, QFontMetricsF, QPainter, QPen, QPolygonF
from PySide6.QtWidgets import QWidget

log = logging.getLogger(__name__)

HEIGHT = 37                 # island.height = 36.67
TOP_GAP = 11                # island.top
SIDE_MARGIN = 6.33          # concentric inset for a 24 circle
TEXT_MARGIN = 12
JOINED_GAP = 8
ART = 24                    # component.art.compact
BAR_COUNT = 24
BAR_W = 3.0                 # component.bars.width
BAR_GAP = 2.5               # component.bars.gap
BAR_MAX = 14                # component.bars.height

BLACK = QColor(0, 0, 0)
PRIMARY = QColor("#EBEBF0")
SECONDARY = QColor("#8E8D94")
TRACK = QColor(255, 255, 255, 41)      # fill.track: white @ 16%
RED = QColor("#FF453A")
GREEN = QColor("#30D158")

SIZES = {
    "listen": 250,
    "think": 196,
    "result": 430,
    "error": 420,
    "status": 250,
}


def _font(pixels: int, weight: QFont.Weight = QFont.Normal) -> QFont:
    font = QFont("Segoe UI Variable Text")
    font.setPixelSize(pixels)
    font.setWeight(weight)
    try:  # tabular figures, so timers don't jitter
        font.setFeature(QFont.Tag("tnum"), 1)
    except Exception:  # noqa: BLE001 - older Qt has no font features
        pass
    return font


def _tint(color: QColor, alpha: float) -> QColor:
    tinted = QColor(color)
    tinted.setAlphaF(alpha)
    return tinted


class Island(QWidget):
    copy_requested = Signal()
    menu_requested = Signal()

    def __init__(self, accent: str = "#0A84FF", key_label: str = "Right Ctrl", parent=None) -> None:
        super().__init__(parent)
        self.setWindowFlags(
            Qt.Tool
            | Qt.FramelessWindowHint
            | Qt.WindowStaysOnTopHint
            | Qt.WindowDoesNotAcceptFocus
            | Qt.NoDropShadowWindowHint
        )
        self.setAttribute(Qt.WA_TranslucentBackground)
        self.setAttribute(Qt.WA_ShowWithoutActivating)
        self.setCursor(Qt.PointingHandCursor)
        self.setMouseTracking(True)

        self._state = "idle"
        self._accent = QColor(accent)
        self._key_label = key_label
        self._text = ""
        self._sub = ""
        self._timer_text = "0:00"
        self._bars: deque[float] = deque([0.0] * BAR_COUNT, maxlen=BAR_COUNT)
        self._level = 0.0
        self._target_level = 0.0
        self._clock = 0.0
        self._drag_from = None
        self._dragged = False
        self._collapse_token = 0

        self._compact = _font(15, QFont.DemiBold)
        self._detail = _font(15)
        self._caption = _font(12, QFont.Medium)
        self._compact_metrics = QFontMetricsF(self._compact)
        self._caption_metrics = QFontMetricsF(self._caption)

        self._ticker = QTimer(self)
        self._ticker.setInterval(33)  # motion.bars.fps = 30
        self._ticker.timeout.connect(self._tick)

        self._anim = QPropertyAnimation(self, b"geometry", self)
        self._anim.setDuration(220)

        self.resize(self._window_size("idle")[0], HEIGHT)

    # ---------- public API ----------

    @property
    def state(self) -> str:
        return self._state

    def set_accent(self, accent: str) -> None:
        self._accent = QColor(accent)
        self.update()

    def set_key_label(self, label: str) -> None:
        self._key_label = label
        self._apply_size(self._state)

    def idle(self) -> None:
        self._text = ""
        self._sub = ""
        self._set_state("idle")

    def listening(self) -> None:
        self._bars = deque([0.0] * BAR_COUNT, maxlen=BAR_COUNT)
        self._level = 0.0
        self._set_state("listen")

    def thinking(self) -> None:
        self._set_state("think")

    def result(self, text: str, sub: str = "", linger_ms: int = 3600) -> None:
        self._text = text
        self._sub = sub
        self._set_state("result")
        self._collapse_token += 1
        token = self._collapse_token
        QTimer.singleShot(linger_ms, lambda: self._collapse(token))

    def error(self, message: str, linger_ms: int = 3200) -> None:
        self._text = message
        self._set_state("error")
        self._collapse_token += 1
        token = self._collapse_token
        QTimer.singleShot(linger_ms, lambda: self._collapse(token))

    def status(self, message: str) -> None:
        self._text = message
        self._set_state("status")

    def feed(self, level: float) -> None:
        self._target_level = max(0.0, min(1.0, level))

    def set_timer(self, seconds: float) -> None:
        total = int(seconds)
        self._timer_text = f"{total // 60}:{total % 60:02d}"

    def show_without_focus(self) -> None:
        self._place()
        self.show()
        self.raise_()

    # ---------- state and geometry ----------

    def _idle_width(self) -> int:
        return round(TEXT_MARGIN + 16 + JOINED_GAP + self._compact_metrics.horizontalAdvance(self._key_label) + TEXT_MARGIN)

    def _window_size(self, state: str) -> tuple[int, int]:
        width = self._idle_width() if state == "idle" else SIZES[state]
        return width, HEIGHT

    def _set_state(self, state: str) -> None:
        expanding = state != "idle"
        self._state = state
        self._apply_size(state, expanding)
        if state in ("listen", "think", "status"):
            if not self._ticker.isActive():
                self._ticker.start()
        else:
            self._ticker.stop()
        self.update()

    def _apply_size(self, state: str, expanding: bool = True) -> None:
        width, height = self._window_size(state)
        current = self.geometry()
        if not self.isVisible():
            self.resize(width, height)
            return
        target = QRect(current.center().x() - width // 2, current.top(), width, height)
        screen = self.screen().availableGeometry() if self.screen() else QRect()
        if not screen.isNull():
            target.moveLeft(max(screen.left() + 2, min(target.left(), screen.right() - width - 2)))
        self._anim.stop()
        self._anim.setDuration(220 if expanding else 180)
        self._anim.setEasingCurve(QEasingCurve.OutBack if expanding else QEasingCurve.OutCubic)
        self._anim.setStartValue(current)
        self._anim.setEndValue(target)
        self._anim.start()

    def _place(self) -> None:
        from PySide6.QtGui import QGuiApplication

        screen = QGuiApplication.primaryScreen() or self.screen()
        if screen is None:
            return
        area = screen.availableGeometry()
        if not self.isVisible() or self.pos().isNull():
            self.move(area.center().x() - self.width() // 2, area.top() + TOP_GAP)

    def _collapse(self, token: int) -> None:
        if token == self._collapse_token and self._state in ("result", "error"):
            self.idle()

    def _tick(self) -> None:
        self._clock += 0.033
        self._level += (self._target_level - self._level) * 0.30
        if self._state == "listen":
            self._bars.append(self._level)
        self.update()

    # ---------- painting ----------

    def paintEvent(self, _event) -> None:  # noqa: N802
        painter = QPainter(self)
        painter.setRenderHint(QPainter.Antialiasing, True)
        body = QRectF(self.rect())
        radius = body.height() / 2
        painter.setPen(Qt.NoPen)
        painter.setBrush(BLACK)
        painter.drawRoundedRect(body, radius, radius)

        if self._state == "idle":
            self._paint_idle(painter, body)
        elif self._state == "listen":
            self._paint_listen(painter, body)
        elif self._state == "think":
            self._paint_think(painter, body)
        elif self._state == "result":
            self._paint_result(painter, body)
        elif self._state == "error":
            self._paint_error(painter, body)
        else:
            self._paint_status(painter, body)

    def _paint_idle(self, painter: QPainter, body: QRectF) -> None:
        x = body.left() + TEXT_MARGIN
        self._paint_mic(painter, x, body.center().y(), 16, SECONDARY)
        x += 16 + JOINED_GAP
        painter.setFont(self._compact)
        painter.setPen(PRIMARY)
        painter.drawText(
            QRectF(x, body.top(), body.right() - TEXT_MARGIN - x, body.height()),
            Qt.AlignVCenter | Qt.AlignLeft,
            self._key_label,
        )

    def _paint_listen(self, painter: QPainter, body: QRectF) -> None:
        cx = body.left() + SIDE_MARGIN
        self._dot(painter, cx, body.center().y(), RED)

        timer_w = self._compact_metrics.horizontalAdvance(self._timer_text) + 2
        bars_left = cx + ART + TEXT_MARGIN
        bars_right = body.right() - TEXT_MARGIN - timer_w - 10
        self._paint_bars(painter, QRectF(bars_left, body.top(), max(10.0, bars_right - bars_left), body.height()))

        painter.setFont(self._compact)
        painter.setPen(PRIMARY)
        painter.drawText(
            QRectF(body.right() - TEXT_MARGIN - timer_w, body.top(), timer_w, body.height()),
            Qt.AlignVCenter | Qt.AlignRight,
            self._timer_text,
        )

    def _paint_bars(self, painter: QPainter, area: QRectF) -> None:
        values = list(self._bars)
        count = len(values)
        width = count * BAR_W + (count - 1) * BAR_GAP
        left = area.center().x() - width / 2
        center = area.center().y()
        painter.setPen(Qt.NoPen)
        painter.setBrush(RED)
        for index, value in enumerate(values):
            height = max(2.0, min(BAR_MAX, 2.0 + math.sqrt(value) * (BAR_MAX - 2.0)))
            rect = QRectF(left + index * (BAR_W + BAR_GAP), center - height / 2, BAR_W, height)
            painter.drawRoundedRect(rect, BAR_W / 2, BAR_W / 2)

    def _paint_think(self, painter: QPainter, body: QRectF) -> None:
        cx = body.left() + SIDE_MARGIN + ART / 2
        self._ring(painter, cx, body.center().y(), ART, self._accent)
        painter.setFont(self._detail)
        painter.setPen(SECONDARY)
        text_left = body.left() + SIDE_MARGIN + ART + TEXT_MARGIN
        painter.drawText(
            QRectF(text_left, body.top(), body.right() - TEXT_MARGIN - text_left, body.height()),
            Qt.AlignVCenter | Qt.AlignLeft,
            "Transcribing",
        )

    def _paint_result(self, painter: QPainter, body: QRectF) -> None:
        cx = body.left() + SIDE_MARGIN
        self._dot(painter, cx, body.center().y(), GREEN, glyph="check")

        sub_w = self._caption_metrics.horizontalAdvance(self._sub) if self._sub else 0
        text_left = cx + ART + TEXT_MARGIN
        text_right = body.right() - TEXT_MARGIN - (sub_w + 10 if sub_w else 0)
        painter.setFont(self._compact)
        painter.setPen(PRIMARY)
        line = self._compact_metrics.elidedText(self._text.replace("\n", " "), Qt.ElideRight, max(10.0, text_right - text_left))
        painter.drawText(
            QRectF(text_left, body.top(), max(10.0, text_right - text_left), body.height()),
            Qt.AlignVCenter | Qt.AlignLeft,
            line,
        )
        if sub_w:
            painter.setFont(self._caption)
            painter.setPen(SECONDARY)
            painter.drawText(
                QRectF(body.right() - TEXT_MARGIN - sub_w, body.top(), sub_w, body.height()),
                Qt.AlignVCenter | Qt.AlignRight,
                self._sub,
            )

    def _paint_error(self, painter: QPainter, body: QRectF) -> None:
        cx = body.left() + SIDE_MARGIN
        self._dot(painter, cx, body.center().y(), RED, glyph="cross")
        text_left = cx + ART + TEXT_MARGIN
        width = body.right() - TEXT_MARGIN - text_left
        painter.setFont(self._detail)
        painter.setPen(PRIMARY)
        line = QFontMetricsF(self._detail).elidedText(self._text, Qt.ElideRight, max(10.0, width))
        painter.drawText(QRectF(text_left, body.top(), width, body.height()), Qt.AlignVCenter | Qt.AlignLeft, line)

    def _paint_status(self, painter: QPainter, body: QRectF) -> None:
        cx = body.left() + SIDE_MARGIN + ART / 2
        self._ring(painter, cx, body.center().y(), ART, self._accent)
        painter.setFont(self._detail)
        painter.setPen(SECONDARY)
        text_left = body.left() + SIDE_MARGIN + ART + TEXT_MARGIN
        painter.drawText(
            QRectF(text_left, body.top(), body.right() - TEXT_MARGIN - text_left, body.height()),
            Qt.AlignVCenter | Qt.AlignLeft,
            self._text,
        )

    # ---------- marks ----------

    def _dot(self, painter: QPainter, x: float, cy: float, color: QColor, glyph: str = "") -> None:
        """A 24 circle in the accent's tint with an 8 dot (or a glyph) inside."""
        circle = QRectF(x, cy - ART / 2, ART, ART)
        painter.setPen(Qt.NoPen)
        painter.setBrush(_tint(color, 0.22))
        painter.drawEllipse(circle)
        if not glyph:
            painter.setBrush(color)
            painter.drawEllipse(QRectF(x + ART / 2 - 4, cy - 4, 8, 8))
            return
        painter.setBrush(Qt.NoBrush)
        painter.setPen(QPen(color, 2.0, Qt.SolidLine, Qt.RoundCap, Qt.RoundJoin))
        cx = x + ART / 2
        if glyph == "check":
            painter.drawPolyline(
                QPolygonF([QPointF(cx - 5, cy + 0.5), QPointF(cx - 1.5, cy + 4), QPointF(cx + 5.5, cy - 4)])
            )
        else:
            painter.drawLine(QPointF(cx - 4, cy - 4), QPointF(cx + 4, cy + 4))
            painter.drawLine(QPointF(cx + 4, cy - 4), QPointF(cx - 4, cy + 4))

    def _ring(self, painter: QPainter, cx: float, cy: float, size: float, color: QColor) -> None:
        rect = QRectF(cx - size / 2 + 1.5, cy - size / 2 + 1.5, size - 3, size - 3)
        painter.setBrush(Qt.NoBrush)
        painter.setPen(QPen(TRACK, 3, Qt.SolidLine, Qt.RoundCap))
        painter.drawEllipse(rect)
        painter.setPen(QPen(color, 3, Qt.SolidLine, Qt.RoundCap))
        start = int(-self._clock * 280) % 360
        painter.drawArc(rect, start * 16, 100 * 16)

    def _paint_mic(self, painter: QPainter, x: float, cy: float, size: float, color: QColor) -> None:
        scale = size / 16.0
        painter.setPen(Qt.NoPen)
        painter.setBrush(color)
        painter.drawRoundedRect(QRectF(x + 5 * scale, cy - 7.5 * scale, 6 * scale, 10 * scale), 3 * scale, 3 * scale)
        painter.setBrush(Qt.NoBrush)
        painter.setPen(QPen(color, 1.4 * scale, Qt.SolidLine, Qt.RoundCap))
        painter.drawArc(QRectF(x + 3 * scale, cy - 4.5 * scale, 10 * scale, 9 * scale), 200 * 16, 140 * 16)
        painter.drawLine(QPointF(x + 8 * scale, cy + 4.5 * scale), QPointF(x + 8 * scale, cy + 7.5 * scale))

    # ---------- mouse ----------

    def mousePressEvent(self, event) -> None:  # noqa: N802
        if event.button() == Qt.LeftButton:
            self._drag_from = event.globalPosition().toPoint() - self.frameGeometry().topLeft()
            self._dragged = False
        elif event.button() == Qt.RightButton:
            self.menu_requested.emit()
        event.accept()

    def mouseMoveEvent(self, event) -> None:  # noqa: N802
        if self._drag_from is not None and event.buttons() & Qt.LeftButton:
            new_pos = event.globalPosition().toPoint() - self._drag_from
            if (new_pos - self.pos()).manhattanLength() > 4:
                self._dragged = True
            self.move(new_pos)
        event.accept()

    def mouseReleaseEvent(self, event) -> None:  # noqa: N802
        if event.button() == Qt.LeftButton:
            if not self._dragged and self._state == "result":
                self.copy_requested.emit()
            self._drag_from = None
        event.accept()
