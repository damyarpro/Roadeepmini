; Installer hooks for the NSIS installer.

!include "FileFunc.nsh"

; ── Trusting Roadeep's self-signed publisher certificate ─────────────────────
;
; The app and this installer are signed with a self-signed code-signing
; certificate (CN=Roadeep), which Windows does not know. After installing we
; OFFER to trust it for the current user (Trusted Root + Trusted Publishers),
; so Windows shows "Roadeep" as a verified publisher for this and later
; versions. It is never done silently: interactive installs ask (default No,
; and Windows asks once more for the root store); silent installs only do it
; when started with /TRUSTCERT. Only this one certificate is added: it is an
; end-entity code-signing certificate (no CA rights), so it cannot vouch for
; websites or other publishers. Uninstalling removes it again, but only if this
; installer added it (the registry flag below).
!define ROADEEP_CERT_FILE "Roadeep-code-signing.cer"
; Resolved here, at include time: inside a macro ${__FILEDIR__} would be the
; folder of Tauri's generated installer.nsi instead of this one.
!define ROADEEP_CERT_SOURCE "${__FILEDIR__}\${ROADEEP_CERT_FILE}"
!define ROADEEP_CERT_THUMBPRINT "1B4B79A94C52771D0218C962465F2D52968E6684"
!define ROADEEP_CERT_FLAG_KEY "Software\Roadeep"
!define ROADEEP_CERT_FLAG "TrustedPublisherCert"

!macro ROADEEP_TRUST_CERT
  InitPluginsDir
  SetOutPath "$PLUGINSDIR"
  File "${ROADEEP_CERT_SOURCE}"
  ExecWait '"$SYSDIR\certutil.exe" -user -f -addstore Root "$PLUGINSDIR\${ROADEEP_CERT_FILE}"' $0
  ${If} $0 == 0
    ExecWait '"$SYSDIR\certutil.exe" -user -f -addstore TrustedPublisher "$PLUGINSDIR\${ROADEEP_CERT_FILE}"' $1
    WriteRegDWORD HKCU "${ROADEEP_CERT_FLAG_KEY}" "${ROADEEP_CERT_FLAG}" 1
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ${GetParameters} $R0
  ClearErrors
  ${GetOptions} $R0 "/TRUSTCERT" $R1
  ${IfNot} ${Errors}
    !insertmacro ROADEEP_TRUST_CERT
  ${ElseIfNot} ${Silent}
    ; Right-to-left for the Persian; the English paragraph is wrapped in an
    ; LRE…PDF embedding, plus an LRM after each of its full stops, since the
    ; message box wraps lines before applying the embedding.
    MessageBox MB_YESNO|MB_ICONQUESTION|MB_DEFBUTTON2|MB_RTLREADING|MB_RIGHT "ناشر «Roadeep» روی این کامپیوتر مورد اعتماد شود؟$\r$\n$\r$\nرودیپ با یک گواهی خودامضا امضا شده است. اگر «بله» را بزنی، ویندوز این امضا را می‌شناسد و رودیپ و نسخه‌های بعدی‌اش با نام ناشر Roadeep نشان داده می‌شوند. ویندوز یک بار دیگر هم خودش تأیید می‌گیرد.$\r$\nفقط وقتی «بله» را بزن که این فایل نصب را از خود رودیپ گرفته‌ای. رودیپ بدون این کار هم کامل کار می‌کند.$\r$\n$\r$\n‪Trust the publisher “Roadeep” on this computer?‎ Roadeep is signed with a self-signed certificate.‎ ‎Choose Yes only if you got this installer from Roadeep itself; the app works fully without it.‎‬" IDNO roadeep_skip_cert
    !insertmacro ROADEEP_TRUST_CERT
    roadeep_skip_cert:
  ${EndIf}
!macroend

; The app stages roadeep-hook.exe into %LOCALAPPDATA%\com.roadeep.desktop\bin at launch, so the
; installer never recorded it and the default uninstaller leaves it behind. The
; inbox and the log live in the same place and are ours too.
;
; Claude Code's own settings.json is deliberately NOT touched here: it belongs to
; the user, it may contain hooks from other tools, and rewriting somebody's
; config from an uninstaller with no diff and no consent is exactly what the rest
; of this app goes out of its way not to do. A relay that is gone exits 0 without
; printing anything, so a leftover entry costs nothing beyond a dead path.

!macro NSIS_HOOK_PREUNINSTALL
  RMDir /r "$LOCALAPPDATA\com.roadeep.desktop\bin"
  RMDir /r "$LOCALAPPDATA\com.roadeep.desktop\inbox"
  Delete "$LOCALAPPDATA\com.roadeep.desktop\roadeep.log"
  ; Relays staged by interim builds inside the install folder.
  RMDir /r "$INSTDIR\bin"
  ; Only the certificate this installer added, and only if it added it.
  ReadRegDWORD $0 HKCU "${ROADEEP_CERT_FLAG_KEY}" "${ROADEEP_CERT_FLAG}"
  ${If} $0 == 1
    ExecWait '"$SYSDIR\certutil.exe" -user -delstore TrustedPublisher ${ROADEEP_CERT_THUMBPRINT}'
    ExecWait '"$SYSDIR\certutil.exe" -user -delstore Root ${ROADEEP_CERT_THUMBPRINT}'
    DeleteRegValue HKCU "${ROADEEP_CERT_FLAG_KEY}" "${ROADEEP_CERT_FLAG}"
  ${EndIf}
!macroend
