%global debug_package %{nil}
%global _missing_build_ids_terminate_build 0

Name:           fastdev
Version:        0.1.0
Release:        2%{?dist}
Summary:        fastdev automotive boot speedup - static device bring-up
License:        Proprietary
Source0:        %{name}-%{version}-vendored.tar.gz
BuildArch:      %{_arch}
BuildRequires:  cargo
BuildRequires:  rust

%description
fastdev speeds up automotive boot by precomputing udev device metadata at
image build time. This source package builds the whole Cargo workspace
once; the resulting binaries ship as three independently installable
subpackages - see each one's description below. This base package carries
no files of its own.

%package -n fastdev-prep
Summary:        fastdev Phase 1 static device bring-up
# For semanage/restorecon in %%post - see the comment there for why this
# approach (not a proper policy module) is a known-acceptable shortcut for
# now, not the final production answer.
Requires:       policycoreutils-python-utils

%description -n fastdev-prep
Boot-time replay of a precomputed udev snapshot (fastdev-prep), plus the
systemd generator (fastdev-check) that decides, per boot and before any
unit loads, whether to run the fast replay or defer to the stock udev
chain. Ships with no [Install] section on static-devprep.service - only
fastdev-check ever enables it, and only on a confirmed match, as part of
the project's fail-safe-by-default design.

%package -n fastdev-usb
Summary:        fastdev Phase 2 USB hotplug gatekeeper

%description -n fastdev-usb
Scoped, deferred USB hotplug support for the exposed cabin port(s): a real
systemd-udevd instance (usb-udevd.service) bind-mounted over a curated
rules.d, plus fastdev-gatekeeper, a default-deny VID:PID allow-list helper
invoked from a udev rule. Ships with no [Install] section on
usb-udevd.service - fastdev-check enables it, mutually exclusive with the
stock udev chain.

%package -n fastdev-tools
Summary:        fastdev build/CI host tools

%description -n fastdev-tools
fastdev-capture, fastdev-compile, and fastdev-diff: build-time and CI
tooling, not part of the vehicle image. Installed on the reference/test VM
image only so fastdev-capture/fastdev-diff can run there over SSH during
the capture-and-validate workflow.

%prep
%setup -q -n fastdev
# mock/COPR builds have no network access, so %%build can't let cargo hit
# crates.io - point it at the vendor/ tree shipped inside Source0 instead
# (populated by `cargo vendor vendor` before the tarball is built - see
# the README). Generated here rather than committed, so it's never stale
# relative to wherever vendor/ actually lands.
mkdir -p .cargo
cat > .cargo/config.toml <<'EOF'
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"
EOF

%build
cargo build --release --workspace --locked --offline

%install
# fastdev-prep
mkdir -p %{buildroot}%{_prefix}/lib/fastdev/bin
mkdir -p %{buildroot}%{_prefix}/lib/systemd/system-generators
mkdir -p %{buildroot}%{_prefix}/lib/systemd/system
install -m 0755 target/release/fastdev-prep \
    %{buildroot}%{_prefix}/lib/fastdev/bin/fastdev-prep
install -m 0755 target/release/fastdev-check \
    %{buildroot}%{_prefix}/lib/systemd/system-generators/fastdev-check
install -m 0644 systemd/static-devprep.service \
    %{buildroot}%{_prefix}/lib/systemd/system/static-devprep.service

# fastdev-usb
mkdir -p %{buildroot}%{_prefix}/lib/fastdev/usb-rules.d
mkdir -p %{buildroot}%{_prefix}/lib/udev/rules.d
mkdir -p %{buildroot}%{_sysconfdir}/fastdev
install -m 0755 target/release/fastdev-gatekeeper \
    %{buildroot}%{_prefix}/lib/fastdev/bin/fastdev-gatekeeper
install -m 0644 systemd/usb-udevd.service \
    %{buildroot}%{_prefix}/lib/systemd/system/usb-udevd.service
# Single source, installed to BOTH locations - the real system-wide
# rules.d (used by the stock fallback chain) and the curated tree
# usb-udevd.service bind-mounts over its own view. Generated from one
# file here, never hand-duplicated.
install -m 0644 udev-rules/99-fastdev-usb.rules \
    %{buildroot}%{_prefix}/lib/udev/rules.d/99-fastdev-usb.rules
install -m 0644 udev-rules/99-fastdev-usb.rules \
    %{buildroot}%{_prefix}/lib/fastdev/usb-rules.d/99-fastdev-usb.rules
install -m 0644 config/usb-allowlist.toml \
    %{buildroot}%{_sysconfdir}/fastdev/usb-allowlist.toml

# fastdev-tools
mkdir -p %{buildroot}%{_bindir}
install -m 0755 target/release/fastdev-capture %{buildroot}%{_bindir}/fastdev-capture
install -m 0755 target/release/fastdev-compile %{buildroot}%{_bindir}/fastdev-compile
install -m 0755 target/release/fastdev-diff %{buildroot}%{_bindir}/fastdev-diff

%post -n fastdev-prep
# fastdev-prep needs to run as udev_t to write
# udev_var_run_t-labeled /run/udev/data - confirmed via a real boot that
# this requires the FILE to carry an entrypoint type SELinux accepts for
# that domain transition (a unit-file SELinuxContext= override alone was
# tried and denied). Label it the same as /usr/bin/udevadm.
#
# KNOWN SHORTCUT, not the final answer: semanage/restorecon may not be
# present on a minimal production image. The durable fix is a proper
# custom SELinux policy module (.te/.fc, shipped and semodule-installed)
# rather than runtime fcontext + restorecon - tracked as a follow-up, not
# implemented here.
semanage fcontext -a -t udev_exec_t '%{_prefix}/lib/fastdev/bin/fastdev-prep' 2>/dev/null || :
restorecon -v %{_prefix}/lib/fastdev/bin/fastdev-prep || :

%postun -n fastdev-prep
if [ "$1" -eq 0 ]; then
    semanage fcontext -d '%{_prefix}/lib/fastdev/bin/fastdev-prep' 2>/dev/null || :
fi

%files

%files -n fastdev-prep
%{_prefix}/lib/fastdev/bin/fastdev-prep
%{_prefix}/lib/systemd/system-generators/fastdev-check
%{_prefix}/lib/systemd/system/static-devprep.service

%files -n fastdev-usb
%{_prefix}/lib/fastdev/bin/fastdev-gatekeeper
%{_prefix}/lib/systemd/system/usb-udevd.service
%{_prefix}/lib/udev/rules.d/99-fastdev-usb.rules
%{_prefix}/lib/fastdev/usb-rules.d/99-fastdev-usb.rules
%config(noreplace) %{_sysconfdir}/fastdev/usb-allowlist.toml

%files -n fastdev-tools
%{_bindir}/fastdev-capture
%{_bindir}/fastdev-compile
%{_bindir}/fastdev-diff

%changelog
* Wed Nov 18 2026 fastdev - 0.1.0-2
- Build offline against a vendor/ tree shipped in Source0
  (`cargo build --locked --offline`) - COPR/mock has no network access
  in %%build, confirmed by a real failed build (could not resolve
  index.crates.io).
* Wed Nov 18 2026 fastdev - 0.1.0-1
- Merge fastdev-prep/fastdev-usb/fastdev-tools into one spec with
  subpackages, sharing a single cargo build. Build from Source0 via
  cargo in %%build instead of copying externally-prebuilt binaries in
  %%install.
