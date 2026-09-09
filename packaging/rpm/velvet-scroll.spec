Name:           velvet-scroll
Version:        %{velvet_scroll_version}
Release:        %{velvet_scroll_release}
Summary:        Linux mouse wheel acceleration

License:        MIT
URL:            https://github.com/zoeyrose/velvet-scroll
Requires:       python3 >= 3.10
Requires:       python3-qt6
Requires:       qt6-qtsvg

# The release binary is already stripped by Cargo. Disabling the debug
# subpackage also keeps this native binary-only build independent of distro
# debuginfo tooling while leaving RPM's ELF dependency generator enabled.
%global debug_package %{nil}

%description
Velvet Scroll accelerates mouse wheel scrolling while keeping precise small
movements. It includes an unprivileged daemon, command-line controls, and a
PyQt6 desktop interface.

%prep

%build

%install
rm -rf "%{buildroot}"
mkdir -p "%{buildroot}"
cp -a "%{velvet_scroll_stage}/." "%{buildroot}/"

%files
%license %{_datadir}/doc/velvet-scroll/copyright
%doc %{_datadir}/doc/velvet-scroll/README.md
%doc %{_datadir}/doc/velvet-scroll/installation.md
%doc %{_datadir}/doc/velvet-scroll/validation.md
%{_bindir}/velvet-scroll
%{_bindir}/velvet-scroll-gui
%dir %{_libexecdir}/velvet-scroll
%{_libexecdir}/velvet-scroll/velvet_scroll.py
%{_prefix}/lib/systemd/user/velvet-scroll.service
%{_prefix}/lib/udev/rules.d/72-velvet-scroll.rules
%config(noreplace) /etc/modules-load.d/velvet-scroll.conf
%{_datadir}/applications/io.github.zoeyrose.VelvetScroll.desktop
%{_datadir}/icons/hicolor/scalable/apps/velvet-scroll.svg
%{_datadir}/icons/hicolor/symbolic/apps/velvet-scroll-symbolic.svg
%dir %{_datadir}/velvet-scroll
%dir %{_datadir}/velvet-scroll/examples
%{_datadir}/velvet-scroll/examples/velvet-scroll-daemon.desktop

%changelog
