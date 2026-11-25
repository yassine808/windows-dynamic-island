"""The island itself: a frameless pill that grows into a recorder, then shows the transcript."""
from __future__ import annotations

import logging
import math
from collections import deque

from PySide6.QtCore import (
    QEasingCurve,
    QPropertyAnimation,
    QRect,
    QRectF,
    Qt,
    QTimer,
    Signal,
)
from PySide6.QtGui import QColor, QFont, QFontMetrics, QLinearGradient, QPainter, QPen
from PySide6.QtWidgets import QWidget

MARGIN = 18
BAR_COUNT = 30

log = logging.getLogger(__name__)

INK = QColor(242, 245, 249)
DIM = QColor(242, 245, 249, 158)
FAINT = QColor(242, 245, 249, 92)
LINE = QColor(255, 255, 255, 30)
RED = QColor(255, 69, 58)
GREEN = QColor(48, 209, 88)

SIZES = {
    "idle": (210, 34),
    "listen": (352, 58),
    "think": (312, 58),
    "result": (452, 78),
    "error": (416, 54),
    "status": (300, 54),
}


class Island(QWidget):
    copy_requested = Signal()
    menu_requested = Signal()

    def __init__(self, accent: str = "#5AC8FA", key_label: str = "Right Ctrl", parent=None) -> None:
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
        self._dots = 0
        self._drag_from = None
        self._dragged = False
        self._collapse_token = 0

        self._font = QFont("Segoe UI Variable Text")
        self._font.setPointSizeF(9.6)
        self._small = QFont(self._font)
        self._small.setPointSizeF(8.6)
        self._small_bold = QFont(self._small)
        self._small_bold.setWeight(QFont.DemiBold)
        self._mono = QFont("Cascadia Mono")
        self._mono.setPointSizeF(8.8)

        self._ticker = QTimer(self)
        self._ticker.setInterval(33)
        self._ticker.timeout.connect(self._tick)

        self._anim = QPropertyAnimation(self, b"geometry", self)
        self._anim.setDuration(200)
        self._anim.setEasingCurve(QEasingCurve.OutCubic)

        self.resize(*self._window_size("idle"))

    # ---------- public API ----------

    @property
    def state(self) -> str:
        return self._state

    def set_accent(self, accent: str) -> None:
        self._accent = QColor(accent)
        self.update()

    def set_key_label(self, label: str) -> None:
        self._key_label = label
        self._apply_size("idle" if self._state == "idle" else self._state)

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

    def _window_size(self, state: str) -> tuple[int, int]:
        width, height = SIZES[state]
        if state == "idle":
            width = self._idle_width()
        return width + MARGIN * 2, height + MARGIN * 2

    def _idle_width(self) -> int:
        fm = QFontMetrics(self._small)
        chip = fm.horizontalAdvance(self._key_label) + 18
        return 16 + 16 + 8 + fm.horizontalAdvance("Hold") + 8 + chip + 16

    def _set_state(self, state: str) -> None:
        self._state = state
        self._apply_size(state)
        if state in ("listen", "think", "status"):
            if not self._ticker.isActive():
                self._ticker.start()
        else:
            self._ticker.stop()
        self.update()

    def _apply_size(self, state: str) -> None:
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
        self._anim.setStartValue(current)
        self._anim.setEndValue(target)
        self._anim.start()

    def _place(self) -> None:
        from PySide6.QtGui import QGuiApplication

        screen = QGuiApplication.primaryScreen() or self.screen()
        if screen is None:
            return
        area = screen.availableGeometry()
        width, height = self.width(), self.height()
        if not self.isVisible() or self.pos().isNull():
            self.move(area.center().x() - width // 2, area.top() + 4)
        log.info(
            "island at %s, screen %s (available %s, dpr %s)",
            self.geometry(),
            screen.geometry(),
            area,
            screen.devicePixelRatio(),
        )

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
        body = QRectF(self.rect()).adjusted(MARGIN, MARGIN, -MARGIN, -MARGIN)
        if body.width() < 10 or body.height() < 10:
            return
        self._paint_shadow(painter, body)
        self._paint_body(painter, body)
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

    def _paint_shadow(self, painter: QPainter, body: QRectF) -> None:
        painter.setBrush(Qt.NoBrush)
        for step in range(10, 0, -1):
            alpha = int(26 * (1 - step / 11.0) ** 2)
            painter.setPen(QPen(QColor(0, 0, 0, alpha), 2))
            painter.drawRoundedRect(
                body.adjusted(-step, -step * 0.9, step, step * 1.1),
                body.height() / 2 + step,
                body.height() / 2 + step,
            )

    def _paint_body(self, painter: QPainter, body: QRectF) -> None:
        radius = body.height() / 2
        gradient = QLinearGradient(body.topLeft(), body.bottomLeft())
        gradient.setColorAt(0.0, QColor(26, 30, 39, 246))
        gradient.setColorAt(1.0, QColor(10, 12, 17, 246))
        painter.setBrush(gradient)
        if self._state in ("listen", "think", "status"):
            glow = 0.5 + 0.5 * math.sin(self._clock * 4.0)
            painter.setPen(QPen(QColor(self._accent.red(), self._accent.green(), self._accent.blue(), 110 + int(60 * glow)), 1.2))
        elif self._state == "error":
            painter.setPen(QPen(QColor(RED.red(), RED.green(), RED.blue(), 150), 1.2))
        else:
            painter.setPen(QPen(LINE, 1.0))
        painter.drawRoundedRect(body, radius, radius)

    def _paint_idle(self, painter: QPainter, body: QRectF) -> None:
        x = body.left() + 16
        self._paint_mic(painter, x, body.center().y(), FAINT)
        x += 16 + 8
        painter.setFont(self._small)
        painter.setPen(QPen(DIM))
        hold = "Hold"
        painter.drawText(QRectF(x, body.top(), 60, body.height()), Qt.AlignVCenter | Qt.AlignLeft, hold)
        x += QFontMetrics(self._small).horizontalAdvance(hold) + 8
        fm = QFontMetrics(self._small)
        chip_w = fm.horizontalAdvance(self._key_label) + 18
        chip = QRectF(x, body.center().y() - 10, chip_w, 20)
        painter.setPen(Qt.NoPen)
        painter.setBrush(QColor(255, 255, 255, 18))
        painter.drawRoundedRect(chip, 10, 10)
        painter.setFont(self._small_bold)
        painter.setPen(QPen(QColor(INK.red(), INK.green(), INK.blue(), 215)))
        painter.drawText(chip, Qt.AlignCenter, self._key_label)

    def _paint_listen(self, painter: QPainter, body: QRectF) -> None:
        pulse = 0.5 + 0.5 * math.sin(self._clock * 7.0)
        dot = QRectF(body.left() + 15, body.center().y() - 4.5, 9, 9)
        painter.setPen(Qt.NoPen)
        painter.setBrush(QColor(RED.red(), RED.green(), RED.blue(), 60 + int(80 * pulse)))
        painter.drawEllipse(dot.adjusted(-4, -4, 4, 4))
        painter.setBrush(RED)
        painter.drawEllipse(dot)

        right = body.right() - 16
        painter.setFont(self._mono)
        painter.setPen(QPen(DIM))
        timer_w = 44
        painter.drawText(QRectF(right - timer_w, body.top(), timer_w, body.height()), Qt.AlignVCenter | Qt.AlignRight, self._timer_text)

        bars_left = dot.right() + 14
        bars_right = right - timer_w - 14
        self._paint_bars(painter, QRectF(bars_left, body.top() + 10, bars_right - bars_left, body.height() - 20))

    def _paint_bars(self, painter: QPainter, area: QRectF) -> None:
        count = BAR_COUNT
        gap = 3.0
        bar_w = max(2.0, (area.width() - gap * (count - 1)) / count)
        values = list(self._bars)[-count:]
        values = [0.0] * (count - len(values)) + values
        center = area.center().y()
        max_h = area.height()
        for index, value in enumerate(values):
            height = max(2.5, min(max_h, 3.0 + math.sqrt(value) * (max_h - 3.0)))
            x = area.left() + index * (bar_w + gap)
            rect = QRectF(x, center - height / 2, bar_w, height)
            alpha = 120 + int(110 * (index / max(1, count - 1)))
            color = QColor(self._accent)
            color.setAlpha(alpha)
            painter.setPen(Qt.NoPen)
            painter.setBrush(color)
            painter.drawRoundedRect(rect, bar_w / 2, bar_w / 2)

    def _paint_think(self, painter: QPainter, body: QRectF) -> None:
        cx = body.left() + 28
        cy = body.center().y()
        painter.setBrush(Qt.NoBrush)
        painter.setPen(QPen(QColor(255, 255, 255, 40), 2.4))
        arc = QRectF(cx - 8, cy - 8, 16, 16)
        painter.drawEllipse(arc)
        painter.setPen(QPen(self._accent, 2.4))
        start = int(-self._clock * 300) % 360
        painter.drawArc(arc, start * 16, 100 * 16)

        dots = "." * (1 + int(self._clock * 2.2) % 3)
        painter.setFont(self._font)
        painter.setPen(QPen(INK))
        painter.drawText(
            QRectF(cx + 20, body.top(), body.width() - 50, body.height()),
            Qt.AlignVCenter | Qt.AlignLeft,
            f"Transcribing{dots}",
        )

    def _paint_result(self, painter: QPainter, body: QRectF) -> None:
        fm = QFontMetrics(self._font)
        x = body.left() + 16
        painter.setPen(Qt.NoPen)
        painter.setBrush(QColor(GREEN.red(), GREEN.green(), GREEN.blue(), 40))
        painter.drawEllipse(QRectF(x, body.center().y() - 9, 18, 18))
        painter.setPen(QPen(GREEN, 1.8))
        painter.drawLine(x + 5, body.center().y(), x + 8, body.center().y() + 4)
        painter.drawLine(x + 8, body.center().y() + 4, x + 13, body.center().y() - 3)

        text_left = x + 18 + 12
        width = body.right() - 16 - text_left
        painter.setFont(self._font)
        painter.setPen(QPen(INK))
        line = fm.elidedText(self._text.replace("\n", " "), Qt.ElideRight, int(width))
        painter.drawText(QRectF(text_left, body.top() + 15, width, 22), Qt.AlignLeft | Qt.AlignVCenter, line)
        if self._sub:
            painter.setFont(self._small)
            painter.setPen(QPen(DIM))
            painter.drawText(QRectF(text_left, body.top() + 37, width, 18), Qt.AlignLeft | Qt.AlignVCenter, self._sub)

    def _paint_error(self, painter: QPainter, body: QRectF) -> None:
        x = body.left() + 18
        painter.setPen(QPen(RED, 1.8))
        painter.setBrush(Qt.NoBrush)
        painter.drawEllipse(QRectF(x, body.center().y() - 8, 16, 16))
        painter.drawLine(x + 5, body.center().y() - 3, x + 11, body.center().y() + 3)
        painter.drawLine(x + 11, body.center().y() - 3, x + 5, body.center().y() + 3)
        painter.setFont(self._font)
        painter.setPen(QPen(QColor(INK.red(), INK.green(), INK.blue(), 230)))
        text_left = x + 26
        width = body.right() - 16 - text_left
        line = QFontMetrics(self._font).elidedText(self._text, Qt.ElideRight, int(width))
        painter.drawText(QRectF(text_left, body.top(), width, body.height()), Qt.AlignVCenter | Qt.AlignLeft, line)

    def _paint_status(self, painter: QPainter, body: QRectF) -> None:
        cx = body.left() + 28
        cy = body.center().y()
        painter.setBrush(Qt.NoBrush)
        painter.setPen(QPen(QColor(255, 255, 255, 36), 2.2))
        arc = QRectF(cx - 7, cy - 7, 14, 14)
        painter.drawEllipse(arc)
        painter.setPen(QPen(self._accent, 2.2))
        start = int(-self._clock * 260) % 360
        painter.drawArc(arc, start * 16, 90 * 16)
        painter.setFont(self._small)
        painter.setPen(QPen(DIM))
        painter.drawText(
            QRectF(cx + 18, body.top(), body.width() - 46, body.height()),
            Qt.AlignVCenter | Qt.AlignLeft,
            self._text,
        )

    def _paint_mic(self, painter: QPainter, x: float, cy: float, color: QColor) -> None:
        painter.setPen(Qt.NoPen)
        painter.setBrush(color)
        painter.drawRoundedRect(QRectF(x + 4, cy - 8, 7, 11), 3.5, 3.5)
        painter.setBrush(Qt.NoBrush)
        painter.setPen(QPen(color, 1.3))
        painter.drawArc(QRectF(x + 2, cy - 5, 11, 9), 200 * 16, 140 * 16)
        painter.drawLine(x + 7.5, cy + 4, x + 7.5, cy + 7)

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
