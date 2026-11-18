-- Synthetic source records with nanosecond-separated epochs and distinct provenance.
INSERT INTO catalog_acquisitions (id, source, acquired_at_utc, source_ref) VALUES
    ('elements', 'fixture-elements', '2026-11-18T00:00:00.000000000Z', 'synthetic-omm'),
    ('metadata', 'fixture-metadata', '2026-11-18T01:00:00.000000000Z', NULL);

INSERT INTO space_objects (norad_id, name, object_type, metadata_acquisition_id) VALUES
    (25544, 'Fixture Object', 'payload', 'metadata');
INSERT INTO space_objects (norad_id) VALUES (43013);

INSERT INTO orbital_elements
    (id, norad_id, epoch_utc, source_epoch, format, raw_record, checksum, acquisition_id)
VALUES
    ('late', 25544, '2026-11-16T23:59:59.098765432Z', '2026-11-16T23:59:59.098765432', 'omm',
     CAST('{"NORAD_CAT_ID":25544,"EPOCH":"2026-11-16T23:59:59.098765432","MEAN_MOTION":15.5,"ECCENTRICITY":0.0002,"INCLINATION":51.6,"RA_OF_ASC_NODE":120.0,"ARG_OF_PERICENTER":30.0,"MEAN_ANOMALY":10.0,"BSTAR":0.00001}' AS BLOB),
     'def051ec4a3708351660201cc29d45d498b4f95b4c4b76b8400d0e387ea0a28c', 'elements'),
    ('early', 25544, '2026-11-16T23:59:59.012345678Z', '2026-11-16T23:59:59.012345678', 'omm',
     CAST('{"NORAD_CAT_ID":25544,"EPOCH":"2026-11-16T23:59:59.012345678","MEAN_MOTION":15.5,"ECCENTRICITY":0.0002,"INCLINATION":51.6,"RA_OF_ASC_NODE":120.0,"ARG_OF_PERICENTER":30.0,"MEAN_ANOMALY":10.0,"BSTAR":0.00001}' AS BLOB),
     '51bf1c5a0b1ac2124fa58082033f257c80956b888a9ae4740b737b8ff286e638', 'elements');

INSERT INTO catalog_snapshots (id, target_month) VALUES ('november', '2026-11');
INSERT INTO snapshot_members (snapshot_id, norad_id, element_id) VALUES ('november', 25544, 'late');
