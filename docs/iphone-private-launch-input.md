# Private input for an iPhone check

`hitl iphone --check ... --launch-env-file /absolute/private/input.json` passes private values only to the device launch of that queued check. The app must already support reading the named variables. This option does not add a login mechanism to an app.

The input is a regular file owned by the current user, mode `0600`, with exactly one hard link. Its immediate parent must be an owned `0700` directory, not a symlink. For example, using placeholder values:

```json
{
  "version": 1,
  "bundleID": "dev.example.review",
  "udid": "EXACT-PHYSICAL-DEVICE-ID",
  "variables": {
    "APP_REVIEW_EMAIL": "disposable@example.invalid",
    "APP_REVIEW_PASSWORD": "placeholder-only"
  }
}
```

The JSON has exactly these four fields. The bundle and device must match the build result and queue exactly. At most 16 variables, 8192 bytes per nonempty string value, and 65536 bytes for the complete snapshot are accepted. NULs, unsafe names and process loader/system environment overrides are rejected.

Before building, the host validates the file. The builder receives neither its contents nor its path. Submission revalidates it and creates one owned `0600` snapshot under the device queue's `private-launch/` directory (`0700`). Queue state contains only an opaque reference and format version. The caller may remove its source after successful submission; changing the source does not change the snapshot.

Before installing and launching, the dispatcher checks the snapshot's ticket ID, bundle, device, version, ownership and permissions. A mismatch fails without installing. The worker supplies `DEVICECTL_CHILD_` variables only on the current `devicectl` child command, never in arguments or the dispatcher's environment. Ambient `DEVICECTL_CHILD_*` values are removed from worker, builder and device children, including checks with no private input. Both attached and unattached launches use this boundary. Private launch errors suppress raw tool output; checklist report strings are redacted before persistence.

A new queue worker holds a versioned capability lock. Submission refuses private input while an older worker owns the device queue. The normal source-change restart upgrades the worker without ending its active check. Do not force-stop an unrelated check to upgrade it.

Pause, requeue and worker restart retain a live ticket's snapshot. Completion, cancellation, supersession and terminal installation/launch failure remove it. Locked queue operations prune only orphaned snapshots; recent verdicts do not retain private input. A failed private-file validation also fails closed. Files are unlinked, not guaranteed securely erased from APFS snapshots or backups.

Use disposable accounts. Do not put real private values in checklist text, command arguments, scripts, repositories or bug reports. This protects transport into the app; it cannot prevent the app itself from persisting or exposing a value it receives.
