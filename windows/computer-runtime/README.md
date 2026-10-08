# Isolated agent computer

The terminal provides the image's Linux shell and Node.js only; other development toolchains are not promised. It has no direct network, and no files from the host are imported automatically. Workspace files are ephemeral and are destroyed by stop/reset.

Explicit setup: install/start Docker Desktop with Linux containers, then build the checked-in runtime:

```powershell
docker build --tag roadeep-computer:1 computer-runtime
```

Run that command from the `windows` directory. Installed builds show a copyable command using their packaged runtime directory. The app never pulls, builds, starts Docker or creates containers automatically. The explicit build downloads the pinned Playwright image/package and can be large. This source uses matching Playwright package/image 1.63.0, from the [official Docker documentation](https://playwright.dev/docs/docker).

Start is per selected agent, including the default chat agent. The app permits four computers. Each has a networkless container, non-root UID 1000, read-only root filesystem, dropped capabilities, no new privileges, private IPC, no published ports, no host mounts/socket, 768 MiB memory, one CPU and 128 PIDs. The ephemeral `/workspace` is 128 MiB and `/tmp` is separately bounded. Stop/reset destroys the workspace; reset starts a fresh computer. Pause freezes its processes. Human takeover resumes a paused container and prevents agent operations until control is returned. Explicit human operations require takeover.

Browser HTTP(S) resources pass through the native public-address DNS-pinned fetch transport. Private/local/link-local/metadata addresses, credentials in URLs, nonstandard ports, WebSockets and service workers are blocked. Redirect chains are limited to three; each fetch is limited to one MiB and ten seconds, operations to 45 seconds. The terminal has no network and executes only inside the container with a 15-second process-group deadline and 64 KiB output budget. Browser viewport is 1024 × 640. Native UI receives bounded PNG images; chat tools get bounded visible text/interactive coordinates, never giant image strings or page HTML. Downloads/file uploads, audio, persistent browser profiles, and arbitrary host access are unavailable.

Every effectful model operation enters the existing chat Ask approval path. The tool schema cannot choose agent/container identity. Reads/status/screenshots are Auto only when the user's tool chip is enabled and this agent's computer was explicitly started. Native Docker commands use an absolute executable and fixed local daemon endpoint; inherited Docker environment overrides cannot redirect them to a remote engine. Native CLI process trees are contained in Windows Jobs and killed on cancellation/timeouts. Container commands have their own runtime deadlines because killing the CLI alone does not stop daemon-owned work.

App exit attempts ownership-validated parallel cleanup with the app's bounded exit deadline. Abrupt process/OS termination may prevent cleanup: the runtime exits after fifteen minutes idle, leaving an exited container (no restart policy) until Docker cleanup. An operation that was running has its own deadline. Never assume an abrupt exit completed cleanup. Docker is a kernel/container security boundary, not a dedicated virtual machine; Chromium sandbox is disabled inside the restricted container because Docker's default seccomp blocks the browser's namespace sandbox. No host IPC/capability/seccomp exemptions are added.

Logs contain operation ID, hashed agent identity, duration and safe outcome code, not URLs, commands, file bodies, headers or screenshots. Image/runtime unavailability and ownership failures are visible; startup never substitutes mock success.

Tests (no daemon, browser install, network or image download):

```powershell
node --test computer-runtime/policy.test.mjs
cargo test -p roadeep computer:: --lib --offline
```

See [Docker run isolation flags](https://docs.docker.com/reference/cli/docker/container/run) and [tmpfs behavior](https://docs.docker.com/engine/storage/tmpfs): tmpfs may be swapped by the host OS. Real browser/container launch requires a running Docker daemon plus the explicitly built local image and is separately verified when available.
