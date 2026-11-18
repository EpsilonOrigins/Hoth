"""Extract Vallado's published verification states and compute independent ECEF vectors."""

import csv
import hashlib
import math
import sys
from datetime import datetime, timedelta, timezone
from decimal import Decimal
from pathlib import Path
from zipfile import ZipFile

import erfa
from sgp4.api import SGP4_ERRORS, Satrec, WGS72, jday

DIRECTORY = Path(__file__).parent
SOURCE_SHA256 = "3642043b706c76be87cf012db3f22e04da6b80498d00f515e51879e0ffadc115"
CASE_IDS = ("00005", "04632", "08195", "24208", "28057")
SYNTHETIC_ID = "08195"
SYNTHETIC_EPOCH = datetime(2026, 11, 17, tzinfo=timezone.utc)
EARTH_ROTATION_RAD_S = 7.29211514670698e-5


def read_tles(archive):
    lines = archive.read("sgp4/cpp/testsgp4/SGP4-VER.TLE").decode("ascii").splitlines()
    return {
        line[2:7]: (line[:69], lines[i + 1][:69])
        for i, line in enumerate(lines)
        if line.startswith("1 ") and line[2:7] in CASE_IDS
    }


def tle_epoch(line):
    year = int(line[18:20])
    year += 2000 if year < 57 else 1900
    day = Decimal(line[20:32])
    microseconds = int((day - 1) * 86_400_000_000)
    return datetime(year, 1, 1, tzinfo=timezone.utc) + timedelta(microseconds=microseconds)


def published_states(archive, norad):
    name = f"sgp4/cpp/testsgp4/TestSGP4/{norad}.e"
    ephemeris = archive.read(name).decode("ascii")
    header, series = ephemeris.split("EphemerisTimePosVel", 1)
    assert "CoordinateSystem TEME" in " ".join(header.split())
    assert "DistanceUnit Kilometers" in " ".join(header.split())
    for line in series.split("END Ephemeris", 1)[0].splitlines():
        if line.strip():
            seconds, *vector = line.split()
            offset = Decimal(seconds)
            assert offset == offset.to_integral_value() and len(vector) == 6
            yield int(offset), vector


def ecef(at, teme):
    ut1_day, ut1_fraction = erfa.dtf2d(
        "UTC", at.year, at.month, at.day, at.hour, at.minute,
        at.second + at.microsecond / 1e6,
    )
    gmst = erfa.gmst82(ut1_day, ut1_fraction)
    s, c = math.sin(gmst), math.cos(gmst)
    x, y, z, vx, vy, vz = [float(component) * 1000 for component in teme]
    px, py = c * x + s * y, -s * x + c * y
    return (
        px, py, z,
        c * vx + s * vy + EARTH_ROTATION_RAD_S * py,
        -s * vx + c * vy - EARTH_ROTATION_RAD_S * px,
        vz,
    )


def synthetic_states(tle):
    source = Satrec.twoline2rv(*tle, WGS72)
    day, fraction = jday(
        SYNTHETIC_EPOCH.year, SYNTHETIC_EPOCH.month, SYNTHETIC_EPOCH.day, 0, 0, 0
    )
    synthetic = Satrec()
    synthetic.sgp4init(
        WGS72, "a", source.satnum, (day - 2433281.5) + fraction,
        0, source.ndot, source.nddot, source.ecco, source.argpo,
        source.inclo, source.mo, source.no_kozai, source.nodeo,
    )
    for offset in (-7200, 0, 10800, 43200, 86400):
        error, position, velocity = synthetic.sgp4_tsince(offset / 60)
        if error:
            raise ValueError(SGP4_ERRORS[error])
        yield offset, [format(value, ".15g") for value in (*position, *velocity)]


def rows(kind, norad, epoch, samples):
    for offset, teme in samples:
        at = epoch + timedelta(seconds=offset)
        utc = at.isoformat().replace("+00:00", "Z")
        fixed = [format(value, ".15g") for value in ecef(at, teme)]
        yield [kind, norad, offset, utc, *teme, *fixed]


def main():
    source = Path(sys.argv[1])
    if hashlib.sha256(source.read_bytes()).hexdigest() != SOURCE_SHA256:
        raise ValueError("Unexpected CelesTrak archive SHA-256")

    with ZipFile(source) as archive:
        tles = read_tles(archive)
        (DIRECTORY / "verification.tle").write_text(
            "".join("\n".join(tles[norad]) + "\n" for norad in CASE_IDS), encoding="ascii"
        )
        with (DIRECTORY / "vectors.csv").open("w", newline="", encoding="ascii") as out:
            writer = csv.writer(out, lineterminator="\n")
            writer.writerow([
                "initialization", "norad", "offset_seconds", "utc",
                "teme_x_km", "teme_y_km", "teme_z_km",
                "teme_vx_km_s", "teme_vy_km_s", "teme_vz_km_s",
                "ecef_x_m", "ecef_y_m", "ecef_z_m",
                "ecef_vx_m_s", "ecef_vy_m_s", "ecef_vz_m_s",
            ])
            for norad in CASE_IDS:
                writer.writerows(
                    rows("source", norad, tle_epoch(tles[norad][0]), published_states(archive, norad))
                )
            writer.writerows(rows(
                "synthetic", SYNTHETIC_ID, SYNTHETIC_EPOCH, synthetic_states(tles[SYNTHETIC_ID])
            ))


if __name__ == "__main__":
    main()
