#!/usr/bin/env bash
# install.sh — sovereign-node Arch Linux installer
# Run as root from the repo root: sudo bash install.sh
set -euo pipefail

# ── colours ──────────────────────────────────────────────────────────────────
RED='\033[0;31m'; GRN='\033[0;32m'; YEL='\033[0;33m'
BLU='\033[0;34m'; BOLD='\033[1m'; RST='\033[0m'

info()  { echo -e "${BLU}[info]${RST}  $*"; }
ok()    { echo -e "${GRN}[ ok ]${RST}  $*"; }
warn()  { echo -e "${YEL}[warn]${RST}  $*"; }
die()   { echo -e "${RED}[fail]${RST}  $*" >&2; exit 1; }
header(){ echo -e "\n${BOLD}── $* ──${RST}"; }

# ── constants ─────────────────────────────────────────────────────────────────
BINARY_NAME="sovereign-node"
BINARY_DST="/usr/local/bin/${BINARY_NAME}"
SERVICE_SRC="sovereign-node.service"
SERVICE_DST="/etc/systemd/system/${BINARY_NAME}.service"
CONFIG_DIR="/etc/sovereign-node"
CONFIG_DST="${CONFIG_DIR}/config.toml"
CONFIG_SRC="config.example.toml"
DATA_DIR="/var/lib/sovereign-node"
SVC_USER="sovereign"
SVC_GROUP="sovereign"

# ── preflight ─────────────────────────────────────────────────────────────────
header "Preflight"

[[ $EUID -eq 0 ]] || die "must be run as root (use: sudo bash install.sh)"

[[ -f /etc/arch-release ]] || \
    warn "not detected as Arch Linux — proceeding anyway"

[[ -f "Cargo.toml" ]] || \
    die "run from the sovereign-node repo root (Cargo.toml not found)"

[[ -f "${SERVICE_SRC}" ]] || \
    die "${SERVICE_SRC} not found in current directory"

[[ -f "${CONFIG_SRC}" ]] || \
    die "${CONFIG_SRC} not found in current directory"

ok "preflight passed"

# ── system dependencies ───────────────────────────────────────────────────────
header "System dependencies"

PACMAN_PKGS=()

if ! command -v gcc &>/dev/null; then
    PACMAN_PKGS+=(base-devel)
fi

if ! command -v pkg-config &>/dev/null; then
    PACMAN_PKGS+=(pkgconf)
fi

# For future BLE support via btleplug
if ! pacman -Qi bluez &>/dev/null 2>&1; then
    PACMAN_PKGS+=(bluez bluez-utils)
fi

if [[ ${#PACMAN_PKGS[@]} -gt 0 ]]; then
    info "installing: ${PACMAN_PKGS[*]}"
    pacman -Sy --noconfirm --needed "${PACMAN_PKGS[@]}"
    ok "system packages installed"
else
    ok "system dependencies already present"
fi

# ── Rust toolchain ────────────────────────────────────────────────────────────
header "Rust toolchain"

if ! command -v cargo &>/dev/null; then
    info "Rust not found — installing via rustup"
    # Install as the invoking user so PATH is set correctly
    SUDO_USER="${SUDO_USER:-root}"
    if [[ "${SUDO_USER}" == "root" ]]; then
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
        source "${HOME}/.cargo/env"
    else
        sudo -u "${SUDO_USER}" bash -c \
            'curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable'
        export PATH="/home/${SUDO_USER}/.cargo/bin:${PATH}"
    fi
    ok "Rust installed"
else
    RUST_VER=$(rustc --version)
    ok "Rust present: ${RUST_VER}"
fi

# ── build ─────────────────────────────────────────────────────────────────────
header "Building ${BINARY_NAME} (release)"

# Run as invoking user if possible (avoids building as root)
BUILD_USER="${SUDO_USER:-root}"
if [[ "${BUILD_USER}" != "root" ]]; then
    info "building as ${BUILD_USER}"
    sudo -u "${BUILD_USER}" bash -c \
        "PATH=\"/home/${BUILD_USER}/.cargo/bin:\$PATH\" cargo build --release 2>&1"
else
    cargo build --release
fi

BINARY_SRC="target/release/${BINARY_NAME}"
[[ -f "${BINARY_SRC}" ]] || die "build succeeded but binary not found at ${BINARY_SRC}"
ok "build complete → ${BINARY_SRC}"

# ── user and group ────────────────────────────────────────────────────────────
header "System user"

if ! getent group "${SVC_GROUP}" &>/dev/null; then
    groupadd --system "${SVC_GROUP}"
    ok "created group: ${SVC_GROUP}"
else
    ok "group exists: ${SVC_GROUP}"
fi

if ! id "${SVC_USER}" &>/dev/null; then
    useradd \
        --system \
        --gid "${SVC_GROUP}" \
        --groups bluetooth \
        --no-create-home \
        --shell /usr/bin/nologin \
        --comment "Sovereign Node daemon" \
        "${SVC_USER}"
    ok "created user: ${SVC_USER}"
else
    ok "user exists: ${SVC_USER}"
    # Ensure bluetooth group membership for BLE scanning
    usermod -aG bluetooth "${SVC_USER}" 2>/dev/null || true
fi

# ── directories ───────────────────────────────────────────────────────────────
header "Directories"

install -dm755                        "${CONFIG_DIR}"
install -dm750 -o "${SVC_USER}" -g "${SVC_GROUP}" "${DATA_DIR}"
ok "created ${CONFIG_DIR} and ${DATA_DIR}"

# ── install binary ────────────────────────────────────────────────────────────
header "Installing binary"

install -Dm755 "${BINARY_SRC}" "${BINARY_DST}"
ok "installed → ${BINARY_DST}"

# ── install systemd unit ──────────────────────────────────────────────────────
header "Systemd service"

install -Dm644 "${SERVICE_SRC}" "${SERVICE_DST}"
systemctl daemon-reload
ok "service file installed → ${SERVICE_DST}"

# ── install config ─────────────────────────────────────────────────────────────
header "Configuration"

if [[ -f "${CONFIG_DST}" ]]; then
    warn "config already exists at ${CONFIG_DST} — not overwriting"
    warn "  to reset: cp ${CONFIG_SRC} ${CONFIG_DST}"
else
    install -Dm640 -o root -g "${SVC_GROUP}" "${CONFIG_SRC}" "${CONFIG_DST}"
    ok "installed config → ${CONFIG_DST}"
    info "edit ${CONFIG_DST} before starting the service"
fi

# ── generate node identity (first boot) ───────────────────────────────────────
header "Node identity"

KEY_FILE="${DATA_DIR}/node.key"
DID_FILE="${DATA_DIR}/node.did"

if [[ -f "${KEY_FILE}" && -f "${DID_FILE}" ]]; then
    NODE_DID=$(cat "${DID_FILE}")
    ok "identity already exists: ${NODE_DID}"
else
    info "generating node identity..."
    # Generate identity files in data dir
    NODE_DID=$(sudo -u "${SVC_USER}" \
        SOVEREIGN_CONFIG="${CONFIG_DST}" \
        "${BINARY_DST}" --init-identity 2>/dev/null | tail -1)
    ok "node DID: ${NODE_DID}"
fi

# ── enable and start ──────────────────────────────────────────────────────────
header "Service"

if systemctl is-enabled --quiet "${BINARY_NAME}" 2>/dev/null; then
    ok "service already enabled"
else
    systemctl enable "${BINARY_NAME}"
    ok "service enabled"
fi

read -r -p $'\n'"Start ${BINARY_NAME} now? [y/N] " START_NOW
if [[ "${START_NOW,,}" == "y" ]]; then
    systemctl restart "${BINARY_NAME}"
    sleep 1
    if systemctl is-active --quiet "${BINARY_NAME}"; then
        ok "service is running"
    else
        warn "service failed to start — check logs:"
        warn "  journalctl -u ${BINARY_NAME} -n 30 --no-pager"
    fi
else
    info "skipped — start manually with: systemctl start ${BINARY_NAME}"
fi

# ── summary ───────────────────────────────────────────────────────────────────
header "Install complete"

echo
echo -e "  ${BOLD}Binary${RST}    ${BINARY_DST}"
echo -e "  ${BOLD}Config${RST}    ${CONFIG_DST}"
echo -e "  ${BOLD}Data dir${RST}  ${DATA_DIR}"
echo -e "  ${BOLD}Service${RST}   ${BINARY_NAME}.service"
echo -e "  ${BOLD}Node DID${RST}  ${NODE_DID:-unknown}"
echo
echo -e "  ${BOLD}Useful commands:${RST}"
echo -e "    journalctl -u ${BINARY_NAME} -f          # live logs"
echo -e "    systemctl status ${BINARY_NAME}          # service status"
echo -e "    curl -s http://127.0.0.1:7779/status | jq  # node API"
echo -e "    curl -s http://127.0.0.1:7779/devices | jq  # VCP devices"
echo -e "    ${BINARY_DST} --print-config            # show resolved config"
echo
echo -e "  ${BOLD}Next steps:${RST}"
echo -e "    1. Edit ${CONFIG_DST} (Vantage URL, Nostr relay, Sui RPC)"
echo -e "    2. systemctl restart ${BINARY_NAME}"
echo -e "    3. Connect a Go2 to the same network — it will appear in /devices"
echo
