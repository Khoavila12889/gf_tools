# post_install — intentionally NOT built

This folder previously held the `post_install` crate: a first-logon agent
that would run once after a clean Windows installation to restore drivers,
silently install applications, configure a remote-access tool with an
unattended password, and clean up the staging area.

**That component is intentionally not built in this rebuild.** Unattended
first-logon automation — silent software installation and remote-access
credential configuration in particular — is left to the machine owner's own
tooling and policies, not baked into this utility.

What happens instead:

- The install wizard (`cai_dat_win`) writes `stage.json` **without** any
  app-install entries and without any remote-access password.
- `winpe_install` treats the post-install agent as optional: if a
  `PostInstall.exe` file happens to be present in the staging folder it is
  copied into the new image and registered under RunOnce (so users who
  supply their own agent can still use the hook); if it is absent, the step
  is skipped silently and the WinPE log records
  "no post-install agent staged".
- Users who want their own first-logon scripts can drop a folder named
  `custom` next to the wizard executable (`CaiDatWinTuDong.exe`). The
  wizard copies it verbatim into the staging area, and `winpe_install`
  copies it into the new Windows image under
  `C:\ProgramData\QSU\custom`. The utility never interprets or executes
  the contents — wiring them up (RunOnce, scheduled task, logon script…)
  is entirely the owner's choice.

The crate has been removed from the workspace members in
`rust/Cargo.toml`; this README is kept so the empty folder documents the
decision.
