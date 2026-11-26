; Extra installer steps for the NSIS setup.exe.
; Settings live in %APPDATA%\dev.yassine808.islandwin and logs in the logs\ folder beside them, so
; they survive upgrades and uninstalls; only the program itself lives in the install directory.

; "Start with Windows" is on out of the box, like a pre-ticked box. The same switch is on the
; island's Settings page. A silent install (/S) takes the default without asking.
!macro NSIS_HOOK_POSTINSTALL
  IfSilent island_autostart_yes
  MessageBox MB_YESNO|MB_ICONQUESTION "Start IslandWin when Windows starts?$\r$\n$\r$\nThis is on by default and can be changed any time on the island's Settings page." IDYES island_autostart_yes
  Goto island_autostart_done
  island_autostart_yes:
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "IslandWin" '"$INSTDIR\IslandWin.exe"'
  island_autostart_done:
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "IslandWin"
!macroend
