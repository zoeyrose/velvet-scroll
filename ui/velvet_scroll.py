#!/usr/bin/env python3
"""Native settings and tray UI for Velvet Scroll.

The UI is deliberately an unprivileged client of the velvet-scroll daemon.  All
device work remains in the Rust process; this module only runs its public CLI.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shutil
import sys
from typing import Any, Callable

try:
    from PyQt6.QtCore import QProcess, QProcessEnvironment, QSize, Qt, QTimer, pyqtSignal
    from PyQt6.QtGui import QAction, QColor, QDesktopServices, QFont, QIcon, QPainter, QPixmap
    from PyQt6.QtWidgets import (
        QApplication,
        QCheckBox,
        QFrame,
        QHBoxLayout,
        QLabel,
        QMainWindow,
        QMenu,
        QPushButton,
        QScrollArea,
        QSizePolicy,
        QSlider,
        QStyle,
        QSystemTrayIcon,
        QVBoxLayout,
        QWidget,
    )
except ImportError as exc:  # pragma: no cover - only exercised on unprepared systems
    raise SystemExit(
        "Velvet Scroll's settings need PyQt6. Install your distro's python3-pyqt6 package."
    ) from exc


APP_NAME = "Velvet Scroll"
APP_ID = "io.github.zoeyrose.VelvetScroll"
PROJECT_ROOT = Path(__file__).resolve().parent.parent
INSTALL_PREFIX = Path(__file__).resolve().parents[2]


def find_binary() -> str | None:
    """Find the daemon CLI, preferring the explicit environment override."""
    explicit = os.environ.get("VELVET_SCROLL_BIN")
    candidates = [
        explicit,
        str(PROJECT_ROOT / "target" / "release" / "velvet-scroll"),
        str(PROJECT_ROOT / "target" / "debug" / "velvet-scroll"),
        shutil.which("velvet-scroll"),
        str(INSTALL_PREFIX / "bin" / "velvet-scroll"),
        "/usr/local/bin/velvet-scroll",
        "/usr/bin/velvet-scroll",
    ]
    for candidate in candidates:
        if candidate and Path(candidate).is_file() and os.access(candidate, os.X_OK):
            return str(Path(candidate).resolve())
    return None


def find_icon() -> QIcon:
    candidates = [
        os.environ.get("VELVET_SCROLL_ICON"),
        str(PROJECT_ROOT / "assets" / "velvet-scroll.svg"),
        str(INSTALL_PREFIX / "share" / "icons" / "hicolor" / "scalable" / "apps" / "velvet-scroll.svg"),
        "/usr/local/share/icons/hicolor/scalable/apps/velvet-scroll.svg",
        "/usr/share/icons/hicolor/scalable/apps/velvet-scroll.svg",
    ]
    for candidate in candidates:
        if candidate and Path(candidate).is_file():
            return QIcon(candidate)
    themed = QIcon.fromTheme("input-mouse")
    if not themed.isNull():
        return themed
    return QApplication.style().standardIcon(QStyle.StandardPixmap.SP_ComputerIcon)


def error_message(raw: str) -> str:
    text = raw.strip()
    try:
        decoded = json.loads(text)
        text = str(decoded.get("error", text)) if isinstance(decoded, dict) else text
    except (json.JSONDecodeError, TypeError):
        pass
    lower = text.lower()
    if "calibration was not inherited" in lower:
        return "Update Velvet Scroll’s package, reload udev rules, then restart the scrolling service."
    if "permission" in lower or "denied" in lower or "udev" in lower:
        return "Input access was denied. Install Velvet Scroll’s udev rule, then sign out and back in."
    if "connect" in lower or "socket" in lower or "running" in lower:
        return "The scrolling service isn’t running yet. Start it below and try again."
    return text or "Velvet Scroll couldn’t reach its scrolling service."


class CommandClient(QWidget):
    """Small asynchronous adapter around the velvet-scroll CLI."""

    status_received = pyqtSignal(dict)
    status_failed = pyqtSignal(str)
    command_finished = pyqtSignal(str, bool, str)

    def __init__(
        self,
        binary: str | None = None,
        parent: QWidget | None = None,
        environment: dict[str, str] | None = None,
    ) -> None:
        super().__init__(parent)
        self.binary = binary or find_binary()
        self.environment = environment or {}
        self._processes: set[QProcess] = set()
        self._status_pending = False

    def refresh(self) -> None:
        if self._status_pending:
            return
        if not self.binary:
            self.status_failed.emit(
                "The velvet-scroll command wasn’t found. Install Velvet Scroll or set VELVET_SCROLL_BIN."
            )
            return
        self._status_pending = True
        self._run(["status", "--json"], "status", self._handle_status)

    def command(self, args: list[str], label: str) -> None:
        if not self.binary:
            self.command_finished.emit(label, False, "The velvet-scroll command wasn’t found.")
            return
        self._run(args, label, lambda code, out, err: self._handle_command(label, code, out, err))

    def start_daemon(self) -> tuple[bool, str]:
        if not self.binary:
            return False, "The velvet-scroll command wasn’t found."
        proc = QProcess()
        proc.setProgram(self.binary)
        proc.setArguments(["daemon"])
        proc.setProcessEnvironment(self._process_environment())
        started, _pid = proc.startDetached()
        return bool(started), "" if started else "The scrolling service could not be started."

    def _process_environment(self) -> QProcessEnvironment:
        environment = QProcessEnvironment.systemEnvironment()
        for name, value in self.environment.items():
            environment.insert(name, value)
        return environment

    def _run(
        self,
        args: list[str],
        label: str,
        callback: Callable[[int, str, str], None],
    ) -> None:
        assert self.binary is not None
        proc = QProcess(self)
        proc.setProcessChannelMode(QProcess.ProcessChannelMode.SeparateChannels)
        proc.setProcessEnvironment(self._process_environment())
        self._processes.add(proc)
        finished = False

        def complete(exit_code: int = -1) -> None:
            nonlocal finished
            if finished:
                return
            finished = True
            stdout = bytes(proc.readAllStandardOutput()).decode("utf-8", "replace").strip()
            stderr = bytes(proc.readAllStandardError()).decode("utf-8", "replace").strip()
            self._processes.discard(proc)
            proc.deleteLater()
            callback(exit_code, stdout, stderr)

        def failed(kind: QProcess.ProcessError) -> None:
            if kind == QProcess.ProcessError.FailedToStart:
                complete(-1)

        def timed_out() -> None:
            if not finished and proc.state() != QProcess.ProcessState.NotRunning:
                proc.kill()

        proc.finished.connect(lambda code, _status: complete(code))
        proc.errorOccurred.connect(failed)
        proc.start(self.binary, args)
        QTimer.singleShot(5000, timed_out)

    def _handle_status(self, code: int, stdout: str, stderr: str) -> None:
        self._status_pending = False
        candidate = stdout or stderr
        try:
            payload = json.loads(candidate)
            if not isinstance(payload, dict):
                raise ValueError("status is not an object")
        except (json.JSONDecodeError, ValueError):
            self.status_failed.emit(error_message(candidate or "The status response was not valid JSON."))
            return
        if payload.get("running") is False:
            self.status_failed.emit(error_message(str(payload.get("error", "Service unavailable"))))
            return
        if code != 0:
            self.status_failed.emit(error_message(str(payload.get("error", candidate))))
            return
        self.status_received.emit(payload)

    def _handle_command(self, label: str, code: int, stdout: str, stderr: str) -> None:
        message = stdout if code == 0 else error_message(stderr or stdout)
        self.command_finished.emit(label, code == 0, message)
        QTimer.singleShot(80, self.refresh)


class SettingsWindow(QMainWindow):
    def __init__(self, client: CommandClient, icon: QIcon, screenshot_mode: bool = False) -> None:
        super().__init__()
        self.client = client
        self.app_icon = icon
        self.screenshot_mode = screenshot_mode
        self._updating = False
        self._daemon_online = False
        self._tray_available = False
        self._device_checks: list[QCheckBox] = []

        self.setWindowTitle(APP_NAME)
        self.setWindowIcon(icon)
        self.setMinimumSize(480, 570)
        self.resize(500, 610)
        self._build_ui()
        self._build_tray()
        self._set_controls_enabled(False)
        self.accel_timer = QTimer(self)
        self.accel_timer.setSingleShot(True)
        self.accel_timer.setInterval(280)
        self.accel_timer.timeout.connect(self._set_acceleration)
        self._connect()

        self.poll_timer = QTimer(self)
        self.poll_timer.setInterval(3000)
        self.poll_timer.timeout.connect(self.client.refresh)

    def _build_ui(self) -> None:
        root = QWidget()
        root.setObjectName("root")
        outer = QVBoxLayout(root)
        outer.setContentsMargins(24, 22, 24, 22)
        outer.setSpacing(15)

        header = QHBoxLayout()
        icon_label = QLabel()
        icon_label.setPixmap(self.app_icon.pixmap(QSize(54, 54)))
        icon_label.setFixedSize(58, 58)
        titles = QVBoxLayout()
        titles.setSpacing(1)
        title = QLabel(APP_NAME)
        title.setObjectName("title")
        tagline = QLabel("A little magic for your mouse wheel")
        tagline.setObjectName("muted")
        titles.addWidget(title)
        titles.addWidget(tagline)
        header.addWidget(icon_label)
        header.addLayout(titles, 1)
        self.status_pill = QLabel("Checking…")
        self.status_pill.setObjectName("statusPill")
        self.status_pill.setAlignment(Qt.AlignmentFlag.AlignCenter)
        header.addWidget(self.status_pill)
        outer.addLayout(header)

        self.notice = QFrame()
        self.notice.setObjectName("notice")
        notice_layout = QHBoxLayout(self.notice)
        notice_layout.setContentsMargins(14, 10, 10, 10)
        notice_layout.setSpacing(10)
        self.notice_text = QLabel("Connecting to the scrolling service…")
        self.notice_text.setWordWrap(True)
        self.notice_text.setObjectName("noticeText")
        self.start_button = QPushButton("Start service")
        self.start_button.setObjectName("quietButton")
        self.start_button.hide()
        notice_layout.addWidget(self.notice_text, 1)
        notice_layout.addWidget(self.start_button)
        outer.addWidget(self.notice)

        magic = self._card("Scrolling")
        magic_layout = magic.layout()
        toggle_row = QHBoxLayout()
        toggle_words = QVBoxLayout()
        toggle_words.setSpacing(2)
        enable_title = QLabel("Wheel acceleration")
        enable_title.setObjectName("settingTitle")
        enable_help = QLabel("Slow turns stay precise; quick spins travel farther.")
        enable_help.setObjectName("muted")
        enable_help.setWordWrap(True)
        toggle_words.addWidget(enable_title)
        toggle_words.addWidget(enable_help)
        self.enabled_check = QCheckBox()
        self.enabled_check.setObjectName("switch")
        self.enabled_check.setAccessibleName("Enable wheel acceleration")
        toggle_row.addLayout(toggle_words, 1)
        toggle_row.addWidget(self.enabled_check)
        magic_layout.addLayout(toggle_row)
        magic_layout.addWidget(self._divider())

        accel_row = QHBoxLayout()
        accel_label = QLabel("Acceleration")
        accel_label.setObjectName("settingTitle")
        self.accel_value = QLabel("3.0×")
        self.accel_value.setObjectName("valuePill")
        accel_row.addWidget(accel_label)
        accel_row.addStretch()
        accel_row.addWidget(self.accel_value)
        magic_layout.addLayout(accel_row)
        self.accel_slider = QSlider(Qt.Orientation.Horizontal)
        self.accel_slider.setRange(10, 80)
        self.accel_slider.setSingleStep(1)
        self.accel_slider.setPageStep(5)
        self.accel_slider.setValue(30)
        self.accel_slider.setAccessibleName("Acceleration amount")
        magic_layout.addWidget(self.accel_slider)

        self.coast_check = QCheckBox("Keep gliding after a quick spin")
        self.coast_check.setObjectName("coastCheck")
        self.coast_check.setToolTip("Adds a brief, smoothly fading coast after fast wheel movement")
        magic_layout.addWidget(self.coast_check)
        outer.addWidget(magic)

        devices = self._card("Mouse devices")
        device_layout = devices.layout()
        device_intro = QLabel("Choose which mice get the sparkle. New devices appear automatically.")
        device_intro.setObjectName("muted")
        device_intro.setWordWrap(True)
        device_layout.addWidget(device_intro)
        self.devices_container = QWidget()
        self.devices_layout = QVBoxLayout(self.devices_container)
        self.devices_layout.setContentsMargins(0, 5, 0, 0)
        self.devices_layout.setSpacing(4)
        self.empty_devices = QLabel("No scroll wheels found")
        self.empty_devices.setObjectName("empty")
        self.devices_layout.addWidget(self.empty_devices)
        scroll = QScrollArea()
        scroll.setObjectName("deviceScroll")
        scroll.setFrameShape(QFrame.Shape.NoFrame)
        scroll.setWidgetResizable(True)
        scroll.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        scroll.setMinimumHeight(82)
        scroll.setMaximumHeight(142)
        scroll.setWidget(self.devices_container)
        device_layout.addWidget(scroll)
        outer.addWidget(devices, 1)

        footer = QHBoxLayout()
        self.refresh_button = QPushButton("Refresh")
        self.refresh_button.setObjectName("quietButton")
        footer.addWidget(self.refresh_button)
        footer.addStretch()
        self.tray_hint = QLabel("Closing keeps Velvet Scroll in the tray")
        self.tray_hint.setObjectName("tinyMuted")
        footer.addWidget(self.tray_hint)
        outer.addLayout(footer)

        self.setCentralWidget(root)
        self.setStyleSheet(STYLE)

    def _card(self, heading: str) -> QFrame:
        card = QFrame()
        card.setObjectName("card")
        layout = QVBoxLayout(card)
        layout.setContentsMargins(18, 15, 18, 17)
        layout.setSpacing(11)
        label = QLabel(heading)
        label.setObjectName("section")
        layout.addWidget(label)
        return card

    @staticmethod
    def _divider() -> QFrame:
        line = QFrame()
        line.setObjectName("divider")
        line.setFrameShape(QFrame.Shape.HLine)
        return line

    def _build_tray(self) -> None:
        self.tray = QSystemTrayIcon(self.app_icon, self)
        menu = QMenu()
        self.tray_toggle = QAction("Enable Velvet Scroll", self)
        self.tray_toggle.setCheckable(True)
        menu.addAction(self.tray_toggle)
        menu.addSeparator()
        open_action = QAction("Open settings", self)
        open_action.triggered.connect(self.bring_forward)
        menu.addAction(open_action)
        refresh_action = QAction("Refresh status", self)
        refresh_action.triggered.connect(self.client.refresh)
        menu.addAction(refresh_action)
        menu.addSeparator()
        quit_action = QAction("Quit settings", self)
        quit_action.triggered.connect(QApplication.instance().quit)
        menu.addAction(quit_action)
        self.tray.setContextMenu(menu)
        menu.aboutToShow.connect(self.client.refresh)
        self.tray.setToolTip(APP_NAME)
        self.tray.activated.connect(self._tray_activated)
        self._tray_available = QSystemTrayIcon.isSystemTrayAvailable() and not self.screenshot_mode
        if self._tray_available:
            self.tray.show()
        else:
            self.tray_hint.setText("Closing leaves the scrolling service running")

    def _connect(self) -> None:
        self.client.status_received.connect(self.apply_status)
        self.client.status_failed.connect(self.show_offline)
        self.client.command_finished.connect(self._command_finished)
        self.enabled_check.clicked.connect(self._set_enabled)
        self.tray_toggle.triggered.connect(self._set_enabled)
        self.accel_slider.valueChanged.connect(self._acceleration_changed)
        self.accel_slider.sliderReleased.connect(self._set_acceleration)
        self.coast_check.clicked.connect(self._set_coast)
        self.refresh_button.clicked.connect(self.client.refresh)
        self.start_button.clicked.connect(self._start_daemon)

    def showEvent(self, event: Any) -> None:
        super().showEvent(event)
        if not self.screenshot_mode:
            self.client.refresh()
            self.poll_timer.start()

    def hideEvent(self, event: Any) -> None:
        self.poll_timer.stop()
        super().hideEvent(event)

    def closeEvent(self, event: Any) -> None:
        if self._tray_available:
            event.ignore()
            self.hide()
            self.tray.showMessage(
                APP_NAME,
                "Still adding a little scroll magic. Use the tray icon to reopen settings.",
                QSystemTrayIcon.MessageIcon.Information,
                2200,
            )
        else:
            event.accept()
            QApplication.instance().quit()

    def bring_forward(self) -> None:
        self.show()
        self.raise_()
        self.activateWindow()

    def _tray_activated(self, reason: QSystemTrayIcon.ActivationReason) -> None:
        if reason in (
            QSystemTrayIcon.ActivationReason.Trigger,
            QSystemTrayIcon.ActivationReason.DoubleClick,
        ):
            self.bring_forward()

    def _set_controls_enabled(self, enabled: bool) -> None:
        self.enabled_check.setEnabled(enabled)
        self.accel_slider.setEnabled(enabled)
        self.coast_check.setEnabled(enabled)
        self.tray_toggle.setEnabled(enabled)
        for check in self._device_checks:
            check.setEnabled(enabled)

    def _set_enabled(self, checked: bool) -> None:
        if self._updating:
            return
        command = "enable" if checked else "disable"
        self.client.command([command], command)

    def _set_acceleration(self) -> None:
        self.accel_timer.stop()
        if not self._updating:
            value = self.accel_slider.value() / 10
            self.client.command(["set", "acceleration", f"{value:.1f}"], "acceleration")

    def _acceleration_changed(self, value: int) -> None:
        self.accel_value.setText(f"{value / 10:.1f}×")
        if not self._updating and not self.accel_slider.isSliderDown():
            self.accel_timer.start()

    def _set_coast(self, checked: bool) -> None:
        if not self._updating:
            self.client.command(["set", "coast", "on" if checked else "off"], "coast")

    def _set_device(self, device_id: str, checked: bool) -> None:
        if not self._updating:
            self.client.command(["select", device_id, "on" if checked else "off"], "device")

    def _start_daemon(self) -> None:
        self.start_button.setEnabled(False)
        self.notice_text.setText("Starting Velvet Scroll…")
        started, message = self.client.start_daemon()
        if not started:
            self.show_offline(message)
            return
        QTimer.singleShot(450, self.client.refresh)
        QTimer.singleShot(1400, self.client.refresh)

    def apply_status(self, status: dict[str, Any]) -> None:
        self._daemon_online = True
        self._updating = True
        enabled = bool(status.get("enabled", False))
        acceleration = min(8.0, max(1.0, float(status.get("acceleration", 3.0))))
        coast = bool(status.get("coast", False))
        self.enabled_check.setChecked(enabled)
        self.tray_toggle.setChecked(enabled)
        self.accel_slider.setValue(round(acceleration * 10))
        self.coast_check.setChecked(coast)
        self._replace_devices(status.get("devices", []))
        self._updating = False
        self._set_controls_enabled(True)
        reported_errors = status.get("errors", [])
        errors = [str(item) for item in reported_errors] if isinstance(reported_errors, list) else []
        if status.get("error"):
            errors.append(str(status["error"]))
        if errors:
            friendly = [error_message(item) for item in errors]
            self.notice_text.setText("\n".join(friendly[:3]))
            self.start_button.hide()
            self.notice.show()
            self.status_pill.setText("Needs attention")
            self.status_pill.setProperty("online", "warning")
            suffix = "needs attention"
        else:
            self.notice.hide()
            self.status_pill.setText("On" if enabled else "Paused")
            self.status_pill.setProperty("online", "true" if enabled else "paused")
            suffix = "on" if enabled else "paused"
        self.status_pill.style().unpolish(self.status_pill)
        self.status_pill.style().polish(self.status_pill)
        self.start_button.setEnabled(True)
        self.tray.setToolTip(f"{APP_NAME} · {suffix}")

    def show_offline(self, message: str) -> None:
        self._daemon_online = False
        self._set_controls_enabled(False)
        self.status_pill.setText("Offline")
        self.status_pill.setProperty("online", "false")
        self.status_pill.style().unpolish(self.status_pill)
        self.status_pill.style().polish(self.status_pill)
        self.notice_text.setText(error_message(message))
        self.start_button.setVisible(self.client.binary is not None)
        self.start_button.setEnabled(True)
        self.notice.show()
        self.tray.setToolTip(f"{APP_NAME} · service offline")

    def _replace_devices(self, devices: Any) -> None:
        for check in self._device_checks:
            self.devices_layout.removeWidget(check)
            check.deleteLater()
        self._device_checks.clear()
        valid = [device for device in devices if isinstance(device, dict)] if isinstance(devices, list) else []
        self.empty_devices.setVisible(not valid)
        for device in valid:
            device_id = str(device.get("id", ""))
            check = QCheckBox(str(device.get("name", "Unnamed mouse")))
            check.setChecked(bool(device.get("selected", False)))
            check.setToolTip(device_id)
            check.setProperty("deviceRow", True)
            check.clicked.connect(lambda checked, ident=device_id: self._set_device(ident, checked))
            self.devices_layout.addWidget(check)
            self._device_checks.append(check)

    def _command_finished(self, label: str, succeeded: bool, message: str) -> None:
        if not succeeded:
            self.show_offline(message)

    def apply_demo_status(self) -> None:
        self.apply_status(
            {
                "running": True,
                "enabled": True,
                "acceleration": 3.2,
                "coast": False,
                "devices": [
                    {"id": "demo-mouse", "name": "Wireless Mouse", "selected": True},
                    {"id": "demo-trackball", "name": "Desk Trackball", "selected": False},
                ],
            }
        )


STYLE = """
QWidget#root {
    background: #faf7fc;
    color: #2f2837;
    font-size: 14px;
}
QLabel#title { font-size: 24px; font-weight: 700; color: #35273d; }
QLabel#muted { color: #756b7d; font-size: 13px; }
QLabel#tinyMuted { color: #918897; font-size: 11px; }
QLabel#section { color: #8f4ba8; font-size: 12px; font-weight: 700; text-transform: uppercase; }
QLabel#settingTitle { color: #342d39; font-weight: 600; }
QLabel#statusPill, QLabel#valuePill {
    background: #efe8f3; color: #65586d; border-radius: 11px;
    min-height: 22px; padding: 0 10px; font-size: 12px; font-weight: 600;
}
QLabel#statusPill[online="true"] { background: #e6f5ec; color: #327050; }
QLabel#statusPill[online="paused"] { background: #f1eafa; color: #775396; }
QLabel#statusPill[online="false"] { background: #fae9ee; color: #9b4563; }
QLabel#statusPill[online="warning"] { background: #fff0db; color: #8b5c22; }
QFrame#card {
    background: #ffffff; border: 1px solid #ebe4ef; border-radius: 13px;
}
QFrame#notice { background: #f7eefa; border: 1px solid #ead8f1; border-radius: 10px; }
QLabel#noticeText { color: #674d70; font-size: 12px; }
QFrame#divider { color: #eee8f1; max-height: 1px; border: none; background: #eee8f1; }
QLabel#empty { color: #9b929f; font-style: italic; padding: 8px 2px; }
QPushButton {
    background: #a45ed2; color: white; border: 0; border-radius: 8px;
    padding: 7px 13px; font-weight: 600;
}
QPushButton:hover { background: #914bbb; }
QPushButton:pressed { background: #7d3da6; }
QPushButton:disabled { background: #d6ceda; color: #99909e; }
QPushButton#quietButton { background: #f1e9f5; color: #714982; }
QPushButton#quietButton:hover { background: #e7d9ef; }
QCheckBox { color: #403747; spacing: 9px; }
QCheckBox::indicator { width: 17px; height: 17px; }
QCheckBox::indicator:unchecked { border: 1px solid #c9bdce; border-radius: 5px; background: white; }
QCheckBox::indicator:checked { border: 1px solid #a45ed2; border-radius: 5px; background: #a45ed2; }
QCheckBox#switch::indicator { width: 38px; height: 21px; border-radius: 11px; }
QCheckBox#switch::indicator:unchecked { background: #d8d0dc; border: 1px solid #cbc1d0; }
QCheckBox#switch::indicator:checked { background: #b262d7; border: 1px solid #9c4fc2; }
QCheckBox[deviceRow="true"] { background: #fbf9fc; border-radius: 7px; padding: 7px; }
QSlider::groove:horizontal { height: 5px; background: #e6ddea; border-radius: 2px; }
QSlider::sub-page:horizontal { background: #bd70dc; border-radius: 2px; }
QSlider::handle:horizontal {
    background: #ffffff; border: 2px solid #a555c7; width: 16px;
    margin: -7px 0; border-radius: 9px;
}
QScrollArea#deviceScroll { background: transparent; }
QScrollArea#deviceScroll > QWidget > QWidget { background: transparent; }
QToolTip { background: #3c3242; color: white; border: none; padding: 5px; }
"""


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Velvet Scroll settings")
    parser.add_argument("--screenshot", metavar="PATH", help="render a sample window to an image and exit")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    if args.screenshot and "QT_QPA_PLATFORM" not in os.environ:
        os.environ["QT_QPA_PLATFORM"] = "offscreen"
    app = QApplication(sys.argv[:1])
    app.setApplicationName(APP_NAME)
    app.setApplicationDisplayName(APP_NAME)
    app.setDesktopFileName(APP_ID)
    app.setOrganizationName("zoeyrose")
    app.setQuitOnLastWindowClosed(args.screenshot is not None)
    icon = find_icon()
    app.setWindowIcon(icon)
    client = CommandClient()
    window = SettingsWindow(client, icon, screenshot_mode=bool(args.screenshot))
    window.show()
    if args.screenshot:
        window.apply_demo_status()

        def capture() -> None:
            target = Path(args.screenshot).expanduser()
            target.parent.mkdir(parents=True, exist_ok=True)
            if not window.grab().save(str(target)):
                print(f"Could not save screenshot to {target}", file=sys.stderr)
                app.exit(1)
                return
            app.quit()

        QTimer.singleShot(250, capture)
    return app.exec()


if __name__ == "__main__":
    raise SystemExit(main())
