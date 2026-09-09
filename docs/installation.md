# Installing Velvet Scroll

Velvet Scroll supports Linux desktop sessions with access to evdev and uinput.
The daemon runs as your normal desktop user. Never run `velvet-scroll daemon` or
the GUI with `sudo`: doing so creates root-owned configuration and socket files
and connects the service to the wrong user session.

## Requirements

Building the Rust binary requires Rust 1.85 or newer, Cargo, and a C compiler.
The GUI requires Python 3 and PyQt6 at runtime. The service integration assumes
a Linux system using udev; systemd is optional. The initial native input backend
supports x86-64 and AArch64 Linux systems.

Common package names are:

| Distribution | Build tools | GUI runtime |
| --- | --- | --- |
| Debian / Ubuntu | `build-essential cargo rustc` | `python3 python3-pyqt6` |
| Fedora | `gcc cargo rust` | `python3 python3-qt6` |
| Arch Linux | `base-devel rust` | `python python-pyqt6` |

Distribution Rust packages may be older than the minimum supported version.
Use [rustup](https://rustup.rs/) if your distribution does not provide Rust
1.85 or newer. PyQt6 may also be installed into a virtual environment for a
source checkout, but a system install should use the distribution package so
the installed launcher can import it with `/usr/bin/env python3`.

## Download a package

[GitHub Releases](https://github.com/zoeyrose/velvet-scroll/releases) provides
Debian, RPM, Arch and tar packages for x86_64 and aarch64. Choose your architecture
and install the downloaded native package with your distro's package manager:

```sh
sudo apt install ./velvet-scroll_*.deb             # Debian / Ubuntu
sudo dnf install ./velvet-scroll-*.rpm              # Fedora
sudo pacman -U ./velvet-scroll-*.pkg.tar.zst          # Arch Linux
```

Use one matching file, not all versions in a downloads folder. CI builds on
Ubuntu 24.04 (glibc 2.39); older systems may need a local source build. The RPM
runtime dependencies target Fedora; other RPM distributions may use different
Qt package names. Packages are not a claim of testing every distribution.

After installation, follow the permission reload and start commands below; no
reboot is required. Tar archives include their own install/uninstall helpers.
For development snapshots and version policy, see [releases](releases.md).

## Arch Linux

Build from source with Arch's packages (including
[`python-pyqt6`](https://archlinux.org/packages/extra/x86_64/python-pyqt6/) and
`qt6-svg` for the app icons):

```sh
sudo pacman -Syu --needed base-devel git rust python-pyqt6 qt6-svg
git clone https://github.com/zoeyrose/velvet-scroll.git
cd velvet-scroll
cargo build --release --locked
sudo make install install-udev install-uinput-module PREFIX=/usr
```

If you already use rustup, keep that toolchain and omit `rust` from the pacman
command. This is a source install using the Makefile; pacman does not track the
installed Velvet Scroll files. No AUR package is provided by this repository.

Apply device permissions without rebooting, then start the app:

```sh
sudo udevadm control --reload-rules
sudo modprobe uinput
sudo udevadm trigger --subsystem-match=input --action=change
sudo udevadm trigger --subsystem-match=misc --sysname-match=uinput --action=change
sudo udevadm settle
systemctl --user daemon-reload
systemctl --user start velvet-scroll
velvet-scroll gui
```

Select your mouse in the window. To start the daemon with future graphical
sessions, run `systemctl --user enable velvet-scroll`. Run the daemon and GUI as
your normal desktop user, including under KDE Plasma.

To remove this source installation later:

```sh
systemctl --user disable --now velvet-scroll
sudo make uninstall uninstall-udev uninstall-uinput-module PREFIX=/usr
sudo udevadm control --reload-rules
systemctl --user daemon-reload
```

## Build and install from source

Build and test without accessing input devices:

```sh
cargo build --release
cargo test
QT_QPA_PLATFORM=offscreen python3 -m unittest discover -s ui -p 'test_*.py' -v
```

Install the application under `/usr/local` (the default):

```sh
sudo make install
```

For a conventional distribution-style `/usr` install:

```sh
sudo make install PREFIX=/usr
```

`DESTDIR` is supported for package staging and does not become part of any
installed path:

```sh
make stage STAGE_DIR="$PWD/dist/root" PREFIX=/usr
```

The application install includes the binary, GUI, icon, desktop entry,
documentation, and a systemd user unit. It does not enable the service and it
does not install the optional host udev rule.

## Device access and its security implications

Velvet Scroll reads selected mouse event devices and creates accelerated wheel
events through `/dev/uinput`. Access to uinput permits a process to inject input
events into the desktop session. Review
[`packaging/udev/72-velvet-scroll.rules`](../packaging/udev/72-velvet-scroll.rules)
before installing it.

The supplied rule grants temporary ACLs through `TAG+="uaccess"` to the active
local seat user. It matches mouse event nodes identified by udev and the uinput
node. It does not grant the `input` group, use world-writable permissions, or
match keyboard-only event nodes. Some gaming mice advertise auxiliary
keyboard-like keys on their mouse event node; access to that node necessarily
allows reading all events it reports. The file is numbered `72` so its tags are
present before systemd's usual `73-seat-late.rules` applies seat ACLs.

Install the host rule separately as root:

```sh
sudo make install-udev
sudo udevadm control --reload-rules
```

If `/dev/uinput` does not exist, load its kernel module:

```sh
sudo modprobe uinput
```

To load it automatically after reboot, install the supplied modules-load entry:

```sh
sudo make install-uinput-module
```

Then unplug and reconnect the mouse and sign out and back in (or reboot) so the
seat ACLs are recreated. On the host, the rule must remain owned by root and
must not be writable by ordinary users:

```sh
ls -l /usr/lib/udev/rules.d/72-velvet-scroll.rules
```

The backend enumerates only mouse devices. No device is selected by default;
choose each mouse explicitly with the GUI or `velvet-scroll select ID on`.

## Start with systemd

The installed unit is a per-user service. Start it manually to verify the setup:

```sh
systemctl --user daemon-reload
systemctl --user start velvet-scroll.service
velvet-scroll status --json
```

After it works, enable it as part of the graphical session target:

```sh
systemctl --user enable velvet-scroll.service
```

You can inspect failures with:

```sh
systemctl --user status velvet-scroll.service
journalctl --user -u velvet-scroll.service
```

Desktop environments vary in how they activate `graphical-session.target`. If
the unit remains inactive after login, use the autostart method below.

## Run without systemd

Run the daemon in the foreground from a terminal or process supervisor:

```sh
velvet-scroll daemon
```

Most graphical sessions set `XDG_RUNTIME_DIR`. If yours does not, configure a
private, user-owned runtime directory with mode `0700`; do not place the control
socket in a shared directory. The socket is created at:

```text
$XDG_RUNTIME_DIR/velvet-scroll/control.sock
```

For XDG desktop autostart, copy the supplied example into your user config:

```sh
mkdir -p "${XDG_CONFIG_HOME:-$HOME/.config}/autostart"
cp /usr/local/share/velvet-scroll/examples/velvet-scroll-daemon.desktop \
  "${XDG_CONFIG_HOME:-$HOME/.config}/autostart/"
```

Adjust `/usr/local` to the `PREFIX` used at installation. A non-desktop process
supervisor can invoke `velvet-scroll daemon` directly and should stop it with a
normal termination signal.

## Configuration and control

The persistent configuration is stored per user at:

```text
${XDG_CONFIG_HOME:-$HOME/.config}/velvet-scroll/config.json
```

Prefer the CLI or GUI over editing that file while the daemon is running:

```sh
velvet-scroll devices --json
velvet-scroll select ID on
velvet-scroll set acceleration 2.5
velvet-scroll set coast on
velvet-scroll toggle
velvet-scroll enable
velvet-scroll disable
velvet-scroll status --json
```

Acceleration accepts values from `1` through `8`. Device IDs come from
`velvet-scroll devices --json`.

## Debian package

On Debian-family systems, install `dpkg-dev`, then build a native package from
the current source tree:

```sh
make deb
sudo apt install ./dist/velvet-scroll_*.deb
```

The package includes the udev rule because installing a system package is an
explicit privileged host action. It also installs the modules-load entry so
uinput is available after reboot. It does not enable or start the user service.
The builder derives shared-library dependencies from the compiled binary with
`dpkg-shlibdeps`; the result is native to the build distribution and may require
the same or a newer C library. This is practical distribution support, and
packages have not been validated across every Debian or Ubuntu release.

Velvet Scroll is not expected to work as a standalone Flatpak. Its core job
requires host evdev and uinput access plus a host udev rule, which conflicts
with the usual Flatpak device sandbox. A host daemon paired with a sandboxed UI
would require a separate, authenticated IPC design.

## Troubleshooting

If `status` cannot connect, confirm that the daemon is running as the same user
and that `XDG_RUNTIME_DIR` is set in both processes. Remove a stale socket only
after confirming no daemon is running.

If no mice appear, inspect `velvet-scroll devices --json`, reconnect the mouse
after reloading the udev rules, and check its ACL:

```sh
getfacl /dev/input/eventX
getfacl /dev/uinput
```

The logged-in local user should have access through an ACL. Do not work around
missing access by running Velvet Scroll as root or by adding your account to a
broad `input` group.

If the GUI does not launch, verify the binding with:

```sh
python3 -c 'from PyQt6 import QtCore; print(QtCore.PYQT_VERSION_STR)'
```

If wheel events are duplicated, disable Velvet Scroll and check that only the
intended mouse is selected. If acceleration feels erratic, start at `1`, turn
coasting off, and increase acceleration gradually.

## Uninstall

Stop and disable the user service first:

```sh
systemctl --user disable --now velvet-scroll.service
```

Then remove only the files installed by the Makefile:

```sh
sudo make uninstall
sudo make uninstall-udev
sudo make uninstall-uinput-module
sudo udevadm control --reload-rules
```

Pass the same `PREFIX`, `udevdir`, and `modulesloaddir` values used during
installation. User configuration is intentionally preserved; remove it
manually if it is no longer needed.
