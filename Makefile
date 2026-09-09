SHELL := /bin/sh

PREFIX ?= /usr/local
DESTDIR ?=
CARGO ?= cargo
INSTALL ?= install
SED ?= sed

bindir := $(PREFIX)/bin
libexecdir := $(PREFIX)/libexec/velvet-scroll
datadir := $(PREFIX)/share
docdir := $(datadir)/doc/velvet-scroll
unitdir := $(PREFIX)/lib/systemd/user
udevdir ?= /usr/lib/udev/rules.d
modulesloaddir ?= /etc/modules-load.d

.PHONY: all build release test check install install-udev install-uinput-module
.PHONY: uninstall uninstall-udev uninstall-uinput-module stage
.PHONY: deb rpm arch archive packages clean

all: build

build:
	$(CARGO) build

release:
	$(CARGO) build --release --locked

test:
	$(CARGO) test
	QT_QPA_PLATFORM=offscreen VELVET_SCROLL_TEST_BIN="$(CURDIR)/target/debug/velvet-scroll" \
		python3 -m unittest discover -s ui -p 'test_*.py' -v

check:
	$(CARGO) fmt --all -- --check
	$(CARGO) clippy --all-targets --all-features -- -D warnings
	$(MAKE) test

install: release
	$(INSTALL) -Dm755 target/release/velvet-scroll "$(DESTDIR)$(bindir)/velvet-scroll"
	$(INSTALL) -Dm755 scripts/velvet-scroll-gui "$(DESTDIR)$(bindir)/velvet-scroll-gui"
	$(INSTALL) -Dm755 ui/velvet_scroll.py "$(DESTDIR)$(libexecdir)/velvet_scroll.py"
	$(INSTALL) -Dm644 assets/velvet-scroll.svg "$(DESTDIR)$(datadir)/icons/hicolor/scalable/apps/velvet-scroll.svg"
	$(INSTALL) -Dm644 assets/velvet-scroll-symbolic.svg "$(DESTDIR)$(datadir)/icons/hicolor/symbolic/apps/velvet-scroll-symbolic.svg"
	$(INSTALL) -Dm644 assets/io.github.zoeyrose.VelvetScroll.desktop "$(DESTDIR)$(datadir)/applications/io.github.zoeyrose.VelvetScroll.desktop"
	$(INSTALL) -Dm644 packaging/autostart/velvet-scroll-daemon.desktop "$(DESTDIR)$(datadir)/velvet-scroll/examples/velvet-scroll-daemon.desktop"
	$(INSTALL) -Dm644 LICENSE "$(DESTDIR)$(docdir)/copyright"
	$(INSTALL) -Dm644 README.md "$(DESTDIR)$(docdir)/README.md"
	$(INSTALL) -Dm644 docs/installation.md "$(DESTDIR)$(docdir)/installation.md"
	$(INSTALL) -Dm644 docs/validation.md "$(DESTDIR)$(docdir)/validation.md"
	$(SED) 's|@BINDIR@|$(bindir)|g' packaging/systemd/velvet-scroll.service.in | \
		$(INSTALL) -Dm644 /dev/stdin "$(DESTDIR)$(unitdir)/velvet-scroll.service"

# This is deliberately separate from `install`: the rule changes host device
# permissions and should only be installed after its implications are reviewed.
install-udev:
	@if [ -z "$(DESTDIR)" ] && [ "$$(id -u)" -ne 0 ]; then \
		echo "install-udev changes host device access and must run as root" >&2; \
		exit 1; \
	fi
	$(SED) 's|@BINDIR@|$(bindir)|g' packaging/udev/72-velvet-scroll.rules | \
		$(INSTALL) -Dm644 /dev/stdin "$(DESTDIR)$(udevdir)/72-velvet-scroll.rules"

install-uinput-module:
	@if [ -z "$(DESTDIR)" ] && [ "$$(id -u)" -ne 0 ]; then \
		echo "install-uinput-module changes host boot configuration and must run as root" >&2; \
		exit 1; \
	fi
	$(INSTALL) -Dm644 packaging/modules-load/velvet-scroll.conf "$(DESTDIR)$(modulesloaddir)/velvet-scroll.conf"

# Remove only paths installed by the targets above. rmdir succeeds only for
# empty directories, preserving any unrelated files in shared locations.
uninstall:
	rm -f "$(DESTDIR)$(bindir)/velvet-scroll"
	rm -f "$(DESTDIR)$(bindir)/velvet-scroll-gui"
	rm -f "$(DESTDIR)$(libexecdir)/velvet_scroll.py"
	rm -f "$(DESTDIR)$(datadir)/icons/hicolor/scalable/apps/velvet-scroll.svg"
	rm -f "$(DESTDIR)$(datadir)/icons/hicolor/symbolic/apps/velvet-scroll-symbolic.svg"
	rm -f "$(DESTDIR)$(datadir)/applications/io.github.zoeyrose.VelvetScroll.desktop"
	rm -f "$(DESTDIR)$(datadir)/velvet-scroll/examples/velvet-scroll-daemon.desktop"
	rm -f "$(DESTDIR)$(docdir)/copyright"
	rm -f "$(DESTDIR)$(docdir)/README.md"
	rm -f "$(DESTDIR)$(docdir)/installation.md"
	rm -f "$(DESTDIR)$(docdir)/validation.md"
	rm -f "$(DESTDIR)$(unitdir)/velvet-scroll.service"
	-rmdir "$(DESTDIR)$(libexecdir)" "$(DESTDIR)$(docdir)" "$(DESTDIR)$(datadir)/velvet-scroll/examples" "$(DESTDIR)$(datadir)/velvet-scroll"

uninstall-udev:
	rm -f "$(DESTDIR)$(udevdir)/72-velvet-scroll.rules"

uninstall-uinput-module:
	rm -f "$(DESTDIR)$(modulesloaddir)/velvet-scroll.conf"

stage:
	./scripts/stage-package.sh "$(abspath $(if $(STAGE_DIR),$(STAGE_DIR),dist/stage))" "$(PREFIX)"

deb:
	./scripts/build-deb.sh

rpm:
	./scripts/build-rpm.sh

arch:
	./scripts/build-arch.sh

archive:
	./scripts/build-archive.sh

packages:
	./scripts/build-packages.sh

clean:
	$(CARGO) clean
	rm -rf -- "$(CURDIR)/dist"
