"use strict";

// Normalize fractional precision without rounding UTC to JavaScript milliseconds.
function utcKey(value) {
  const parts = /^(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d)(?:\.(\d{1,9}))?Z$/.exec(value);
  if (!parts) return null;
  const base = new Date(parts[1] + "Z");
  if (!Number.isFinite(+base) || base.toISOString().slice(0, 19) !== parts[1]) return null;
  return `${parts[1]}.${(parts[2] || "").padEnd(9, "0")}Z`;
}

function parseReference(text) {
  const [header, ...rows] = text.trim().split(/\r?\n/);
  const columns = header.split(",").map((field) => field.trim());
  const names = columns.includes("norad_id")
    ? ["norad_id", "x_ecef_m", "y_ecef_m", "z_ecef_m", "vx_ecef_m_s", "vy_ecef_m_s", "vz_ecef_m_s"]
    : ["norad", "ecef_x_m", "ecef_y_m", "ecef_z_m", "ecef_vx_m_s", "ecef_vy_m_s", "ecef_vz_m_s"];
  const indices = ["utc", ...names].map((name) => columns.indexOf(name));
  if (indices.some((index) => index < 0)) {
    throw new Error("Use a HOTH state CSV or the Vallado vectors CSV with ECEF positions in meters and velocities in m/s.");
  }
  const status = columns.indexOf("status");
  const reference = new Map();
  for (const [index, row] of rows.entries()) {
    if (!row.trim()) continue;
    const fields = row.split(",").map((field) => field.trim());
    if (status >= 0 && fields[status] === "unavailable") continue;
    const [time, id, ...vectors] = indices.map((i) => fields[i]);
    const key = utcKey(time);
    const norad = Number(id);
    if (!key || !/^\d+$/.test(id) || norad < 1 || norad > 0xffffffff
        || !vectors.every((value) => value && Number.isFinite(Number(value)))) {
      throw new Error(`Invalid UTC, NORAD ID or ECEF vectors on CSV line ${index + 2}.`);
    }
    if (!reference.has(norad)) reference.set(norad, new Map());
    const track = reference.get(norad);
    if (track.has(key)) throw new Error(`Duplicate NORAD and UTC on CSV line ${index + 2}.`);
    track.set(key, vectors.map(Number));
  }
  if (!reference.size) throw new Error("The CSV contains no usable ECEF states.");
  return reference;
}
