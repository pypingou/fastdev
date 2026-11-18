%global debug_package %{nil}
%global _missing_build_ids_terminate_build 0

Name:           fastdev-usb
Version:        0.1.0
Release:        1%{?dist}
Summary:        fastdev Phase 2 USB hotplug gatekeeper
License:        Proprietary
BuildArch:      %{_arch}

%{!?fastdev_src: %global fastdev_src %{_sourcedir}/..}

%description
Scoped, deferred USB hotplug support for the exposed cabin port(s): a real
systemd-udevd instance (usb-udevd.service) bind-mounted over a curated
rules.d, plus fastdev-gatekeeper, a default-deny VID:PID allow-list helper
invoked from a udev rule. Ships with no [Install] section on
usb-udevd.service - fastdev-check enables it, mutually exclusive with the
stock udev chain.

%install
mkdir -p %{buildroot}%{_prefix}/lib/fastdev/bin
mkdir -p %{buildroot}%{_prefix}/lib/fastdev/usb-rules.d
mkdir -p %{buildroot}%{_prefix}/lib/systemd/system
mkdir -p %{buildroot}%{_prefix}/lib/udev/rules.d
mkdir -p %{buildroot}%{_sysconfdir}/fastdev

install -m 0755 %{fastdev_src}/target/release/fastdev-gatekeeper \
    %{buildroot}%{_prefix}/lib/fastdev/bin/fastdev-gatekeeper
install -m 0644 %{fastdev_src}/systemd/usb-udevd.service \
    %{buildroot}%{_prefix}/lib/systemd/system/usb-udevd.service

# Single source, installed to BOTH locations - the
# real system-wide rules.d (used by the stock fallback chain) and the
# curated tree usb-udevd.service bind-mounts over its own view. Generated
# from one file here, never hand-duplicated.
install -m 0644 %{fastdev_src}/udev-rules/99-fastdev-usb.rules \
    %{buildroot}%{_prefix}/lib/udev/rules.d/99-fastdev-usb.rules
install -m 0644 %{fastdev_src}/udev-rules/99-fastdev-usb.rules \
    %{buildroot}%{_prefix}/lib/fastdev/usb-rules.d/99-fastdev-usb.rules

install -m 0644 %{fastdev_src}/config/usb-allowlist.toml \
    %{buildroot}%{_sysconfdir}/fastdev/usb-allowlist.toml

%files
%{_prefix}/lib/fastdev/bin/fastdev-gatekeeper
%{_prefix}/lib/systemd/system/usb-udevd.service
%{_prefix}/lib/udev/rules.d/99-fastdev-usb.rules
%{_prefix}/lib/fastdev/usb-rules.d/99-fastdev-usb.rules
%config(noreplace) %{_sysconfdir}/fastdev/usb-allowlist.toml

%changelog
* Wed Nov 18 2026 fastdev - 0.1.0-1
- Initial packaging (Milestone 1-4 scope).
