; DisplayGroups hooks for the Tauri NSIS installer (ARCHITECTURE.md §13).
;
; The installer is per-machine (Program Files), as the optional lock-screen service
; requires. "Start with Windows" and the displaygroups:// handler are per-user (HKCU)
; and are offered as yes/no questions in interactive installs only, and only when
; they are not already on. They are written for the account that approved the
; elevation prompt, which is normally the person installing; with over-the-shoulder
; elevation they can be turned on later in Settings.
;
; $UpdateMode is 1 while an older version is removed during an upgrade: per-user
; integration and data are kept then.

!macro DG_STOP_AGENT
  ; The agent and settings app keep their executables open.
  nsExec::Exec 'taskkill /F /IM displaygroupsw.exe'
  Pop $0
  nsExec::Exec 'taskkill /F /IM displaygroups-settings.exe'
  Pop $0
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro DG_STOP_AGENT
!macroend

!macro NSIS_HOOK_POSTINSTALL
  IfSilent dg_post_done

  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "DisplayGroups"
  StrCmp $0 "" 0 dg_startup_done
  MessageBox MB_YESNO|MB_ICONQUESTION "Start DisplayGroups (hotkeys, tray and on-screen display) when you sign in to Windows?" IDNO dg_startup_done
    nsExec::Exec '"$INSTDIR\displaygroups.exe" startup on'
    Pop $0
  dg_startup_done:

  ReadRegStr $0 HKCU "Software\Classes\displaygroups" ""
  StrCmp $0 "" 0 dg_post_done
  MessageBox MB_YESNO|MB_ICONQUESTION|MB_DEFBUTTON2 "Let displaygroups:// links (Stream Deck, macros, automation tools) switch your displays?$\n$\nYou can change this later in Settings." IDNO dg_post_done
    nsExec::Exec '"$INSTDIR\displaygroups.exe" protocol on'
    Pop $0

  dg_post_done:
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro DG_STOP_AGENT
  ${If} $UpdateMode <> 1
    ; Per-user integration of the account running the uninstaller.
    nsExec::Exec '"$INSTDIR\displaygroups.exe" startup off'
    Pop $0
    nsExec::Exec '"$INSTDIR\displaygroups.exe" protocol off'
    Pop $0
    ; Shortcuts made in the app (Start menu folder) point at files about to be removed.
    ; Desktop shortcuts are the user's own files and are left alone.
    SetShellVarContext current
    RMDir /r "$SMPROGRAMS\DisplayGroups"
    SetShellVarContext all
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Tauri's "Delete the application data" checkbox covers the WebView data; the
  ; groups, settings and logs live in DisplayGroups folders of their own.
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    SetShellVarContext current
    RMDir /r "$APPDATA\DisplayGroups"
    RMDir /r "$LOCALAPPDATA\DisplayGroups"
    SetShellVarContext all
  ${EndIf}
!macroend
