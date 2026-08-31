#!/bin/sh
# Build a Debian apt repo tree from rings_*.deb files.
# Usage: build-apt-repo.sh [DEB_DIR] [OUT_DIR] [VERSION]
#   DEB_DIR  directory containing rings_*.deb (or $DEB_DIR)
#   OUT_DIR  apt tree root (pool/ + dists/ + rings-apt.asc + index.html)
#   VERSION  optional; shown on the index page (else inferred from .deb names)
# Env:
#   RINGS_APT_GPG_PRIVATE_KEY   armored private key (CI secret)
#   RINGS_APT_GPG_PRIVATE_KEY_FILE
#   RINGS_APT_GPG_KEY_ID        optional key id
#   RINGS_APT_GPG_PUBLIC_KEY    path to rings-apt.asc (default: this tree)
# Signs InRelease + Release.gpg when a key is available. Unsigned Release is
# still written (exit 0) with a warning if no key is present.
set -eu

rings_die() {
	echo "rings-apt: $*" >&2
	exit 1
}

rings_warn() {
	echo "rings-apt: $*" >&2
}

have_cmd() {
	command -v "$1" >/dev/null 2>&1
}

script_dir=$(CDPATH= cd "$(dirname "$0")" && pwd)
repo_root=$(CDPATH= cd "$script_dir/../.." && pwd)

deb_dir=${1:-${DEB_DIR:-}}
out_dir=${2:-${OUT_DIR:-}}
version=${3:-${VERSION:-}}

[ -n "$deb_dir" ] || rings_die "DEB_DIR required (directory of .deb files)"
[ -n "$out_dir" ] || rings_die "OUT_DIR required (apt tree output)"
[ -d "$deb_dir" ] || rings_die "deb directory not found: $deb_dir"

deb_dir=$(CDPATH= cd "$deb_dir" && pwd)
mkdir -p "$out_dir"
out_dir=$(CDPATH= cd "$out_dir" && pwd)

sha256_file() {
	if have_cmd sha256sum; then
		sha256sum "$1" | awk '{print $1}'
	elif have_cmd shasum; then
		shasum -a 256 "$1" | awk '{print $1}'
	else
		rings_die "need sha256sum or shasum"
	fi
}

md5_file() {
	if have_cmd md5sum; then
		md5sum "$1" | awk '{print $1}'
	elif have_cmd md5; then
		md5 -q "$1"
	else
		echo "missing"
	fi
}

file_size() {
	wc -c <"$1" | tr -d ' '
}

# Copy every rings_*.deb into the pool. Filename stays rings_${ver}_${arch}.deb.
pool="$out_dir/pool/main/r/rings"
mkdir -p "$pool"
copied=0
for deb in "$deb_dir"/rings_*.deb; do
	[ -f "$deb" ] || continue
	cp -f "$deb" "$pool/"
	copied=$((copied + 1))
	base=$(basename "$deb")
	case "$base" in
	rings_*_*.deb)
		if [ -z "$version" ]; then
			version=$(printf '%s\n' "$base" | sed -n 's/^rings_\(.*\)_[^_][^.]*\.deb$/\1/p')
		fi
		;;
	esac
done
[ "$copied" -gt 0 ] || rings_die "no rings_*.deb files in $deb_dir"

if [ -z "$version" ]; then
	version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$repo_root/Cargo.toml" | head -1)
fi

have_cmd dpkg-scanpackages || rings_die "dpkg-scanpackages not found (apt install dpkg-dev)"

# Packages Filename paths are relative to the apt root (pool/main/r/rings/...).
for arch in amd64 arm64 armhf; do
	bindir="$out_dir/dists/stable/main/binary-$arch"
	mkdir -p "$bindir"
	# dpkg-scanpackages writes to stdout; an arch with no debs still gets an empty Packages.
	(
		CDPATH= cd "$out_dir" || exit 1
		dpkg-scanpackages -a "$arch" pool/main /dev/null
	) >"$bindir/Packages"
	gzip -n -9 -c "$bindir/Packages" >"$bindir/Packages.gz"
done

# Copy (or export) the public key to the apt root.
pub_src=${RINGS_APT_GPG_PUBLIC_KEY:-"$repo_root/packaging/debian/rings-apt.asc"}
if [ -f "$pub_src" ]; then
	cp -f "$pub_src" "$out_dir/rings-apt.asc"
fi

release="$out_dir/dists/stable/Release"
{
	echo "Origin: rings"
	echo "Label: rings"
	echo "Suite: stable"
	echo "Codename: stable"
	echo "Architectures: amd64 arm64 armhf"
	echo "Components: main"
	echo "Date: $(date -u +"%a, %d %b %Y %H:%M:%S +0000")"
	echo "Description: rings apt repository"
	echo "SHA256:"
	(
		CDPATH= cd "$out_dir/dists/stable" || exit 1
		# Portable find: no -printf. Skip the files we are writing now.
		find . -type f ! -name Release ! -name Release.gpg ! -name InRelease | sort | while IFS= read -r f; do
			f=${f#./}
			[ -f "$f" ] || continue
			hash=$(sha256_file "$f")
			size=$(file_size "$f")
			printf ' %s %8s %s\n' "$hash" "$size" "$f"
		done
	)
	echo "MD5Sum:"
	(
		CDPATH= cd "$out_dir/dists/stable" || exit 1
		find . -type f ! -name Release ! -name Release.gpg ! -name InRelease | sort | while IFS= read -r f; do
			f=${f#./}
			[ -f "$f" ] || continue
			hash=$(md5_file "$f")
			size=$(file_size "$f")
			printf ' %s %8s %s\n' "$hash" "$size" "$f"
		done
	)
} >"$release"

# Optional signing. Never print the private key.
gpg_work=
cleanup_gpg() {
	if [ -n "$gpg_work" ] && [ -d "$gpg_work" ]; then
		rm -rf "$gpg_work"
	fi
}
trap cleanup_gpg EXIT INT TERM

imported=0
if [ -n "${RINGS_APT_GPG_PRIVATE_KEY:-}" ] || [ -n "${RINGS_APT_GPG_PRIVATE_KEY_FILE:-}" ]; then
	have_cmd gpg || rings_die "gpg not found (needed to sign Release)"
	gpg_work=$(mktemp -d)
	chmod 700 "$gpg_work"
	export GNUPGHOME="$gpg_work"
	if [ -n "${RINGS_APT_GPG_PRIVATE_KEY_FILE:-}" ]; then
		[ -f "$RINGS_APT_GPG_PRIVATE_KEY_FILE" ] || rings_die "RINGS_APT_GPG_PRIVATE_KEY_FILE not found"
		gpg --batch --import "$RINGS_APT_GPG_PRIVATE_KEY_FILE" >/dev/null 2>&1
	else
		# Env value: pipe into gpg. Do not echo.
		printf '%s\n' "$RINGS_APT_GPG_PRIVATE_KEY" | gpg --batch --import >/dev/null 2>&1
	fi
	imported=1
fi

signed=0
if [ "$imported" -eq 1 ] || [ -n "${RINGS_APT_GPG_KEY_ID:-}" ]; then
	if have_cmd gpg; then
		key_id=${RINGS_APT_GPG_KEY_ID:-}
		if [ -z "$key_id" ]; then
			key_id=$(gpg --batch --list-secret-keys --with-colons 2>/dev/null | awk -F: '/^sec:/ { print $5; exit }')
		fi
		if [ -n "$key_id" ]; then
			gpg --batch --yes --pinentry-mode loopback --digest-algo SHA256 \
				--clearsign -o "$out_dir/dists/stable/InRelease" "$release"
			gpg --batch --yes --pinentry-mode loopback --digest-algo SHA256 \
				--detach-sign -a -o "$out_dir/dists/stable/Release.gpg" "$release"
			signed=1
			# If the committed public key was missing, export it now.
			if [ ! -f "$out_dir/rings-apt.asc" ]; then
				gpg --batch --export --armor "$key_id" >"$out_dir/rings-apt.asc"
			fi
		fi
	fi
fi

if [ "$signed" -eq 0 ]; then
	rm -f "$out_dir/dists/stable/InRelease" "$out_dir/dists/stable/Release.gpg"
	rings_warn "no GPG key (RINGS_APT_GPG_PRIVATE_KEY unset); wrote unsigned Release"
	rings_warn "add-apt-repo.sh will use [trusted=yes] until a key is configured"
fi

# Stop GitHub Pages/Jekyll from rewriting the pool/dists tree.
touch "$out_dir/.nojekyll"

# Tiny landing page (how to add the source + apt install rings).
cat >"$out_dir/index.html" <<EOF
<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>rings apt</title>
<style>
body { font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; margin: 2rem; max-width: 48rem; color: #e8e6e3; background: #1b1b1b; }
a { color: #8cb4ff; }
code, pre { background: #111; color: #f2f0ec; }
pre { padding: 1rem; overflow: auto; }
code { padding: 0.1rem 0.3rem; }
h1 { font-weight: 600; }
p { line-height: 1.5; }
</style>
</head>
<body>
<h1>rings apt</h1>
<p>Signed apt source for <a href="https://github.com/zachwilke/rings">rings</a>${version:+ $version}.</p>
<pre>curl -fsSL https://raw.githubusercontent.com/zachwilke/rings/main/packaging/debian/add-apt-repo.sh | sudo sh
sudo apt install rings</pre>
<p>Sources line:</p>
<pre>deb [arch=amd64,arm64,armhf signed-by=/etc/apt/keyrings/rings.gpg] https://zachwilke.github.io/rings stable main</pre>
<p>Public key: <a href="rings-apt.asc">rings-apt.asc</a>. Architectures: amd64, arm64, armhf.</p>
</body>
</html>
EOF

echo "rings-apt: wrote $out_dir (debs=$copied version=${version:-unknown} signed=$signed)"
