%bcond_with check

Name:           tansu
Version:        0.1.0
Release:        1%{?dist}
Summary:        A floating shelf for files, text and images

License:        MIT
URL:            https://github.com/Westenradev/Tansu_for_Linux
Source0:        %{url}/archive/v%{version}/%{name}-%{version}.tar.gz

BuildRequires:  cargo-rpm-macros
BuildRequires:  rust >= 1.85
BuildRequires:  pkgconfig(gtk4) >= 4.8
BuildRequires:  pkgconfig(libadwaita-1) >= 1.7
BuildRequires:  pkgconfig(gtk4-layer-shell-0)

%description
Tansu is a small floating shelf window where you temporarily park files,
text snippets and images during drag-and-drop, so you can navigate freely
between windows and workspaces before dropping the items at their final
destination.

%prep
%cargo_prep

%build
%cargo_build --features layer-shell

%install
%cargo_install
install -Dm644 data/io.github.Westenradev.Tansu.desktop -t %{buildroot}%{_datadir}/applications
install -Dm644 data/io.github.Westenradev.Tansu.metainfo.xml -t %{buildroot}%{_datadir}/metainfo
install -Dm644 data/icons/hicolor/scalable/apps/io.github.Westenradev.Tansu.svg -t %{buildroot}%{_datadir}/icons/hicolor/scalable/apps
install -Dm644 data/icons/hicolor/symbolic/apps/io.github.Westenradev.Tansu-symbolic.svg -t %{buildroot}%{_datadir}/icons/hicolor/symbolic/apps

%if %{with check}
%check
%cargo_test
%endif

%files
%license LICENSE
%{_bindir}/tansu
%{_datadir}/applications/io.github.Westenradev.Tansu.desktop
%{_datadir}/metainfo/io.github.Westenradev.Tansu.metainfo.xml
%{_datadir}/icons/hicolor/scalable/apps/io.github.Westenradev.Tansu.svg
%{_datadir}/icons/hicolor/symbolic/apps/io.github.Westenradev.Tansu-symbolic.svg
