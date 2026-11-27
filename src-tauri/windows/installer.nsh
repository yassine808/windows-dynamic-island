; Extra installer steps for the NSIS setup.exe.
; Settings live in %APPDATA%\Windows Dynamic Island and logs in the logs\ folder beside them, so
; they survive upgrades and uninstalls; only the program itself lives in the install directory.

; "Start with Windows" is on out of the box, like a pre-ticked box. The same switch is on the
; island's Settings page. A silent install (/S) must not touch the value: silent installs are
; mostly upgrades and updates, and rewriting Run there silently re-enabled a setting the user had
; turned off.
!macro NSIS_HOOK_POSTINSTALL
  IfSilent island_autostart_done
  MessageBox MB_YESNO|MB_ICONQUESTION "Start IslandWin when Windows starts?$\r$\n$\r$\nThis is on by default and can be changed any time on the island's Settings page." IDYES island_autostart_yes
  Goto island_autostart_done
  island_autostart_yes:
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "IslandWin" '"$INSTDIR\IslandWin.exe"'
    ; an old Task Manager "disabled" marker would keep the entry off
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "IslandWin"
  island_autostart_done:
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "IslandWin"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "IslandWin"
!macroend
