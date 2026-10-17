Name:           linuxprinterg
Version:        0.1.0
Release:        1%{?dist}
Summary:        Printing and scanning center for CUPS and SANE
License:        MIT
URL:            https://github.com/linuxprinterg/linuxprinterg
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  cargo >= 1.85
BuildRequires:  gcc
BuildRequires:  pkgconfig(gtk4) >= 4.14
BuildRequires:  pkgconfig(libadwaita-1) >= 1.5
BuildRequires:  make

Requires:       cups-client
Requires:       sane-backends
Requires:       poppler-utils

%description
LinuxPrinterG is a fast, minimalistic desktop hub for printing and scanning.
It works with the system services — CUPS for printing and SANE (including
eSCL/AirScan backends) for scanning — and uses the drivers already installed
in the system.

%prep
%autosetup

%build
%make_build release

%install
%make_install DESTDIR=%{buildroot}

%files
%license LICENSE
%{_bindir}/linuxprinterg
%{_datadir}/applications/org.linuxprinterg.LinuxPrinterG.desktop
%{_datadir}/metainfo/org.linuxprinterg.LinuxPrinterG.metainfo.xml

%changelog
* Sat Oct 17 2026 LinuxPrinterG contributors - 0.1.0-1
- Первая версия: печать, сканирование, устройства, настройки, журнал.
