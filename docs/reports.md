# Reports: Save to Coordinator

A developer can save a report (a note, this device's diagnostics bundle, up to
five screenshots and one screen recording) on the hosted coordinator straight
from the diagnostics page, instead of saving a file and attaching it somewhere.
Reports stay on the coordinator's disk; nothing goes to a third party.

Only the accounts the operator named with `--diagnostics-user` may save
reports, and only on a coordinator with a `--data-dir` (and without
`--no-reports`). The apps show **Save to Coordinator** only once the
coordinator has said this account may, so it is never greyed out or broken.

## Flow

1. The diagnostics page collects room diagnostics as it opens
   (`Query::RoomDiagnostics`). The coordinator's answer to an account it keeps
   reports for carries a `reportGrant` on part 0 of its `DiagnosticsReply`
   (`wire::ReportGrant { token, expiresInMs }`). The room never relays a grant
   (it rebuilds every reply it passes on with none), and a client takes one
   only from the `coordinator` source.
2. The core holds the grant (never shows it to the platform) and sets
   `RoomDiagnosticsStatus.reportUpload` while it is good and the device is
   connected to that coordinator.
3. The platform shows **Save to Coordinator**. The user writes a note and picks
   attachments; the platform resizes screenshots (longest side at most 2000 px,
   re-encoded as JPEG, which drops their metadata), leaves recordings as they
   are, and sends `Command::StartReportUpload { note, bundle, attachments }`.
4. The core uploads in the background over HTTPS (below), reporting progress
   as `Event::ReportUploadChanged`. Both apps show a bar across the top of the
   window; Android also runs a foreground service with a progress
   notification, so the upload carries on outside the app. The original
   recording goes up even on mobile data.
5. Done: the bar shows the report id (`R-20261101-7k3f`) with Copy. Failed: it
   offers Retry (`Command::RetryReportUpload`, resuming where it stopped) and
   Dismiss (`Command::DismissReportUpload`).

The core keeps the upload's state in `<dataDir>/report-upload/` (`job.json`
plus the bundle text), so an upload interrupted by quitting resumes the next
time the core starts.

## HTTP API (coordinator)

Served on the coordinator's own listener, next to `/ws`; TLS comes from the
reverse proxy in front of it. Clients derive the base from the coordinator URL
they use: `wss://` → `https://` (`ws://` → `http://`) with a trailing `/ws`
path segment removed, so `wss://hocket.example.com/ws` →
`https://hocket.example.com`.

The proxy must let request bodies of at least 9 MB through (uploads go in
4 MiB requests; nginx's default `client_max_body_size` of 1 MB is too small,
Traefik has no limit by default) and keep a stalled request open for at least
a minute.

Errors are JSON `{"error": "<code>", "detail": "<English>"}` with codes
`unauthorized` (401), `notFound` (404), `badRequest` (400), `conflict` (409),
`timeout` (408), `tooLarge` (413), `rateLimited` (429), `disabled` (503), and
`internal` (500) when the coordinator cannot write to its disk. Clients retry
408 and 5xx on their own (resuming from `GET`); the rest are final.

### `POST /reports`

`Authorization: Bearer <grant token>`, JSON body (at most 64 KiB):

```json
{
  "note": "Queue jumped back two songs after unlocking",
  "client": "Hocket Android 0.9.0",
  "device": "Pixel 7 Pro",
  "files": [
    { "kind": "bundle", "size": 812345, "name": "hocket-diagnostics-20261101-1403.txt" },
    { "kind": "screenshot", "size": 402113, "name": "screenshot-1.jpg" },
    { "kind": "recording", "size": 98765432, "name": "screen-20261101-140211.mp4" }
  ]
}
```

- Exactly one `bundle`, first; at most 5 `screenshot`s and 1 `recording`.
- `note` at most 8000 characters; `client`/`device` at most 200; a file's
  `name` at most 255 (it is only recorded, never used as a path).
- Sizes: bundle at most 16 MiB, each screenshot at most 25 MiB, the recording
  at most 1 GiB (413 beyond those). Empty files are refused.
- The grant is good for any number of reports until it expires (6 h), with at
  most 20 new reports per account per hour.
- The coordinator stores each file under a name it picks (`bundle.txt`,
  `screenshot-1.jpg`, `recording.mp4`; the extension comes from the client's
  name when it is a known image or video type, otherwise `.bin`) and writes
  its own process log as `coordinator.log`.
- If the new report would take the reports over `--reports-quota-mb`, the
  oldest finished reports, and unfinished ones nothing has arrived for in an
  hour (abandoned), are deleted until it fits (413 when it cannot).

`201 Created`:

```json
{ "id": "R-20261101-7k3f", "uploadKey": "<64 hex>", "files": [ { "index": 0, "size": 812345, "received": 0 } ] }
```

### `PUT /reports/{id}/files/{index}`

`Authorization: Bearer <uploadKey>`, `Upload-Offset: <bytes already sent>`,
body: the next bytes of file `index` (at most 8 MiB per request; clients send
4 MiB). A mismatched offset gets `409 {"error":"conflict","received":<n>}`;
going past the declared size (or 8 MiB) gets 413 and nothing of that request
is kept. A request whose body stalls for a minute or breaks off keeps what
arrived and answers 408, so `GET` then tells where to resume. Answers
`200 {"received": <n>, "complete": <whole report received>}`.

### `GET /reports/{id}`

`Authorization: Bearer <uploadKey>`. Answers
`200 {"id", "files": [{"index","size","received"}], "complete"}`, which is
how a client resumes.

### `DELETE /reports/{id}`

`Authorization: Bearer <uploadKey>`. Deletes an unfinished report: clients
send it when the user dismisses (or replaces) an upload that has not
finished. `204`; `409` for a finished report, which stays until retention
removes it.

## Storage and retention

```text
<data-dir>/reports/R-20261101-7k3f/
  report.json        id, createdAt, completedAt, username, serverUrl, note,
                     client, device, uploadKeySha256,
                     files [{index, kind, name, file, size, received, deletedAt?}]
  bundle.txt
  screenshot-1.jpg
  recording.mp4
  coordinator.log
```

- Times are epoch milliseconds (UTC). The upload key is kept only as its
  SHA-256.
- Recordings are deleted 30 days after their report was made (the file's
  `deletedAt` is set), everything else after 90 days; unfinished reports
  after 48 hours. The coordinator checks at startup and hourly, by the dates
  in `report.json`.
- `--reports-quota-mb` (default 20480) caps all reports together.

## Reading reports

```sh
hocket-coordinator --data-dir /data reports list
hocket-coordinator --data-dir /data reports show R-20261101-7k3f
```

`list` prints one line per report, newest first. `show` prints the metadata,
the note and the absolute path of every file, so the files can be read or
copied off directly.
