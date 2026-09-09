#!/usr/bin/env python3
from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import unittest

from PyQt6.QtCore import QEventLoop, QTimer
os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from PyQt6.QtGui import QIcon
from PyQt6.QtWidgets import QApplication

import velvet_scroll as gui


APP = QApplication.instance() or QApplication([])


class VelvetScrollUiTests(unittest.TestCase):
    def setUp(self) -> None:
        self.client = gui.CommandClient(binary="/bin/true")
        self.window = gui.SettingsWindow(self.client, QIcon(), screenshot_mode=True)

    def tearDown(self) -> None:
        self.window.close()
        self.window.deleteLater()
        APP.processEvents()

    def test_status_updates_every_control_and_device(self) -> None:
        self.window.apply_status(
            {
                "running": True,
                "enabled": True,
                "acceleration": 4.6,
                "coast": True,
                "devices": [{"id": "usb-1", "name": "Pocket Mouse", "selected": True}],
            }
        )
        self.assertTrue(self.window.enabled_check.isChecked())
        self.assertEqual(self.window.accel_slider.value(), 46)
        self.assertEqual(self.window.accel_value.text(), "4.6×")
        self.assertTrue(self.window.coast_check.isChecked())
        self.assertEqual(self.window.status_pill.text(), "On")
        self.assertEqual(len(self.window._device_checks), 1)
        self.assertEqual(self.window._device_checks[0].toolTip(), "usb-1")
        self.assertTrue(self.window._device_checks[0].isChecked())

    def test_offline_state_explains_permissions(self) -> None:
        self.window.show_offline("Permission denied while opening /dev/input/event4")
        self.assertEqual(self.window.status_pill.text(), "Offline")
        self.assertIn("udev rule", self.window.notice_text.text())
        self.assertFalse(self.window.enabled_check.isEnabled())

    def test_running_service_surfaces_device_errors_without_disabling_controls(self) -> None:
        self.window.apply_status(
            {
                "running": True,
                "enabled": True,
                "acceleration": 3,
                "coast": False,
                "devices": [],
                "errors": ["Permission denied while opening /dev/input/event4"],
            }
        )
        self.assertEqual(self.window.status_pill.text(), "Needs attention")
        self.assertTrue(self.window.notice.isVisible() or not self.window.isVisible())
        self.assertIn("udev rule", self.window.notice_text.text())
        self.assertTrue(self.window.enabled_check.isEnabled())

    def test_missing_calibration_explains_upgrade_without_reboot(self):
        message = gui.error_message("Mouse calibration was not inherited. Install the matching udev rule.")
        self.assertIn("reload udev rules", message)
        self.assertIn("restart", message)
        self.assertNotIn("denied", message)

    def test_error_json_is_unwrapped(self) -> None:
        message = gui.error_message(json.dumps({"running": False, "error": "socket unavailable"}))
        self.assertIn("isn’t running", message)

    def test_controls_emit_documented_cli_commands(self) -> None:
        calls = []
        self.client.command = lambda args, label: calls.append((args, label))
        self.window._set_enabled(True)
        self.window.accel_slider.setValue(27)
        self.window._set_acceleration()
        self.window._set_coast(True)
        self.window._set_device("event-by-id", False)
        self.assertEqual(
            calls,
            [
                (["enable"], "enable"),
                (["set", "acceleration", "2.7"], "acceleration"),
                (["set", "coast", "on"], "coast"),
                (["select", "event-by-id", "off"], "device"),
            ],
        )

    def test_binary_override_wins(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            binary = Path(folder) / "velvet-scroll"
            binary.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
            binary.chmod(0o755)
            previous = os.environ.get("VELVET_SCROLL_BIN")
            os.environ["VELVET_SCROLL_BIN"] = str(binary)
            try:
                self.assertEqual(gui.find_binary(), str(binary.resolve()))
            finally:
                if previous is None:
                    os.environ.pop("VELVET_SCROLL_BIN", None)
                else:
                    os.environ["VELVET_SCROLL_BIN"] = previous


@unittest.skipUnless(
    os.environ.get("VELVET_SCROLL_TEST_BIN"),
    "set VELVET_SCROLL_TEST_BIN to run the real daemon/Qt integration test",
)
class VelvetScrollCliIntegrationTest(unittest.TestCase):
    """Exercise QProcess IPC against an isolated daemon with no selected mice."""

    def _wait_signal(self, signal, trigger, timeout_ms: int = 5000):
        loop = QEventLoop()
        received = []

        def record(*args):
            received.append(args)
            loop.quit()

        signal.connect(record)
        timer = QTimer()
        timer.setSingleShot(True)
        timer.timeout.connect(loop.quit)
        timer.start(timeout_ms)
        trigger()
        loop.exec()
        signal.disconnect(record)
        self.assertTrue(received, "timed out waiting for Qt process signal")
        return received[0]

    def test_status_toggle_and_acceleration_round_trip(self) -> None:
        binary = str(Path(os.environ["VELVET_SCROLL_TEST_BIN"]).resolve())
        self.assertTrue(os.access(binary, os.X_OK), f"test binary is not executable: {binary}")
        with tempfile.TemporaryDirectory(prefix="velvet-scroll-qt-") as folder:
            base = Path(folder)
            runtime = base / "runtime"
            config_home = base / "config"
            runtime.mkdir(mode=0o700)
            config_dir = config_home / "velvet-scroll"
            config_dir.mkdir(parents=True, mode=0o700)
            (config_dir / "config.json").write_text(
                json.dumps(
                    {
                        "version": 1,
                        "enabled": True,
                        "acceleration": 3.0,
                        "coast": False,
                        "selected_devices": [],
                    }
                ),
                encoding="utf-8",
            )
            isolated = {
                "XDG_RUNTIME_DIR": str(runtime),
                "XDG_CONFIG_HOME": str(config_home),
            }
            daemon_environment = os.environ.copy()
            daemon_environment.update(isolated)
            daemon = subprocess.Popen(
                [binary, "daemon"],
                env=daemon_environment,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.PIPE,
                text=True,
            )
            client = gui.CommandClient(binary=binary, environment=isolated)
            try:
                socket = runtime / "velvet-scroll" / "control.sock"
                deadline = time.monotonic() + 5
                while not socket.exists() and daemon.poll() is None and time.monotonic() < deadline:
                    APP.processEvents()
                    time.sleep(0.01)
                if daemon.poll() is not None:
                    self.fail(f"isolated daemon exited: {daemon.stderr.read().strip()}")
                self.assertTrue(socket.exists(), "isolated daemon did not create its control socket")

                initial, = self._wait_signal(client.status_received, client.refresh)
                self.assertTrue(initial["running"])
                self.assertTrue(initial["enabled"])
                self.assertEqual(initial["acceleration"], 3.0)
                self.assertFalse(initial.get("active_devices"))
                self.assertTrue(all(not item["selected"] for item in initial["devices"]))

                label, succeeded, message = self._wait_signal(
                    client.command_finished,
                    lambda: client.command(["toggle"], "toggle"),
                )
                self.assertEqual(label, "toggle")
                self.assertTrue(succeeded, message)
                toggled, = self._wait_signal(client.status_received, client.refresh)
                self.assertFalse(toggled["enabled"])

                label, succeeded, message = self._wait_signal(
                    client.command_finished,
                    lambda: client.command(["set", "acceleration", "4.4"], "acceleration"),
                )
                self.assertEqual(label, "acceleration")
                self.assertTrue(succeeded, message)
                changed, = self._wait_signal(client.status_received, client.refresh)
                self.assertEqual(changed["acceleration"], 4.4)
                self.assertFalse(changed.get("active_devices"))
            finally:
                try:
                    subprocess.run(
                        [binary, "stop"],
                        env=daemon_environment,
                        stdout=subprocess.DEVNULL,
                        stderr=subprocess.DEVNULL,
                        timeout=2,
                        check=False,
                    )
                except subprocess.TimeoutExpired:
                    pass
                try:
                    daemon.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    daemon.terminate()
                    daemon.wait(timeout=2)
                if daemon.stderr is not None:
                    daemon.stderr.close()
                client.deleteLater()
                APP.processEvents()


if __name__ == "__main__":
    unittest.main()
