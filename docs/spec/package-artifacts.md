# Package Artifacts

The `xdg-thumbnail` package installs the `xdg-thumbnail-generate` and `xdg-thumbnail-prune` CLI binaries, generated shell completions, generated section 1 man pages, and platform-specific runtime integration artifacts documented here.

## Linux Systemd User Units

On Linux systems, the package installs systemd user units under `share/systemd/user/`:

- `xdg-thumbnail-prune.service`
- `xdg-thumbnail-prune.timer`

Users may enable automatic thumbnail cache pruning with:

```text
systemctl --user enable --now xdg-thumbnail-prune.timer
```

The timer runs the prune service daily, catches up a missed run after the user manager becomes available, and randomizes each run by up to one hour to avoid predictable startup work.

The service runs `xdg-thumbnail-prune --delete` as a one-shot user service. This intentionally uses the prune CLI defaults for all other policy choices: scan successful thumbnail namespaces only, use the default 30-day threshold for age-based remote, virtual, and removable entries, use access time as the age basis, skip failure entries, and report stale local thumbnails without deleting them.

The installed service must execute the `xdg-thumbnail-prune` binary from the same package output rather than relying on the user's ambient `PATH`.
