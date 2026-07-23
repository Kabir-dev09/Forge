#!/bin/sh

# Forge source installer for Linux.
#
# Optional environment overrides:
#   FORGE_REPO_URL  Repository to clone.
#   FORGE_REF       Branch or tag to install (default: repository default branch).
#   TMPDIR          Parent directory for temporary build files.

set -eu
set -f

REPO_URL=${FORGE_REPO_URL:-https://github.com/Kabir-dev09/Forge.git}
REPO_REF=${FORGE_REF:-}
INSTALL_DIR=/opt/forge/bin
INSTALL_PATH=$INSTALL_DIR/forge
LINK_PATH=/usr/bin/forge
SOURCE_BINARY=forge-main

TEMP_DIR=
ROOT_STAGE=
ROOT_LINK_STAGE=
USER_STAGE=
ELEVATION_TOOL=
ACTION=install
REMOVE_BLUR_RULE=0

if [ -t 1 ]; then
    BOLD='\033[1m'
    GREEN='\033[32m'
    YELLOW='\033[33m'
    RED='\033[31m'
    RESET='\033[0m'
else
    BOLD=
    GREEN=
    YELLOW=
    RED=
    RESET=
fi

stage() {
    printf '%b==>%b %s\n' "$GREEN$BOLD" "$RESET" "$1"
}

note() {
    printf '%b  ->%b %s\n' "$GREEN" "$RESET" "$1"
}

warn() {
    printf '%bwarning:%b %s\n' "$YELLOW$BOLD" "$RESET" "$1" >&2
}

die() {
    printf '%berror:%b %s\n' "$RED$BOLD" "$RESET" "$1" >&2
    exit 1
}

usage() {
    cat <<EOF
Forge Linux installer

Usage:
  ./install.sh
  ./install.sh uninstall [--remove-blur-rule]
  ./install.sh --uninstall [--remove-blur-rule]

Environment overrides:
  FORGE_REPO_URL=<url>  Clone a different Forge repository.
  FORGE_REF=<ref>       Install a specific branch or tag.
  TMPDIR=<directory>    Select the temporary build parent directory.

Uninstall removes the installed binary and Forge-generated runtime data.
It preserves ~/.config/forge and does not remove shared build dependencies.
The Niri blur rule is preserved unless --remove-blur-rule is supplied.
EOF
}

for argument in "$@"; do
    case "$argument" in
        -h|--help)
            [ "$#" -eq 1 ] || die "--help cannot be combined with other arguments"
            usage
            exit 0
            ;;
        uninstall|--uninstall)
            [ "$ACTION" = install ] || die "uninstall was specified more than once"
            ACTION=uninstall
            ;;
        --remove-blur-rule)
            [ "$REMOVE_BLUR_RULE" -eq 0 ] ||
                die "--remove-blur-rule was specified more than once"
            REMOVE_BLUR_RULE=1
            ;;
        *)
            usage >&2
            die "unknown argument: $argument"
            ;;
    esac
done

if [ "$REMOVE_BLUR_RULE" -eq 1 ] && [ "$ACTION" != uninstall ]; then
    die "--remove-blur-rule can only be used with uninstall"
fi

if [ "$(id -u)" -eq 0 ]; then
    die "run this installer as a regular user; it will request elevation only when required"
fi

[ "$(uname -s)" = Linux ] || die "Forge currently supports Linux only"

find_elevation_tool() {
    if [ -n "$ELEVATION_TOOL" ]; then
        return
    fi
    if command -v sudo >/dev/null 2>&1; then
        ELEVATION_TOOL=sudo
    elif command -v doas >/dev/null 2>&1; then
        ELEVATION_TOOL=doas
    else
        die "sudo or doas is required for system package and /opt installation steps"
    fi
}

as_root() {
    find_elevation_tool
    "$ELEVATION_TOOL" "$@"
}

cleanup() {
    status=$?
    trap - 0 HUP INT TERM

    if [ -n "$TEMP_DIR" ] && [ -d "$TEMP_DIR" ]; then
        rm -rf "$TEMP_DIR"
    fi
    if [ -n "$USER_STAGE" ]; then
        rm -f "$USER_STAGE"
    fi
    if [ -n "$ROOT_STAGE" ] || [ -n "$ROOT_LINK_STAGE" ]; then
        find_elevation_tool
        [ -z "$ROOT_STAGE" ] || "$ELEVATION_TOOL" rm -f "$ROOT_STAGE" || true
        [ -z "$ROOT_LINK_STAGE" ] || "$ELEVATION_TOOL" rm -f "$ROOT_LINK_STAGE" || true
    fi

    exit "$status"
}

trap cleanup 0
trap 'exit 130' HUP INT TERM

OS_ID=linux
OS_NAME=Linux
OS_LIKE=
if [ -r /etc/os-release ]; then
    # /etc/os-release is the distribution-defined source of these shell values.
    # shellcheck disable=SC1091
    . /etc/os-release
    OS_ID=${ID:-linux}
    OS_NAME=${PRETTY_NAME:-${NAME:-Linux}}
    OS_LIKE=${ID_LIKE:-}
fi

PACKAGE_MANAGER=
PACKAGES=
COMPILER_PACKAGES=
RUST_PACKAGES=
GIT_PACKAGE=
CMAKE_PACKAGE=
PYTHON_PACKAGE=
PKG_CONFIG_PACKAGE=

select_package_manager() {
    identifiers="$OS_ID $OS_LIKE"
    case " $identifiers " in
        *' debian '*|*' ubuntu '*)
            command -v apt-get >/dev/null 2>&1 || die "apt-get was expected but not found"
            PACKAGE_MANAGER=apt
            PACKAGES='libwayland-dev libxkbcommon-dev libvulkan1'
            COMPILER_PACKAGES=build-essential
            RUST_PACKAGES='cargo rustc'
            GIT_PACKAGE=git
            CMAKE_PACKAGE=cmake
            PYTHON_PACKAGE=python3
            PKG_CONFIG_PACKAGE=pkg-config
            ;;
        *' fedora '*|*' rhel '*|*' centos '*)
            command -v dnf >/dev/null 2>&1 || die "dnf was expected but not found"
            PACKAGE_MANAGER=dnf
            PACKAGES='wayland-devel libxkbcommon-devel vulkan-loader'
            COMPILER_PACKAGES='gcc gcc-c++ make'
            RUST_PACKAGES='rust cargo'
            GIT_PACKAGE=git
            CMAKE_PACKAGE=cmake
            PYTHON_PACKAGE=python3
            PKG_CONFIG_PACKAGE=pkgconf-pkg-config
            ;;
        *' arch '*|*' manjaro '*)
            command -v pacman >/dev/null 2>&1 || die "pacman was expected but not found"
            PACKAGE_MANAGER=pacman
            PACKAGES='wayland libxkbcommon vulkan-icd-loader'
            COMPILER_PACKAGES=base-devel
            RUST_PACKAGES=rust
            GIT_PACKAGE=git
            CMAKE_PACKAGE=cmake
            PYTHON_PACKAGE=python
            PKG_CONFIG_PACKAGE=pkgconf
            ;;
        *' opensuse '*|*' suse '*)
            command -v zypper >/dev/null 2>&1 || die "zypper was expected but not found"
            PACKAGE_MANAGER=zypper
            PACKAGES='wayland-devel libxkbcommon-devel libvulkan1'
            COMPILER_PACKAGES='gcc gcc-c++ make'
            RUST_PACKAGES='rust cargo'
            GIT_PACKAGE=git
            CMAKE_PACKAGE=cmake
            PYTHON_PACKAGE=python3
            PKG_CONFIG_PACKAGE=pkg-config
            ;;
        *)
            if command -v apt-get >/dev/null 2>&1; then
                PACKAGE_MANAGER=apt
                PACKAGES='libwayland-dev libxkbcommon-dev libvulkan1'
                COMPILER_PACKAGES=build-essential
                RUST_PACKAGES='cargo rustc'
                GIT_PACKAGE=git
                CMAKE_PACKAGE=cmake
                PYTHON_PACKAGE=python3
                PKG_CONFIG_PACKAGE=pkg-config
            elif command -v dnf >/dev/null 2>&1; then
                PACKAGE_MANAGER=dnf
                PACKAGES='wayland-devel libxkbcommon-devel vulkan-loader'
                COMPILER_PACKAGES='gcc gcc-c++ make'
                RUST_PACKAGES='rust cargo'
                GIT_PACKAGE=git
                CMAKE_PACKAGE=cmake
                PYTHON_PACKAGE=python3
                PKG_CONFIG_PACKAGE=pkgconf-pkg-config
            elif command -v pacman >/dev/null 2>&1; then
                PACKAGE_MANAGER=pacman
                PACKAGES='wayland libxkbcommon vulkan-icd-loader'
                COMPILER_PACKAGES=base-devel
                RUST_PACKAGES=rust
                GIT_PACKAGE=git
                CMAKE_PACKAGE=cmake
                PYTHON_PACKAGE=python
                PKG_CONFIG_PACKAGE=pkgconf
            elif command -v zypper >/dev/null 2>&1; then
                PACKAGE_MANAGER=zypper
                PACKAGES='wayland-devel libxkbcommon-devel libvulkan1'
                COMPILER_PACKAGES='gcc gcc-c++ make'
                RUST_PACKAGES='rust cargo'
                GIT_PACKAGE=git
                CMAKE_PACKAGE=cmake
                PYTHON_PACKAGE=python3
                PKG_CONFIG_PACKAGE=pkg-config
            else
                die "unsupported distribution; install Rust, Git, CMake, Python 3, a C++ compiler, pkg-config, Wayland, xkbcommon, and a Vulkan loader manually"
            fi
            warn "distribution '$OS_NAME' is not explicitly supported; using detected package manager '$PACKAGE_MANAGER'"
            ;;
    esac
}

package_is_installed() {
    case "$PACKAGE_MANAGER" in
        apt)
            dpkg-query -W -f='${Status}' "$1" 2>/dev/null | grep -q '^install ok installed$'
            ;;
        dnf|zypper)
            rpm -q "$1" >/dev/null 2>&1
            ;;
        pacman)
            pacman -Q "$1" >/dev/null 2>&1
            ;;
        *)
            return 1
            ;;
    esac
}

install_missing_packages() {
    candidates=$PACKAGES
    if ! command -v cc >/dev/null 2>&1 ||
        ! command -v c++ >/dev/null 2>&1 ||
        ! command -v make >/dev/null 2>&1; then
        candidates="$candidates $COMPILER_PACKAGES"
    fi
    if ! command -v cargo >/dev/null 2>&1 || ! command -v rustc >/dev/null 2>&1; then
        candidates="$candidates $RUST_PACKAGES"
    fi
    command -v git >/dev/null 2>&1 || candidates="$candidates $GIT_PACKAGE"
    command -v cmake >/dev/null 2>&1 || candidates="$candidates $CMAKE_PACKAGE"
    if ! command -v python3 >/dev/null 2>&1 && ! command -v python >/dev/null 2>&1; then
        candidates="$candidates $PYTHON_PACKAGE"
    fi
    command -v pkg-config >/dev/null 2>&1 || candidates="$candidates $PKG_CONFIG_PACKAGE"

    missing=
    for package in $candidates; do
        if ! package_is_installed "$package"; then
            missing="$missing $package"
        fi
    done

    if [ -z "$missing" ]; then
        note "Build and runtime dependencies are already installed"
        return
    fi

    note "Installing missing packages:$missing"
    # Package names are selected from fixed, distribution-specific lists above.
    set -- $missing
    case "$PACKAGE_MANAGER" in
        apt)
            as_root apt-get update || die "apt package index refresh failed"
            as_root env DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends "$@" ||
                die "apt could not install the required Forge dependencies"
            ;;
        dnf)
            as_root dnf install -y "$@" ||
                die "dnf could not install the required Forge dependencies"
            ;;
        pacman)
            as_root pacman -S --needed --noconfirm "$@" ||
                die "pacman could not install the required Forge dependencies"
            ;;
        zypper)
            as_root zypper --non-interactive install --no-recommends "$@" ||
                die "zypper could not install the required Forge dependencies"
            ;;
    esac
}

verify_toolchain() {
    command -v git >/dev/null 2>&1 || die "git is unavailable after dependency installation"
    command -v cargo >/dev/null 2>&1 || die "cargo is unavailable after dependency installation"
    command -v rustc >/dev/null 2>&1 || die "rustc is unavailable after dependency installation"
    command -v cmake >/dev/null 2>&1 || die "cmake is unavailable after dependency installation"
    command -v python3 >/dev/null 2>&1 || command -v python >/dev/null 2>&1 ||
        die "Python is unavailable after dependency installation"
    command -v pkg-config >/dev/null 2>&1 || die "pkg-config is unavailable after dependency installation"
}

verify_binary() {
    binary=$1
    [ -f "$binary" ] || die "release build did not produce $binary"
    [ -x "$binary" ] || die "release artifact is not executable: $binary"
    [ ! -L "$binary" ] || die "release artifact unexpectedly resolved to a symbolic link"

    if command -v ldd >/dev/null 2>&1; then
        ldd_output=$TEMP_DIR/ldd.txt
        if ! ldd "$binary" >"$ldd_output" 2>&1; then
            die "unable to inspect release artifact runtime linkage"
        fi
        if grep -q 'not found' "$ldd_output"; then
            cat "$ldd_output" >&2
            die "release artifact has unresolved runtime libraries"
        fi
    fi
}

install_binary() {
    artifact=$1

    if [ -L /opt/forge ] || [ -L "$INSTALL_DIR" ]; then
        die "refusing to install through a symbolic-link installation directory under /opt"
    fi
    if [ -d "$INSTALL_PATH" ]; then
        die "$INSTALL_PATH is a directory; remove or relocate it before installing Forge"
    fi
    if [ -e "$LINK_PATH" ] && [ ! -L "$LINK_PATH" ]; then
        die "$LINK_PATH exists and is not a symbolic link; refusing to overwrite it"
    fi

    as_root install -d -m 0755 /opt/forge "$INSTALL_DIR"

    ROOT_STAGE=$INSTALL_DIR/.forge.new.$$
    as_root rm -f "$ROOT_STAGE"
    as_root install -m 0755 "$artifact" "$ROOT_STAGE"
    as_root test -x "$ROOT_STAGE" || die "staged Forge binary is not executable"
    as_root mv -f "$ROOT_STAGE" "$INSTALL_PATH"
    ROOT_STAGE=

    ROOT_LINK_STAGE=/usr/bin/.forge.link.$$
    as_root rm -f "$ROOT_LINK_STAGE"
    as_root ln -s "$INSTALL_PATH" "$ROOT_LINK_STAGE"
    as_root mv -Tf "$ROOT_LINK_STAGE" "$LINK_PATH"
    ROOT_LINK_STAGE=

    [ -L "$LINK_PATH" ] || die "installation did not create $LINK_PATH"
    [ "$(readlink "$LINK_PATH")" = "$INSTALL_PATH" ] ||
        die "$LINK_PATH does not point to $INSTALL_PATH"
    [ -x "$LINK_PATH" ] || die "installed forge command is not executable"
}

remove_user_tree() {
    path=$1
    description=$2

    [ -e "$path" ] || [ -L "$path" ] || return 0
    case "$path" in
        */forge) ;;
        *) die "refusing to remove unexpected $description path: $path" ;;
    esac

    rm -rf -- "$path" || die "could not remove $description at $path"
    note "Removed $path"
}

remove_runtime_data() {
    if [ -n "${HOME:-}" ]; then
        remove_user_tree "$HOME/.cache/forge" "Forge cache"
        remove_user_tree "$HOME/.local/share/forge" "Forge shell integration data"
    fi

    case ${XDG_CACHE_HOME:-} in
        /*)
            if [ "${XDG_CACHE_HOME%/}/forge" != "${HOME:-}/.cache/forge" ]; then
                remove_user_tree "${XDG_CACHE_HOME%/}/forge" "Forge XDG cache"
            fi
            ;;
    esac

    crash_fallback=/tmp/forge_crash.log
    if [ -f "$crash_fallback" ]; then
        crash_owner=$(stat -c '%u' "$crash_fallback" 2>/dev/null || printf 'unknown')
        if [ "$crash_owner" = "$(id -u)" ]; then
            rm -f -- "$crash_fallback" || die "could not remove $crash_fallback"
            note "Removed $crash_fallback"
        fi
    fi
}

remove_managed_niri_rule() {
    case ${XDG_CONFIG_HOME:-} in
        /*) config_home=${XDG_CONFIG_HOME%/} ;;
        *) config_home=$HOME/.config ;;
    esac
    niri_config=$config_home/niri/config.kdl
    [ -f "$niri_config" ] || return 0

    rule_begin='// Forge terminal compositor blur rule'
    rule_end='// End Forge terminal compositor blur rule'
    grep -Fqx "$rule_begin" "$niri_config" || return 0
    if ! grep -Fqx "$rule_end" "$niri_config"; then
        warn "preserving malformed Forge rule in $niri_config because its end marker is missing"
        return 0
    fi

    USER_STAGE=$(mktemp "${niri_config}.forge-uninstall.XXXXXX") ||
        die "could not stage the Niri configuration cleanup"
    if ! awk -v begin="$rule_begin" -v end="$rule_end" '
        $0 == begin { removing = 1; next }
        removing && $0 == end { removing = 0; next }
        !removing { print }
        END { if (removing) exit 1 }
    ' "$niri_config" >"$USER_STAGE"; then
        die "could not remove the managed Forge rule from $niri_config"
    fi
    chmod --reference="$niri_config" "$USER_STAGE" ||
        die "could not preserve permissions for $niri_config"
    mv -f "$USER_STAGE" "$niri_config" ||
        die "could not update $niri_config"
    USER_STAGE=
    note "Removed Forge's managed Niri blur rule"
}

remove_system_installation() {
    if [ -L "$LINK_PATH" ]; then
        link_target=$(readlink "$LINK_PATH")
        if [ "$link_target" = "$INSTALL_PATH" ]; then
            as_root rm -f "$LINK_PATH" || die "could not remove $LINK_PATH"
            [ ! -e "$LINK_PATH" ] && [ ! -L "$LINK_PATH" ] ||
                die "$LINK_PATH still exists after removal"
            note "Removed $LINK_PATH"
        else
            warn "preserving $LINK_PATH because it points to $link_target"
        fi
    elif [ -e "$LINK_PATH" ]; then
        warn "preserving $LINK_PATH because it is not Forge's symbolic link"
    fi

    if [ -L /opt/forge ] || [ -L "$INSTALL_DIR" ]; then
        warn "preserving a symbolic-link installation directory under /opt"
        return
    fi

    if [ -e "$INSTALL_PATH" ] || [ -L "$INSTALL_PATH" ]; then
        as_root rm -f "$INSTALL_PATH" || die "could not remove $INSTALL_PATH"
        [ ! -e "$INSTALL_PATH" ] && [ ! -L "$INSTALL_PATH" ] ||
            die "$INSTALL_PATH still exists after removal"
        note "Removed $INSTALL_PATH"
    fi
    if [ -d "$INSTALL_DIR" ]; then
        as_root rmdir "$INSTALL_DIR" 2>/dev/null || true
    fi
    if [ -d /opt/forge ]; then
        as_root rmdir /opt/forge 2>/dev/null || true
    fi
    if [ -d /opt/forge ]; then
        warn "preserving non-Forge files remaining under /opt/forge"
    fi
}

uninstall_forge() {
    [ -n "${HOME:-}" ] || die "HOME is required to locate Forge runtime data safely"

    stage "Removing Forge installation"
    remove_system_installation

    stage "Removing Forge runtime data"
    remove_runtime_data
    if [ "$REMOVE_BLUR_RULE" -eq 1 ]; then
        remove_managed_niri_rule
    fi

    case ${XDG_CONFIG_HOME:-} in
        /*) preserved_config=${XDG_CONFIG_HOME%/}/forge ;;
        *) preserved_config=$HOME/.config/forge ;;
    esac
    printf '\n%bForge uninstalled successfully.%b\n' "$GREEN$BOLD" "$RESET"
    printf '  Preserved configuration: %s\n' "$preserved_config"
}

if [ "$ACTION" = uninstall ]; then
    uninstall_forge
    exit 0
fi

stage "Detecting Linux platform"
select_package_manager
note "$OS_NAME ($PACKAGE_MANAGER)"

stage "Checking build and runtime dependencies"
install_missing_packages
verify_toolchain

if [ "${XDG_SESSION_TYPE:-}" != wayland ] && [ -z "${WAYLAND_DISPLAY:-}" ]; then
    warn "no active Wayland session was detected; Forge will require one when launched"
fi

stage "Fetching Forge source"
TEMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/forge-install.XXXXXX") ||
    die "unable to create a temporary build directory"
SOURCE_DIR=$TEMP_DIR/Forge
if [ -n "$REPO_REF" ]; then
    git clone --quiet --depth 1 --branch "$REPO_REF" "$REPO_URL" "$SOURCE_DIR" ||
        die "failed to clone '$REPO_REF' from $REPO_URL"
    note "Checked out $REPO_REF"
else
    git clone --quiet --depth 1 "$REPO_URL" "$SOURCE_DIR" ||
        die "failed to clone $REPO_URL"
    note "Checked out the repository default branch"
fi

stage "Building Forge in release mode"
BUILD_TARGET=$TEMP_DIR/target
(
    cd "$SOURCE_DIR"
    if [ -f Cargo.lock ]; then
        CARGO_TARGET_DIR=$BUILD_TARGET cargo build --release --locked -p forge-main
    else
        CARGO_TARGET_DIR=$BUILD_TARGET cargo build --release -p forge-main
    fi
) || die "Forge release build failed"

ARTIFACT=$BUILD_TARGET/release/$SOURCE_BINARY
stage "Verifying release artifact"
verify_binary "$ARTIFACT"
note "$ARTIFACT"

stage "Installing Forge"
install_binary "$ARTIFACT"

printf '\n%bForge installed successfully.%b\n' "$GREEN$BOLD" "$RESET"
printf '  Binary: %s\n' "$INSTALL_PATH"
printf '  Command: forge\n'
