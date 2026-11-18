CREATE TABLE catalog_acquisitions (
    id TEXT PRIMARY KEY CHECK (length(id) > 0),
    source TEXT NOT NULL CHECK (length(trim(source)) > 0),
    acquired_at_utc TEXT NOT NULL CHECK (
        acquired_at_utc GLOB (
            '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]' ||
            'T[0-9][0-9]:[0-9][0-9]:[0-9][0-9].' ||
            '[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'
        )
    ),
    source_ref TEXT
) STRICT;

CREATE TABLE space_objects (
    norad_id INTEGER PRIMARY KEY CHECK (norad_id BETWEEN 1 AND 4294967295),
    name TEXT,
    object_type TEXT,
    catalog_status TEXT,
    metadata_acquisition_id TEXT REFERENCES catalog_acquisitions(id),
    rcs_m2 REAL CHECK (rcs_m2 BETWEEN 0 AND 1.7976931348623157e308),
    rcs_acquisition_id TEXT REFERENCES catalog_acquisitions(id),
    CHECK (metadata_acquisition_id IS NOT NULL OR
           (name IS NULL AND object_type IS NULL AND catalog_status IS NULL)),
    CHECK (rcs_m2 IS NULL OR rcs_acquisition_id IS NOT NULL)
) STRICT;

CREATE TABLE orbital_elements (
    id TEXT PRIMARY KEY CHECK (length(id) > 0),
    norad_id INTEGER NOT NULL REFERENCES space_objects(norad_id),
    epoch_utc TEXT NOT NULL CHECK (
        epoch_utc GLOB (
            '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]' ||
            'T[0-9][0-9]:[0-9][0-9]:[0-9][0-9].' ||
            '[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'
        )
    ),
    source_epoch TEXT NOT NULL CHECK (length(source_epoch) > 0),
    format TEXT NOT NULL CHECK (format IN ('omm', 'tle')),
    raw_record BLOB NOT NULL CHECK (length(raw_record) > 0),
    checksum TEXT NOT NULL CHECK (length(checksum) = 64 AND checksum NOT GLOB '*[^0-9a-f]*'),
    acquisition_id TEXT NOT NULL REFERENCES catalog_acquisitions(id),
    UNIQUE (norad_id, id),
    UNIQUE (norad_id, format, checksum)
) STRICT;

CREATE TABLE catalog_snapshots (
    id TEXT PRIMARY KEY CHECK (length(id) > 0),
    target_month TEXT NOT NULL UNIQUE CHECK (
        target_month GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]' AND
        substr(target_month, 6, 2) BETWEEN '01' AND '12'
    )
) STRICT;

CREATE TABLE snapshot_members (
    snapshot_id TEXT NOT NULL REFERENCES catalog_snapshots(id) ON DELETE CASCADE,
    norad_id INTEGER NOT NULL,
    element_id TEXT NOT NULL,
    PRIMARY KEY (snapshot_id, norad_id),
    FOREIGN KEY (norad_id, element_id) REFERENCES orbital_elements(norad_id, id)
) STRICT, WITHOUT ROWID;
