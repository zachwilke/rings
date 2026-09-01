#!/bin/sh
# Detect OS/arch and install rings from the latest GitHub Release.
# Mapping (uname -s / uname -m → asset):
#   Linux    x86_64|amd64   rings-x86_64-linux-musl.xz
#   Linux    aarch64|arm64  rings-aarch64-linux-musl.xz
#   Linux    armv7l         rings-armv7-linux-musleabihf.xz
#   Linux    armv6l|armv6   rings-arm-linux-musleabihf.xz
#   Darwin   arm64          rings-aarch64-apple-darwin.xz
#   Darwin   x86_64         rings-x86_64-apple-darwin.xz  (Intel, including Rosetta)
#   Windows  AMD64|x86_64   rings-x86_64-pc-windows-msvc.exe.zip
#   Windows  ARM64          (no asset — fail)
# Dest: RING_PREFIX, else /usr/local/bin (writable or one sudo copy), else ~/.local/bin.
set -eu

REPO="zachwilke/rings"

rings_die() {
	echo "rings-install: $*" >&2
	exit 1
}

# Print the release asset name for an OS/arch pair (uname -s / uname -m style).
rings_asset() {
	os=$1
	arch=$2
	case "$os" in
	Linux)
		case "$arch" in
		x86_64 | amd64) echo "rings-x86_64-linux-musl.xz" ;;
		aarch64 | arm64) echo "rings-aarch64-linux-musl.xz" ;;
		armv7l) echo "rings-armv7-linux-musleabihf.xz" ;;
		armv6l | armv6) echo "rings-arm-linux-musleabihf.xz" ;;
		*) rings_die "unsupported Linux arch: $arch (uname -m)" ;;
		esac
		;;
	Darwin)
		case "$arch" in
		arm64) echo "rings-aarch64-apple-darwin.xz" ;;
		x86_64) echo "rings-x86_64-apple-darwin.xz" ;;
		*) rings_die "unsupported macOS arch: $arch (uname -m)" ;;
		esac
		;;
	Windows)
		case "$arch" in
		AMD64 | amd64 | x86_64) echo "rings-x86_64-pc-windows-msvc.exe.zip" ;;
		ARM64 | arm64)
			rings_die "no Windows ARM64 build yet (arch=$arch); use install.ps1 on x64"
			;;
		*) rings_die "unsupported Windows arch: $arch" ;;
		esac
		;;
	*)
		rings_die "unsupported OS: $os (need Linux, macOS, or Windows)"
		;;
	esac
}

# Typical sudo secure_path: /usr/sbin:/usr/bin:/sbin:/bin:/usr/local/bin
# ~/.local/bin is never on it — `sudo rings` will not find a user-local binary.
rings_on_sudo_path() {
	dir=$(dirname "$1")
	case "$dir" in
	/usr/local/bin | /usr/bin | /bin | /usr/sbin | /sbin | /usr/local/sbin)
		return 0
		;;
	*)
		return 1
		;;
	esac
}

rings_next() {
	dest=$1
	if rings_on_sudo_path "$dest"; then
		echo "next: sudo rings /"
	else
		echo "next: sudo $dest /"
	fi
}

if [ "${1:-}" = "--print-asset" ]; then
	[ $# -eq 3 ] || rings_die "usage: install.sh --print-asset OS ARCH"
	rings_asset "$2" "$3"
	exit 0
fi

if [ "${1:-}" = "--print-next" ]; then
	[ $# -eq 2 ] || rings_die "usage: install.sh --print-next DEST"
	rings_next "$2"
	exit 0
fi

have_cmd() {
	command -v "$1" >/dev/null 2>&1
}

http_get() {
	url=$1
	dest=$2
	if have_cmd curl; then
		curl -fsSL -A "rings-install" -o "$dest" "$url"
	elif have_cmd wget; then
		wget -q -O "$dest" --user-agent="rings-install" "$url"
	else
		rings_die "need curl or wget to download"
	fi
}

http_get_stdout() {
	url=$1
	if have_cmd curl; then
		curl -fsSL -A "rings-install" -H "Accept: application/vnd.github+json" "$url"
	elif have_cmd wget; then
		wget -q -O - --user-agent="rings-install" --header="Accept: application/vnd.github+json" "$url"
	else
		rings_die "need curl or wget to download"
	fi
}

uname_s=$(uname -s)
case "$uname_s" in
Linux) os=Linux ;;
Darwin) os=Darwin ;;
MINGW* | MSYS* | CYGWIN* | Windows_NT)
	rings_die "Windows: irm https://raw.githubusercontent.com/zachwilke/rings/main/install.ps1 | iex"
	;;
*)
	rings_die "unsupported OS: $uname_s (need Linux, macOS, or Windows)"
	;;
esac

arch=$(uname -m)
asset=$(rings_asset "$os" "$arch")

if [ -n "${RING_VERSION:-}" ]; then
	tag=$RING_VERSION
	case "$tag" in
	v*) ;;
	*) tag="v$tag" ;;
	esac
	api="https://api.github.com/repos/${REPO}/releases/tags/${tag}"
else
	api="https://api.github.com/repos/${REPO}/releases/latest"
fi

json=$(http_get_stdout "$api") || rings_die "failed to fetch $api"
# Works on both pretty-printed and minified GitHub JSON (no jq).
tag=$(printf '%s\n' "$json" | tr ',' '\n' | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)
[ -n "$tag" ] || rings_die "could not read tag_name from GitHub release JSON"

case "$tag" in
*[!A-Za-z0-9._-]*) rings_die "unexpected release tag: $tag" ;;
esac

printf '%s\n' "$json" | grep -F "\"$asset\"" >/dev/null \
	|| rings_die "release $tag has no asset $asset"

url="https://github.com/${REPO}/releases/download/${tag}/${asset}"

tmp=$(mktemp -d 2>/dev/null || mktemp -d -t rings-install)
trap 'rm -rf "$tmp"' EXIT INT TERM

archive="$tmp/$asset"
http_get "$url" "$archive"
[ -s "$archive" ] || rings_die "download was empty: $url"

bin="$tmp/rings"
if have_cmd xz; then
	xz -d -c "$archive" >"$bin"
elif have_cmd xzcat; then
	xzcat "$archive" >"$bin"
else
	rings_die "need xz to decompress .xz
  Debian/Ubuntu/Raspbian:  apt install xz-utils
  RHEL/Fedora:             dnf install xz
  macOS:                   brew install xz"
fi
[ -s "$bin" ] || rings_die "decompressed binary is empty"
chmod +x "$bin"

# Place $1 at /usr/local/bin/rings. Optional $2 is a sudo flag (e.g. -n).
rings_sudo_copy() {
	src=$1
	nflag=${2:-}
	if ! have_cmd sudo; then
		return 1
	fi
	if have_cmd install; then
		if [ -d /usr/local/bin ]; then
			if sudo $nflag install -m 755 "$src" /usr/local/bin/rings; then
				return 0
			fi
			return 1
		fi
		if sudo $nflag sh -c 'mkdir -p /usr/local/bin && install -m 755 "$1" /usr/local/bin/rings' _ "$src"; then
			return 0
		fi
		return 1
	fi
	if sudo $nflag sh -c 'mkdir -p /usr/local/bin && cp "$1" /usr/local/bin/rings && chmod 755 /usr/local/bin/rings' _ "$src"; then
		return 0
	fi
	return 1
}

# Confirm (TTY only) then copy with sudo. Never sudo the GitHub download.
rings_try_usr_local() {
	src=$1
	if [ -t 0 ]; then
		printf 'Install to /usr/local/bin so sudo rings works? [Y/n] ' >&2
		ans=
		read -r ans || ans=n
		case "$ans" in
		'' | [Yy] | [Yy][Ee][Ss]) ;;
		*) return 1 ;;
		esac
	fi
	if ! have_cmd sudo; then
		return 1
	fi
	# Cached credentials (curl | sh has no stdin TTY).
	if sudo -n true >/dev/null 2>&1; then
		if rings_sudo_copy "$src" -n; then
			return 0
		fi
	fi
	# sudo can prompt on /dev/tty even when this script's stdin is the pipe.
	if rings_sudo_copy "$src"; then
		return 0
	fi
	return 1
}

rings_ensure_local_path() {
	bindir=$1
	case ":$PATH:" in
	*":$bindir:"*) return 0 ;;
	esac
	rc=
	shellname=${SHELL##*/}
	case "$shellname" in
	bash) rc="$HOME/.bashrc" ;;
	zsh) rc="$HOME/.zshrc" ;;
	esac
	line="export PATH=\"$bindir:\$PATH\""
	if [ -n "$rc" ]; then
		if [ -f "$rc" ] && grep -F "$bindir" "$rc" >/dev/null 2>&1; then
			echo "note: $bindir is not on PATH in this shell — open a new shell"
			return 0
		fi
		printf '%s\n' "$line" >>"$rc"
		echo "note: added $bindir to PATH in $rc — open a new shell"
		return 0
	fi
	echo "note: $bindir is not on PATH — add it, or run $bindir/rings"
}

fallback=0
if [ -n "${RING_PREFIX:-}" ]; then
	mkdir -p "$RING_PREFIX"
	dest="$RING_PREFIX/rings"
	cp "$bin" "$dest"
	chmod +x "$dest"
elif [ -d /usr/local/bin ] && [ -w /usr/local/bin ]; then
	dest=/usr/local/bin/rings
	cp "$bin" "$dest"
	chmod +x "$dest"
elif rings_try_usr_local "$bin"; then
	dest=/usr/local/bin/rings
else
	mkdir -p "$HOME/.local/bin"
	dest="$HOME/.local/bin/rings"
	cp "$bin" "$dest"
	chmod +x "$dest"
	fallback=1
fi

echo "installed $dest ($tag)"
"$dest" --version

if [ "$fallback" -eq 1 ]; then
	rings_ensure_local_path "$HOME/.local/bin"
	rings_next "$dest"
	echo "to get sudo rings: sudo install -m 755 $dest /usr/local/bin/rings"
else
	bindir=$(dirname "$dest")
	case ":$PATH:" in
	*":$bindir:"*) ;;
	*)
		echo "note: $bindir is not on PATH — add it, or run $dest"
		;;
	esac
	rings_next "$dest"
fi
