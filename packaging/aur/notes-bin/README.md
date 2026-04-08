# notes-bin AUR packaging

This directory mirrors the packaging files for the `notes-bin` AUR package.

- Preferred install path for Omarchy/Arch users: AUR
- Upstream binary source: GitHub Release `AppImage`
- GitHub Release `AppImage` remains the manual fallback path
- AUR installs disable the in-app updater and rely on Omarchy/AUR updates instead
- The installed launcher always uses `--appimage-extract-and-run` so Arch users do not need FUSE for the packaged path
- On Hyprland/Wayland, the launcher defaults to `WEBKIT_DISABLE_DMABUF_RENDERER=1` because some WebKitGTK AppImage builds fail to create an EGL display
- XWayland is available as an opt-in fallback instead of the default path

Runtime overrides:

- `notes` - normal Omarchy-friendly launch path
- `NOTES_PREFER_X11=1 notes` - opt into XWayland if Wayland still fails on your machine
- `GDK_BACKEND=wayland WEBKIT_DISABLE_DMABUF_RENDERER=0 notes` - fully restore the default Wayland path for testing
- `GDK_BACKEND=x11 WEBKIT_DISABLE_COMPOSITING_MODE=1 notes` - strongest XWayland fallback when EGL keeps failing
- `WEBKIT_DISABLE_COMPOSITING_MODE=0 notes` - disable the X11 compositing workaround if you want to test without it

Before publishing a new AUR release:

1. Update `pkgver` in `PKGBUILD`
2. Run `./update-release-metadata.sh <version>` after the GitHub release has an `.AppImage` asset
3. Regenerate `.SRCINFO` with `makepkg --printsrcinfo > .SRCINFO`
4. Push the updated packaging files to the AUR repo
