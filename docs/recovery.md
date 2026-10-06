# Recovery

How the owner's tern setup is backed up and how to get it back on a reset or replaced Mac.
This page holds locations only. No secret value belongs here.

## The chain

```
GPG private key (offline, the one thing not on GitHub)
  → gopass store  (github.com:your-account/password-store, auto-pushed)
    → vault passphrase  (gopass: path/to/tern-vault)
      → tern sync backup  (github.com:your-account/tern-sync, age-encrypted)
        → hosts, snippets, settings, keymap, vault
```

Lose the GPG key and nothing below it can be opened. Keep it backed up offline.

A second copy of the vault passphrase, with these restore steps in its notes, is in Bitwarden
(`vault.example.com`) as the item **"tern recovery"**. That path needs only the Bitwarden master
password, not the GPG key.

## What lives where on this Mac

| What | Where | Synced |
| --- | --- | --- |
| Connections, snippets, settings, keymap | `~/Library/Application Support/tern/*.json` | yes, encrypted |
| Saved secrets (vault) | `~/Library/Application Support/tern/vault.age` | yes, encrypted |
| Vault passphrase | gopass `path/to/tern-vault` | via gopass's own repo |
| PIN unlock, Keychain unlock | login Keychain items `tern vault pin`, `tern vault` | no, per Mac |
| Approved password commands | `approved-commands.json` | no, per Mac by design |
| Own wallpaper images | `wallpapers/` | no (the setting syncs, the file does not) |
| Signing identity for local builds | login Keychain identity `tern local` | no; rerun `scripts/make-local-identity.sh` |

## Restore on a fresh Mac

1. Restore the GPG key, clone the gopass store (`gopass clone git@github.com:your-account/password-store.git`).
2. Build and install tern:
   1. `scripts/make-local-identity.sh` (once);
   2. `scripts/bundle-app.sh`, then copy `target/release/bundle/tern.app` to `/Applications`;
   3. `cargo install --locked --path crates/tern` for the CLI.
3. Open tern → Settings → Sync → repository `git@github.com:your-account/tern-sync.git` → Sync now.
   When it asks for the vault passphrase: `gopass show -c path/to/tern-vault` and paste.
4. Settings → Vault: turn on Keychain unlock and set the PIN again.
5. The first time each gopass password command runs, approve it ("Run it? yes").
