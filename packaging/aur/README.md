# AUR `rings-bin`

Binary package for Arch. It wraps the static musl GitHub Release — no rust or cargo.

```bash
yay -S rings-bin
```

## First publish

The AUR repo does not exist until someone with an [AUR account](https://aur.archlinux.org/register) pushes it. One time:

1. Add an SSH key at https://aur.archlinux.org/account/ (`ssh://aur@aur.archlinux.org`).
2. Clone, copy this package in, and push:

```bash
git clone ssh://aur@aur.archlinux.org/rings-bin.git
cp packaging/aur/rings-bin/PKGBUILD packaging/aur/rings-bin/.SRCINFO rings-bin/
cd rings-bin
git add PKGBUILD .SRCINFO
git commit -m "Initial import of rings-bin 0.2.0"
git push
```

## Later versions

After that first push, `.github/workflows/aur.yml` can update the AUR package on `release: published`. It needs repo secrets `AUR_USERNAME` and `AUR_SSH_PRIVATE_KEY` (the AUR SSH private key). If those secrets are unset, the job skips. It does not create them.
