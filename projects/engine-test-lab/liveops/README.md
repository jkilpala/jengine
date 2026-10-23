# The test lab's live ops service

What scene 226 asks (`settings.liveops` in `project.jengine` points at
`http://127.0.0.1:9940` and names this folder's public key).

```powershell
cargo run -p jengine-server -- 127.0.0.1:9940 --admin-token lab --liveops projects/engine-test-lab/liveops
node projects/engine-test-lab/liveops/seed.mjs        # values, an experiment and two events, on every channel
# then open http://127.0.0.1:9940/admin/liveops (token: lab) and change them
```

**`signing.key` is a test key and public on purpose** - it is in the
repository so anyone can run the lab's service and have the lab believe it.
Never build a real game with its public key, and never serve a real game
from this folder: make a service its own folder (`--liveops <dir>`), and
keep that folder's `signing.key` secret and backed up.

The channels and archives the service keeps (`liveops.db`, `blobs/`) are
left out of git.
