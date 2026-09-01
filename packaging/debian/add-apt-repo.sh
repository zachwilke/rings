#!/bin/sh
# Add the rings apt source (GitHub Pages) and refresh apt.
# Run as root. Does not install the rings package.
#   curl -fsSL https://raw.githubusercontent.com/zachwilke/rings/main/packaging/debian/add-apt-repo.sh | sudo sh
#   sudo apt install rings
set -eu

REPO_URL="https://zachwilke.github.io/rings"
KEY_URL="$REPO_URL/rings-apt.asc"
KEY_URL_FALLBACK="https://raw.githubusercontent.com/zachwilke/rings/main/packaging/debian/rings-apt.asc"
KEYRING="/etc/apt/keyrings/rings.gpg"
LIST="/etc/apt/sources.list.d/rings.list"
SOURCES_SIGNED="deb [arch=amd64,arm64,armhf signed-by=/etc/apt/keyrings/rings.gpg] https://zachwilke.github.io/rings stable main"
SOURCES_TRUSTED="deb [arch=amd64,arm64,armhf trusted=yes] https://zachwilke.github.io/rings stable main"

rings_die() {
	echo "rings-apt: $*" >&2
	exit 1
}

have_cmd() {
	command -v "$1" >/dev/null 2>&1
}

http_get() {
	url=$1
	dest=$2
	if have_cmd curl; then
		curl -fsSL -A "rings-apt" -o "$dest" "$url"
	elif have_cmd wget; then
		wget -q -O "$dest" --user-agent="rings-apt" "$url"
	else
		rings_die "need curl or wget to download the apt key"
	fi
}

http_ok() {
	url=$1
	if have_cmd curl; then
		curl -fsSL -A "rings-apt" -o /dev/null "$url"
	elif have_cmd wget; then
		wget -q -O /dev/null --user-agent="rings-apt" "$url"
	else
		return 1
	fi
}

if [ "${1:-}" = "--print-sources" ]; then
	echo "$SOURCES_SIGNED"
	exit 0
fi

[ "$(id -u)" -eq 0 ] || rings_die "run as root (sudo sh)"

have_cmd gpg || rings_die "need gpg (apt install gnupg)"
if ! have_cmd curl && ! have_cmd wget; then
	rings_die "need curl or wget"
fi

tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT INT TERM

if http_get "$KEY_URL" "$tmp" 2>/dev/null && [ -s "$tmp" ]; then
	:
elif http_get "$KEY_URL_FALLBACK" "$tmp" && [ -s "$tmp" ]; then
	echo "rings-apt: using fallback key $KEY_URL_FALLBACK"
else
	rings_die "could not fetch the public key from $KEY_URL or $KEY_URL_FALLBACK"
fi

install -d -m 755 /etc/apt/keyrings
gpg --dearmor <"$tmp" >"$KEYRING"
chmod 644 "$KEYRING"

# Prefer a signed source. If InRelease / Release.gpg is missing, last resort is trusted=yes.
line=$SOURCES_SIGNED
if http_ok "$REPO_URL/dists/stable/InRelease" 2>/dev/null || http_ok "$REPO_URL/dists/stable/Release.gpg" 2>/dev/null; then
	:
elif http_ok "$REPO_URL/dists/stable/Release" 2>/dev/null; then
	echo "rings-apt: repo is unsigned; using [trusted=yes] (prefer a signed InRelease)"
	line=$SOURCES_TRUSTED
fi

printf '%s\n' "$line" >"$LIST"
chmod 644 "$LIST"

if have_cmd apt-get; then
	apt-get update
elif have_cmd apt; then
	apt update
else
	rings_die "apt-get not found"
fi

echo "rings-apt: wrote $LIST"
echo "next: sudo apt install rings"
