# Automatic apps after clean Windows installation

The owner has explicitly requested automatic UltraViewer, Chrome and 7-Zip
installation. This supersedes the previous decision to omit all first-logon apps.

The worker is embedded in qsu_common/src/post_install.ps1 and written to the
OFFLINE Windows target at ProgramData/QSU/PostInstall.ps1. Both the wizard's
WinPE engine and the interactive WinPE installer stage it. FirstLogonCommands
starts it in the background at the first administrative logon. Desktop startup
is not held waiting for downloads. In the WinPE installer, leave the automatic
Windows setup checkbox enabled. This feature applies to clean installs.

Internet access is required in the new Windows. UltraViewer is attempted first,
then Google Chrome, then 7-Zip. Existing installed executables are skipped.
UltraViewer and Chrome require valid vendor signatures; 7-Zip 26.06 x64 requires
a pinned SHA256 verified against the official download linked by 7-zip.org.
The official vendor silent switches are used, without forcing a restart.

Each app has an independent result. A failed app does not block the other two.
A scheduled task retries every five minutes for one hour and at the same admin's
next logon, capped at twelve worker attempts. The task is removed on success.
A global mutex prevents duplicate workers, and a saved live installer PID
prevents starting another copy after an installer timeout.

Logs and state: %ProgramData%/QSU/PostInstall.log, PostInstall-status.json,
UltraViewer-install.log, Chrome-install.log and PostInstall-attempts.txt.

Manual retry on the installed client: in elevated PowerShell, remove only
%ProgramData%/QSU/PostInstall-attempts.txt to reset the retry cap, then run:

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$env:ProgramData\QSU\PostInstall.ps1"

The worker does not configure remote passwords or claim external remote access.
The user-supplied optional PostInstall.exe hook and custom folder remain separate.
No standalone post_install Rust crate is needed.

Non-installing development checks:

powershell -NoProfile -ExecutionPolicy Bypass -File packaging/test-post-install.ps1
cargo +stable-x86_64-pc-windows-msvc test --manifest-path rust/Cargo.toml -p qsu_common --target x86_64-pc-windows-msvc --offline

Do not run the real worker normally on the development PC.

## Telegram completion notification

Configure the destination locally before starting an installation:

powershell -NoProfile -ExecutionPolicy Bypass -File packaging/configure-telegram.ps1

Run elevated. The script asks for chat_id and a hidden bot token, writes only
%ProgramData%/QSU/Telegram.json with SYSTEM/Administrators ACLs, and sends no test
message. This file is excluded from Git and binary packaging. The Windows wizard
copies it into the protected data staging folder; the engine then copies it into
the new Windows. The WinPE preparation action can copy it beside the PE tool for
the interactive path. NTFS permissions must be supported by the destination.
Keep `configure-telegram.ps1` from the source checkout beside the package when
configuring the technician PC; credentials are deliberately not embedded in
`UtilityRebuild.exe`. The current bot is `@gfwin_bot`; configure the technician
chat ID before installation when notifications are wanted.

After application setup succeeds, the worker sends the machine name, Windows
build and each app's result. If the application retry budget is exhausted it
sends SETUP_INCOMPLETE instead. No passwords, tokens, raw error logs or claimed
remote connection IDs are included. Telegram-outbox.json stores a stable event
ID, pending/delivered state and the API message_id. The retry task stays while
delivery is pending. Retries respect retry_after and use bounded backoff.
Bad credentials/chat permissions stop automatic notification retries and are
reported in the local log. Direct Bot API delivery may duplicate a message if
Telegram accepted it but the response was lost; the event ID identifies this.

Telegram delivery was verified with the technician chat: chat ID `8391336433`,
message ID `2`. The bot token is not stored in the source tree or binary package.
