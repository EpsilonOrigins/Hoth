# SQLite catalog

HOTH stores original orbital-element records, their provenance, and monthly snapshot membership in a SQLite file. The snapshots provide representative populations for scenario preparation.

| Table | Role |
| --- | --- |
| `catalog_acquisitions` | Identifies the acquisition source and its UTC timestamp. An optional source reference can identify a source artifact without credentials. |
| `space_objects` | Holds one row per positive NORAD ID. Name, type, status, and radar cross section in square meters are optional. Metadata and radar cross section can reference separate acquisitions. |
| `orbital_elements` | Stores the source epoch text and the exact OMM or TLE bytes, with the acquisition ID and a lowercase SHA-256 checksum of those bytes. The separately normalized UTC epoch supports chronological queries. |
| `catalog_snapshots` | Identifies a snapshot by a stable ID and a unique `YYYY-MM` target month. |
| `snapshot_members` | Associates one orbital element with each NORAD ID in a snapshot. The element must belong to that same NORAD ID. |

Ingestion validates and emits normalized timestamps as `YYYY-MM-DDTHH:MM:SS.nnnnnnnnnZ`. Fixed-width UTC strings sort chronologically at nanosecond precision. The database checks the text shape. The original epoch text and record bytes retain the source representation. A given NORAD ID, format, and checksum identify a single stored record with its acquisition. Unknown radar cross section stays `NULL`.

The membership primary key supports traversal in NORAD order within a snapshot. Separate indexes support element-to-snapshot lookups and element searches by NORAD ID and epoch or by epoch across the catalog. Deleting a snapshot removes its membership and retains the source elements.

## Migrations and portability

`hoth_catalog::schema::open(path)` opens or creates a writable SQLite file with foreign keys enabled. Use `schema::migrate(&mut connection)` to initialize an existing writable connection outside a transaction. Migration 1 creates the tables and migration 2 adds query indexes. The migrator reads `PRAGMA user_version` under a writer lock and commits all pending changes and version updates together. If a migration fails, the transaction rolls back. A build rejects catalog versions newer than it supports.

The crate builds with bundled SQLite and uses ordinary SQLite files with strict tables. External SQLite tools need SQLite 3.37 or later. Transfer a closed, fully committed catalog or use SQLite's backup facilities to take a consistent copy. Catalog queries and migrations need no network access.

The [synthetic SQL fixture](../../crates/hoth-catalog/tests/fixtures/catalog.sql) shows two source epochs and one snapshot membership. See [ADR 0004](../decisions/0004-sqlite-catalog.md) for the schema decisions.
