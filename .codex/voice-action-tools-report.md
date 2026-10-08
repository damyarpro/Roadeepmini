# Voice task execution

User confirmed voice capture works. The new requirement is actual task execution, with sensitive actions excluded.

## Contract

Administrator verification precedes transcription and voice chat submission. Fixed desktop actions execute natively before local model routing, so opening This PC does not need an API account. General requests continue through the existing local/API chat route. Voice API turns receive an explicit safe builtin tool catalogue, independent of external MCP connections. Native identity checks enforce the catalogue again at execution; untrusted tool annotations cannot grant authority. Server permission requests in voice turns must stop the job, without approval.

The desktop input contract accepts only complete single-intent requests for enumerated destinations. It never interprets model output as a host shell command, arbitrary path, URL or executable. Results acknowledge successful process dispatch, not completion of arbitrary work inside another application. Failed launches must produce an error reply rather than success text.

## Voice assistant modes

Settings → Assistant offers manual (default) or explicitly opted-in always-ready listening. Manual processes one utterance; always-ready rearms and starts when the app/runtime/admin reference are ready. The microphone button pauses listening for the current session. Enrollment, shutdown, profile removal and runtime disable cancel capture and inference.

Verified speech is analyzed locally. Tasks and notes become one expiring proposal, with its complete text and Save/Reject controls. Exact contextual spoken decisions can resolve only that local proposal. Nothing is saved beforehand. Storage failure retains the proposal for retry; concurrent confirmation saves once. Ambient unrelated speech stays silent and does not use the API. Addressed questions and fixed Windows commands use the safe voice chat/action route.

## Verification

Root independently passed TypeScript, 433 frontend tests in54 files, 29 release tests, 461 native application tests and20 MCP tests.16 environment-dependent native tests remain ignored by default. Explicitly invoked checks passed the actual Persian model's task/note content and ambient silence, the local canned brain response, and real fixed This PC process dispatch. Dispatch does not prove arbitrary work inside Explorer.

Root independently ran the public speaker subprocess regression: three padded same-speaker enrollment fixtures passed; held-out same speaker0.8379 exceeded0.7740, while other speakers0.3133/0.0838 were rejected. No microphone or administrator reference was accessed by the tests.

Three fresh independent UI critics reviewed three iterations. Final10 criteria scored9/10 with no Critical/Major defects. Initial bounded-container captures were superseded by actual375/768/1440 viewport captures in iterations2/3. Synthetic previews verify interface behavior, not real microphone/API/store behavior. Reports and screenshots are in `voice-assistant-review/`.

Full offline0.1.4 release built and installed successfully (installer exit0). Installed executable reports0.1.4, matches the NSIS release binary and remains responding after launch. All518 runtime receipt files verified by size and SHA256. All11 pinned bundle assets also verified (1,671,023,349 bytes). Installer1,640,989,263 bytes; SHA256 `a78232ea6d82a83d7391265ca5e6ede7e7df82cbb1978414f89bcf5e44d70211`; installed executable SHA256 `a2d7711f584ee6f24e7e9ecdc6cf3825f6ce7626646c196c21e0243f37d0cf96`. The versioned and rolling installer hashes agree.

Post-install speaker subprocess check against the actual installed0.1.4 executable passed5.95s using public fixtures, preserving the reported same/other-speaker scores and padded-boundary acceptance. No user reference or microphone was accessed.

## Limits

Voice can use only installed, explicitly supported safe tools. General cloud planning requires an existing Roadeep account and connection. Sensitive or unknown remote/computer tools are not voice-enabled. Existing Codex sessions remain observed only; replies and permissions remain in Codex. No arbitrary desktop command execution is offered.
