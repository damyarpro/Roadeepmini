# Installed Windows build

User explicitly requested build and installation. Built with `npm run tauri -- build --bundles nsis`; optimized build and NSIS bundling succeeded. Typecheck/Vite build passed; 20 release-script tests passed. No source edits were necessary.

Installer: `windows/release/Roadeep-Windows-setup.exe` (5,825,163 bytes). SHA256: `8961627ECC75684F1C47988EA7F7A8DB1E5EB39FC6E7662114EC7350CA1E73EE`. Also retained versioned installer `Roadeep-Windows-0.1.1-setup.exe`.

Installed using `/S /UPDATE` to the existing current-user destination `C:/Users/Amir/AppData/Local/Roadeep`. Fresh generated NSIS script was inspected: update mode bypasses the old uninstaller. `/TRUSTCERT` was omitted; existing trust flag remains unchanged. The separate installation of the original upstream app was untouched. Installer exit code was not captured; successful installation was established by updated registry, files and process verification instead.

Both helper executables match built SHA256 values exactly. The installed main executable differs from the built one only at three bytes in Tauri's bundle type marker (`UNK` to `NSS`); every other byte matches. A separate agent confirmed the marker semantics in cached tauri-utils runtime source.

Installed `Roadeep.exe` launched as PID 22000; exact installed path and `Responding=true` verified. Native UI automation is unavailable; process and binary verification were used.
