%global debug_package %{nil}
%global _missing_build_ids_terminate_build 0

Name:           fastdev-prep
Version:        0.1.0
Release:        20%{?dist}
Summary:        fastdev Phase 1 static device bring-up
License:        Proprietary
BuildArch:      %{_arch}
# For semanage/restorecon in %%post - see the comment there for why this
# approach (not a proper policy module) is a known-acceptable shortcut for
# now, not the final production answer.
Requires:       policycoreutils-python-utils

# Pass the workspace root so %%install can grab prebuilt release binaries -
# the workspace is built once (`cargo build --release`), not rebuilt per
# package:
#   rpmbuild -bb fastdev-prep.spec --define "fastdev_src /path/to/fastdev"
%{!?fastdev_src: %global fastdev_src %{_sourcedir}/..}

%description
Boot-time replay of a precomputed udev snapshot (fastdev-prep), plus the
systemd generator (fastdev-check) that decides, per boot and before any
unit loads, whether to run the fast replay or defer to the stock udev
chain. Ships with no [Install] section on static-devprep.service - only
fastdev-check ever enables it, and only on a confirmed match, as part of
the project's fail-safe-by-default design.

%install
mkdir -p %{buildroot}%{_prefix}/lib/fastdev/bin
mkdir -p %{buildroot}%{_prefix}/lib/systemd/system-generators
mkdir -p %{buildroot}%{_prefix}/lib/systemd/system

install -m 0755 %{fastdev_src}/target/release/fastdev-prep \
    %{buildroot}%{_prefix}/lib/fastdev/bin/fastdev-prep
install -m 0755 %{fastdev_src}/target/release/fastdev-check \
    %{buildroot}%{_prefix}/lib/systemd/system-generators/fastdev-check
install -m 0644 %{fastdev_src}/systemd/static-devprep.service \
    %{buildroot}%{_prefix}/lib/systemd/system/static-devprep.service

%post
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

%postun
if [ "$1" -eq 0 ]; then
    semanage fcontext -d '%{_prefix}/lib/fastdev/bin/fastdev-prep' 2>/dev/null || :
fi

%files
%{_prefix}/lib/fastdev/bin/fastdev-prep
%{_prefix}/lib/systemd/system-generators/fastdev-check
%{_prefix}/lib/systemd/system/static-devprep.service

%changelog
* Wed Nov 18 2026 fastdev - 0.1.0-1
- Initial packaging (Milestone 1-4 scope).
