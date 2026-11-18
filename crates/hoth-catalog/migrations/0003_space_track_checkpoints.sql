CREATE TABLE space_track_checkpoints (
    object_scope TEXT NOT NULL,
    epoch_month TEXT NOT NULL,
    last_gp_id INTEGER NOT NULL CHECK (last_gp_id >= 0),
    complete INTEGER NOT NULL CHECK (complete IN (0, 1)),
    seen_ids TEXT NOT NULL,
    PRIMARY KEY (object_scope, epoch_month)
) STRICT, WITHOUT ROWID;
