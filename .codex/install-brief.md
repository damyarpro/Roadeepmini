# Build and install latest Windows app

User explicitly requested building and installing current Roadeep. No source changes are planned. Orchestrator owns build, existing-process stop, installer invocation and final installed-binary verification. Worker owns read-only inspection of installer configuration, existing current-user installation registry and release preconditions. No worker edits or installer execution. Workers are not alone; preserve all existing edits.

Use current-user NSIS install. Do not pass /TRUSTCERT or modify trust stores, account settings, hooks configuration or secrets. Never run uninstall separately or remove user data. Verify exact generated installer path and installed destination, successful installer exit, installed executable hash matching built executable, and responding installed process. Existing feature verification completed: 262 TypeScript tests, 399 Rust tests, four ignored; rerun only necessary release checks.
