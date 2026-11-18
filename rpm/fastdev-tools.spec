%global debug_package %{nil}
%global _missing_build_ids_terminate_build 0

Name:           fastdev-tools
Version:        0.1.0
Release:        1%{?dist}
Summary:        fastdev build/CI host tools
License:        Proprietary
BuildArch:      %{_arch}

%{!?fastdev_src: %global fastdev_src %{_sourcedir}/..}

%description
fastdev-capture, fastdev-compile, and fastdev-diff: build-time and CI
tooling, not part of the vehicle image. Installed on the reference/test VM
image only so fastdev-capture/fastdev-diff can run there over SSH during
the capture-and-validate workflow.

%install
mkdir -p %{buildroot}%{_bindir}
install -m 0755 %{fastdev_src}/target/release/fastdev-capture %{buildroot}%{_bindir}/fastdev-capture
install -m 0755 %{fastdev_src}/target/release/fastdev-compile %{buildroot}%{_bindir}/fastdev-compile
install -m 0755 %{fastdev_src}/target/release/fastdev-diff %{buildroot}%{_bindir}/fastdev-diff

%files
%{_bindir}/fastdev-capture
%{_bindir}/fastdev-compile
%{_bindir}/fastdev-diff

%changelog
* Wed Nov 18 2026 fastdev - 0.1.0-1
- Initial packaging (Milestone 1-4 scope).
