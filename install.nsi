; install.nsi – NSIS installer for 冒险岛录屏工具.
;
; Per-user, fully silent install to  %LocalAppData%\Programs\cms_video_recorder\ .
; The uninstaller silently removes the whole install directory, the
; %AppData%\cms_video_recorder\ settings directory and the shortcuts.
;
; The same src/icon.ico that build.rs compiles into cms_video_recorder.exe
; (via the ICON resource in app.rc) is embedded into the installer and the
; uninstaller below, so all three binaries share the same icon.
;
; The installer takes its payload from dist\:
;   dist\cms_video_recorder.exe   – cargo build --release output
;   dist\ffmpeg.exe               – renamed ffmpeg-windows-x86_64.exe from
;                                   HCTOrganization/ffmpeg-build releases
; The release workflow (.github/workflows/release.yml) stages both files.
; For a local build, copy the two files into dist\ yourself, then run:
;   makensis /DVERSION=0.1.0 install.nsi

; ---------------------------------------------------------------------------
; Metadata
; ---------------------------------------------------------------------------
Unicode true

; Keep in sync with `version` in Cargo.toml. CI passes the real value as
; /DVERSION=x.y.z, so it can't drift there.
!ifndef VERSION
    !define VERSION "0.1.1"
!endif
; VIProductVersion wants four 16-bit numbers; Cargo.toml versions are three-part.
!define VERSION4 "${VERSION}.0"

Name "冒险岛录屏工具"
OutFile "dist\cms_video_recorder-${VERSION}-setup.exe"

; ---------------------------------------------------------------------------
; Icons – the same src/icon.ico the program embeds via app.rc.
; ---------------------------------------------------------------------------
Icon "src\icon.ico"
UninstallIcon "src\icon.ico"

; Per-user install into LocalAppData: no UAC prompt.
RequestExecutionLevel user
; Always silent: double-clicking the installer or the uninstaller shows no UI.
SilentInstall silent
SilentUninstall silent

SetCompressor /SOLID lzma

; Version resources, matching the values in app.rc.
VIProductVersion "${VERSION4}"
VIAddVersionKey /LANG=1033 "ProductName" "冒险岛录屏工具"
VIAddVersionKey /LANG=1033 "CompanyName" "Hikari Calyx Tech"
VIAddVersionKey /LANG=1033 "FileDescription" "冒险岛录屏工具 安装程序"
VIAddVersionKey /LANG=1033 "FileVersion" "${VERSION4}"
VIAddVersionKey /LANG=1033 "ProductVersion" "${VERSION4}"
VIAddVersionKey /LANG=1033 "OriginalFilename" "cms_video_recorder-${VERSION}-setup.exe"
VIAddVersionKey /LANG=1033 "InternalName" "cms_video_recorder-setup"

; Fixed per-user install directory. The uninstaller uses the same hard-coded
; path, so nothing has to be looked up in the registry.
!define APP_DIR "$LOCALAPPDATA\Programs\cms_video_recorder"

; ---------------------------------------------------------------------------
; Install
; ---------------------------------------------------------------------------
Section "冒险岛录屏工具" SecInstall
    ; Per-user Desktop / Start Menu.
    SetShellVarContext current

    StrCpy $INSTDIR "${APP_DIR}"

    ; A running instance – or the ffmpeg child it spawned – would lock the
    ; files we are about to overwrite. nsExec runs taskkill hidden (no
    ; console window flash); /F forcefully terminates the process.
    nsExec::Exec 'taskkill /F /IM cms_video_recorder.exe'
    nsExec::Exec 'taskkill /F /IM ffmpeg.exe'

    SetOutPath "$INSTDIR"
    File "dist\cms_video_recorder.exe"
    File "dist\ffmpeg.exe"

    WriteUninstaller "$INSTDIR\uninstall.exe"

    ; Desktop shortcut.
    CreateShortCut "$DESKTOP\冒险岛录屏工具.lnk" "$INSTDIR\cms_video_recorder.exe" \
        "" "$INSTDIR\cms_video_recorder.exe" 0

    ; Start Menu: the app plus the uninstaller.
    CreateDirectory "$SMPROGRAMS\冒险岛录屏工具"
    CreateShortCut "$SMPROGRAMS\冒险岛录屏工具\冒险岛录屏工具.lnk" "$INSTDIR\cms_video_recorder.exe" \
        "" "$INSTDIR\cms_video_recorder.exe" 0
    CreateShortCut "$SMPROGRAMS\冒险岛录屏工具\卸载冒险岛录屏工具.lnk" "$INSTDIR\uninstall.exe" \
        "" "$INSTDIR\uninstall.exe" 0

    ; Launch the freshly installed app. The working directory is $INSTDIR
    ; (SetOutPath above), so the bundled ffmpeg.exe is found next to it.
    Exec '"$INSTDIR\cms_video_recorder.exe"'
SectionEnd

; ---------------------------------------------------------------------------
; Uninstall (always silent – no confirmation pages, deletes everything)
; ---------------------------------------------------------------------------
Section "Uninstall"
    ; Make sure nothing is locking the files we are about to delete.
    nsExec::Exec 'taskkill /F /IM cms_video_recorder.exe'
    nsExec::Exec 'taskkill /F /IM ffmpeg.exe'

    SetShellVarContext current

    ; A running process cannot delete its own file, and the uninstaller lives
    ; inside the install directory. When started from the install directory,
    ; copy ourselves to %TEMP% and re-exec from there first.
    StrCmp "$EXEDIR" "${APP_DIR}" 0 not_from_installdir
        Delete "$TEMP\cms_video_recorder_uninstaller.exe"
        CopyFiles /SILENT "$EXEPATH" "$TEMP\cms_video_recorder_uninstaller.exe"
        Exec '"$TEMP\cms_video_recorder_uninstaller.exe"'
        Quit
    not_from_installdir:

    StrCpy $INSTDIR "${APP_DIR}"

    ; Shortcuts.
    Delete "$DESKTOP\冒险岛录屏工具.lnk"
    RMDir /r "$SMPROGRAMS\冒险岛录屏工具"

    ; Settings and other app data (%AppData%\cms_video_recorder\config.json).
    RMDir /r "$APPDATA\cms_video_recorder"

    ; The whole install directory. We run from %TEMP%, so this also removes
    ; the uninstaller itself.
    RMDir /r "$INSTDIR"

    ; Delete the temp copy of ourselves once this process has exited.
    ; nsExec runs cmd hidden and returns immediately; the ping gives the
    ; process a moment to fully terminate before del runs.
    nsExec::Exec 'cmd /C ping 127.0.0.1 -n 3 >nul & del /f /q "$EXEPATH"'
SectionEnd
