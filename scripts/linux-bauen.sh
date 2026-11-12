#!/usr/bin/env bash
# Baut die Shell für den eigenen Linux-Rechner (Fedora) und richtet sie im Benutzerkonto ein — ohne sudo.
# Gebaut wird in einem Podman-Container, weil Rust und die WebKitGTK-Entwicklerpakete auf dem Rechner fehlen.
# Verteilt wird weiterhin nur macOS; was unter Linux fehlt, steht in src-tauri/src/linux.rs.
set -euo pipefail
cd "$(dirname "$0")/.."

podman build -t prepwell-linux-bau - <<'CONTAINER'
FROM registry.fedoraproject.org/fedora:45
RUN dnf -y install gcc gcc-c++ make pkgconf-pkg-config openssl-devel webkit2gtk4.1-devel gtk3-devel libsoup3-devel librsvg2-devel libappindicator-gtk3-devel curl file && dnf clean all
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
ENV PATH=/root/.cargo/bin:$PATH
CONTAINER

mkdir -p out
podman run --rm --security-opt label=disable \
  -v "$PWD":/quelle -v prepwell-cargo:/root/.cargo/registry -v prepwell-ziel:/ziel \
  -e CARGO_TARGET_DIR=/ziel -w /quelle/src-tauri prepwell-linux-bau \
  sh -c 'cargo build --release --locked --features tauri/custom-protocol && cp /ziel/release/prepwell-desktop /quelle/out/prepwell-linux'

install -Dm755 out/prepwell-linux "$HOME/.local/bin/prepwell"
install -Dm644 src-tauri/icons/icon.png "$HOME/.local/share/icons/prepwell.png"
mkdir -p "$HOME/.local/share/applications"
cat > "$HOME/.local/share/applications/de.prepwell.desktop.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=PrepWell
Comment=PrepWell Desktop
Exec=$HOME/.local/bin/prepwell
Icon=$HOME/.local/share/icons/prepwell.png
Terminal=false
Categories=Education;
StartupWMClass=prepwell
DESKTOP
update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
echo "Fertig: PrepWell steht in der App-Übersicht (oder im Terminal: prepwell)."
