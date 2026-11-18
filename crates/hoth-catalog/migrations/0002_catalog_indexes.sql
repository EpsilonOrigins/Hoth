CREATE INDEX orbital_elements_norad_epoch ON orbital_elements (norad_id, epoch_utc);
CREATE INDEX orbital_elements_epoch ON orbital_elements (epoch_utc, norad_id);
CREATE INDEX snapshot_members_element ON snapshot_members (norad_id, element_id);
