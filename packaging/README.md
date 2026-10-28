# Packaging

Installers are built by `.github/workflows/release.yml` whenever a version
tag is pushed, and attached to that tag's GitHub release:

```
git tag v0.2.0
git push origin v0.2.0
```

Bump `version` in `Cargo.toml` first, so the programs report the same number.
The workflow can also be started by hand from the Actions tab; it then only
keeps the installers as downloadable build artifacts.

| System | File | Built with |
|---|---|---|
| Windows | `neopsych-<version>-windows-setup.exe` | Inno Setup, `windows/neopsych.iss` |
| Ubuntu / Debian | `neopsych_<version>-1_amd64.deb` | `cargo deb`, settings in `Cargo.toml` |
| macOS | `neopsych-<version>-macos.dmg` | `macos/bundle.sh` (Apple Silicon and Intel in one app) |

The Windows installer installs for the current user without administrator
rights, adds Start menu entries and offers to set neopsych as the screensaver.
The macOS app is not signed, so the first start needs right-click → Open.
