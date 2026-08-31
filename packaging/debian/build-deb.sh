#!/bin/sh
# Wrap a static musl rings binary in a tiny .deb (no libc Depends).
# Usage: build-deb.sh [VERSION] [ARCH] [BIN]
#   VERSION  default: $VERSION, else Cargo.toml in the repo
#   ARCH     amd64|arm64|armhf  (default: $ARCH or amd64)
#   BIN      path to the uncompressed musl binary (or $BIN)
# Env: OUTDIR  directory for rings_${VERSION}_${ARCH}.deb (default: cwd)
set -eu

rings_die() {
	echo "rings-deb: $*" >&2
	exit 1
}

script_dir=$(CDPATH= cd "$(dirname "$0")" && pwd)
repo_root=$(CDPATH= cd "$script_dir/../.." && pwd)

version=${1:-${VERSION:-}}
arch=${2:-${ARCH:-amd64}}
bin=${3:-${BIN:-}}

if [ -z "$version" ]; then
	version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$repo_root/Cargo.toml" | head -1)
fi
[ -n "$version" ] || rings_die "VERSION is empty (pass it or set it in Cargo.toml)"
[ -n "$bin" ] || rings_die "path to uncompressed musl binary required"

case "$arch" in
amd64 | arm64 | armhf) ;;
*) rings_die "ARCH must be amd64, arm64, or armhf (got $arch)" ;;
esac

[ -f "$bin" ] || rings_die "binary not found: $bin"
[ -f "$repo_root/LICENSE" ] || rings_die "LICENSE not found at $repo_root/LICENSE"

outdir=${OUTDIR:-.}
mkdir -p "$outdir"
outdir=$(CDPATH= cd "$outdir" && pwd)
deb="$outdir/rings_${version}_${arch}.deb"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

root="$work/pkg"
install -d -m 755 "$root/DEBIAN" "$root/usr/bin" "$root/usr/share/doc/rings"
install -m 755 "$bin" "$root/usr/bin/rings"
install -m 644 "$repo_root/LICENSE" "$root/usr/share/doc/rings/copyright"

# Installed-Size is KiB of the staged payload (not DEBIAN/).
installed_size=$(du -sk "$root/usr" | awk '{print $1}')

cat >"$root/DEBIAN/control" <<EOF
Package: rings
Version: $version
Architecture: $arch
Maintainer: Zach Wilke <zach@pinefall.dev>
Installed-Size: $installed_size
Section: utils
Priority: optional
Homepage: https://github.com/zachwilke/rings
Description: DaisyDisk-style disk usage TUI
 A DaisyDisk-style disk map in one tiny static binary.
EOF
chmod 644 "$root/DEBIAN/control"

# Static musl: do not emit Depends (no libc6).
command -v dpkg-deb >/dev/null 2>&1 || rings_die "dpkg-deb not found"

# --root-owner-group so the archive is root:root even when built as a user.
dpkg-deb --root-owner-group --build "$root" "$deb"
echo "rings-deb: wrote $deb"
