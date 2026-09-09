# Validation

The automated suite checks engine behavior, event transformation, protocol
validation, a real daemon/client round trip with temporary configuration, and
Qt controls rendered offscreen. None of the default tests grabs a physical mouse.

## Before a public release

Run the following on a disposable desktop session or with an alternate input
method available. Mark combinations tested; do not extrapolate a Linux API unit
test into a claim that every desktop works.

| Session | Detented wheel | High-resolution wheel | Composite mouse | Status |
| --- | --- | --- | --- | --- |
| KDE Plasma Wayland | pending | pending | pending | manual acceptance needed |
| KDE Plasma X11 | pending | pending | pending | manual acceptance needed |
| GNOME Wayland | pending | pending | pending | manual acceptance needed |
| Xfce X11 | pending | pending | pending | manual acceptance needed |
| A wlroots compositor | pending | pending | pending | manual acceptance needed |

For each tested mouse/session:

1. Start with no selected devices. Confirm the physical mouse is unchanged.
2. Select the mouse, keeping all buttons released during the handoff. Confirm
   pointer movement, clicks, side buttons, keyboard/media keys on composite mice,
   horizontal scrolling, and slow vertical scrolling retain their behavior.
3. Compare slow isolated notches with a quick flick at 1×, 3× and 8×. Confirm the
   first notch stays precise and reversing direction immediately resets gain.
4. Toggle with the window, tray and desktop shortcut while scrolling. Confirm no
   duplicate scrolling and no old momentum after disabling.
5. With coasting on, flick then reverse, click, move the mouse, disable, suspend,
   and stop the service. Confirm the glide stops. Move a second selected mouse
   during a glide to check cancellation across devices.
6. Test ordinary apps: a browser, file manager, Qt text editor, GTK text editor,
   a terminal, and an application that uses the wheel for zoom. Check that any
   app-specific smooth scrolling does not cause surprising extra momentum.
7. Unplug/reconnect, connect another selected mouse while moving the first, and
   restart the service. Confirm the first pointer never pauses during preparation.
8. Stop with SIGTERM, then force-stop with SIGKILL. Confirm physical input returns
   and a subsequent service can start. Repeat after dragging and after reconnect.
9. Switch sessions/log out and test autostart. Confirm the service runs for the
   intended user, and the device ACLs suit the machine's session policy.
10. Measure idle CPU and latency under an 8 kHz mouse and disk/CPU load. Report
    measurements and hardware rather than promising a theoretical latency.

## Recorded development checks

On 2026-09-08, development checks on Ubuntu 26.04 x86_64 completed:

- 27 Rust unit tests and four real daemon/socket tests passed on Rust 1.85.0.
- Six Qt UI tests and one real Qt/CLI/daemon integration test passed offscreen.
- Stable Rust formatting and strict Clippy passed.
- ARM64 Linux cross-check (`cargo check --target aarch64-unknown-linux-gnu`) passed;
  this is compilation evidence, not an ARM64 runtime test.
- A release binary and native amd64 Debian package built successfully. Package
  dependencies were computed with `dpkg-shlibdeps`, and package ownership and
  staged install/uninstall were checked.
- Desktop entries and udev rules passed their validators.
- Read-only enumeration recognized a Logitech G Pro composite mouse and a
  SteelSeries mouse interface. No physical device was selected or grabbed.

The development sandbox restricts Unix sockets; daemon integration tests were
run outside it with private temporary configuration and no device selections.
Actual evdev capture requires administrator-installed input permissions, which
were not changed during development. Manual desktop feel and the above hardware
combinations remain unverified until exercised on those devices. Read-only
enumeration does not prove event forwarding.

## Pointer identity and timing

Mirrors preserve exact name/vendor/product, import effective source DPI and
wheel calibration through udev, and retain physical frame timestamps. They are
excluded from enumeration by a hidden physical-path marker. Capture refuses to
start without a successful metadata import.

Validation includes 32 Rust unit tests, four daemon integration tests, strict
Clippy, Rust 1.85 compilation, and seven UI unit tests. The metadata helper was
checked read-only against a G Pro's udev database. Generated installed rules pass
`udevadm verify`. These checks do not establish physical pointer feel; use the
hardware checklist above to verify that after installation.
