# Space-Track GP History import

Preview an import range using the operator CLI. The start month is inclusive and the end month is exclusive. A preview works without credentials or a database.

```sh
hoth catalog bootstrap --start 2021-11 --end 2026-11 --dry-run
```

For a live import, supply your approved account's `SPACE_TRACK_IDENTITY` and `SPACE_TRACK_PASSWORD` through the process environment using your secret manager. Set an explicit NORAD list and use a catalog location you can retain.

```sh
hoth catalog bootstrap --start 2020-02 --end 2020-04 \
  --norad 25544,43013 --database data/hoth.db
```

The importer accepts up to 50 distinct positive NORAD IDs and 60 completed UTC months. The default database is `data/hoth.db`. Use the same database and NORAD list when resuming. These are HOTH's bounds for targeted queries. Space-Track directs users to its annual archives for many objects or large ranges, to retain previously downloaded history, and to use GP for current ephemerides. Review the provider's current limits before acquisition. [Space-Track API use guidelines](https://www.space-track.org/documentation#api-use-guidelines)

Use your own authorized account and keep its credentials private. Space-Track's user agreement governs transfer and access. The provider describes blanket permission for specified basic SSA products, including TLE and OMM data, with appropriate citation. Check which permission applies before distributing the data or derived analysis. [User agreement](https://www.space-track.org/documentation#user_agree) · [Basic data and redistribution](https://www.space-track.org/documentation#odr)

## Requests and retries

The HTTPS client submits the provider login form to `/ajaxauth/login` and keeps session cookies in memory. Each GP History query filters one month by `EPOCH`, uses the canonical comma-delimited NORAD list, and requests up to 1,000 JSON rows in ascending `GP_ID` order. The cursor requests IDs greater than the last committed ID. The upper epoch bound is the final microsecond of the month. [GP History fields](https://www.space-track.org/basicspacedata/modeldef/class/gp_history/format/html) · [Space-Track client protocol reference](https://spacetrack.readthedocs.io/en/stable/_modules/spacetrack/base.html)

The client waits at least 13 seconds between requests, including login and retries. This keeps a single importer below the published limits of 30 requests per minute and 300 per hour. Coordinate requests across all jobs using the same account. HTTP 429, 5xx, and timeouts get at most three retries with exponential delays. A valid `Retry-After` can extend the delay. If the requested delay exceeds two minutes, the importer reports it and exits so the operator can resume later. A data request that returns 401 gets one session refresh. Authentication failures report a generic error without response bodies, cookies, or credentials.

## Resume and coverage

Each page's orbital rows, acquisition provenance, and checkpoint commit together. A checkpoint records the cursor, observed NORAD IDs, and completion status for the month and canonical object list. A full page advances the cursor. A short or empty page completes the month. A rerun using the same database and object list skips completed months and resumes at the last committed cursor. Only an unfinished page can be fetched again in that run. Use disjoint scopes when adding objects so completed history can be retained without repeated requests.

The importer reports the months it completed or left incomplete and lists requested NORAD IDs with no stored record in that month's committed pages. A completed month can still have missing objects. The acquisition row stores a SHA-256 page digest, and each element stores its exact OMM JSON bytes and record digest. Snapshot selection runs separately against the offline catalog.

The test suite uses injected HTTP replies and a fake delay function. Live authenticated behavior requires an operator's permitted account.
