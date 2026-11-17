# Hidden preparation for an iPhone check

Use `hitl iphone … --launch-env-file FILE --prepare --no-wait` only with the
updated ConveyorCheck build. Older builds and running workers are refused.
The launch receives the ordinary checklist, including item setup and the
same ticket ID. `DeviceCheckStore.isActive` still means the context is active.
Use `canPresentChecklist` for human-facing UI only. Keep `listen()` running
during preparation. If the app has its own waiting cover, remove it on a
matching terminal host message and show `preparationError` if present.

The owner of the ticket calls
`hitl _queues preparation --udid UDID --operation OP --receipt FILE`.
Each JSON receipt includes `ticketID`, `bundleID` and `udid`.

* `delivered`: after successful input copy, include `primaryUserID`, `pid`,
  `bootID`, `epoch` from the verified pre-copy phone receipt.
* `ack`: same tuple, plus the native receipt's `readyAt` in Unix seconds and
  both `fixtureInputsReady: true` and `manualPanelReady: true`.
  The owner must read the receipt afresh from the verified process. The
  command is a trusted-local-owner interface, not a remote attestation API.
* `fail`: include `phase` (short token), `exitCode` (integer or null) and
  `errors` (array of `{domain, code}`); never include raw tool output.

The dispatcher sends permission to the same phone app and then presents the
same points on the Mac. Cancel with the existing `hitl drop ID` command.
Preparation expires 90 seconds after installation; failures are kept in
the ticket verdict and reported as a technical failure.
