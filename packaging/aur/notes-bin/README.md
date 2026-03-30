# notes-bin AUR packaging

This directory mirrors the packaging files for the `notes-bin` AUR package.

- Preferred install path for Omarchy/Arch users: AUR
- Upstream binary source: GitHub Release `AppImage`
- GitHub Release `AppImage` remains the manual fallback path
- AUR installs disable the in-app updater and rely on Omarchy/AUR updates instead
- The installed launcher always uses `--appimage-extract-and-run` so Arch users do not need FUSE for the packaged path

Before publishing a new AUR release:

1. Update `pkgver` in `PKGBUILD`
2. Run `./update-release-metadata.sh <version>` after the GitHub release has an `.AppImage` asset
3. Regenerate `.SRCINFO` with `makepkg --printsrcinfo > .SRCINFO`
4. Push the updated packaging files to the AUR repo
