# Vallado SGP4 verification vectors

The sample inputs and TEME vectors come from David A. Vallado, Paul Crawford, Richard Hujsak, and T. S. Kelso, *Revisiting Spacetrack Report #3*, AIAA 2006-6753. The CelesTrak archive supplies the original TLEs in `sgp4/cpp/testsgp4/SGP4-VER.TLE` and the corresponding TEME ephemerides in `sgp4/cpp/testsgp4/TestSGP4/*.e`. [Paper and verification archive](https://celestrak.org/publications/AIAA/2006-6753/)

| NORAD ID | Published case | Samples |
| ---: | --- | ---: |
| 5 | Near-Earth TEME verification example | 13 |
| 4632 | Deep-space Lyddane case with negative offsets | 5 |
| 8195 | Eccentric 12-hour resonant Molniya orbit | 25 |
| 24208 | Inclined 24-hour resonant GEO | 13 |
| 28057 | Near-Earth orbit with small drag terms | 25 |

`verification.tle` keeps both original 69-character lines for the five cases. The 81 `source` rows in `vectors.csv` keep Vallado's printed TEME positions and velocities. Each row also has an absolute UTC calculated from the TLE epoch and published offset. ECEF values come from a separate ERFA IAU 1982 sidereal rotation and include the rotating-frame velocity term. The frame calculation uses UTC for UT1 and zero polar motion, matching the documented HOTH frame policy. [ERFA GMST82 implementation](https://github.com/liberfa/erfa/blob/v2.0.1/src/gmst82.c)

The five `synthetic` rows use the Molniya elements, the synthetic epoch `2026-11-17T00:00:00Z`, and zero B*. Their reference TEME predictions come from the independent Vallado C++ implementation in the Python `sgp4` package, initialized with WGS-72 and AFSPC mode. The original source epoch stays in the HOTH result. [Python SGP4 2.27](https://github.com/brandon-rhodes/python-sgp4/tree/2.27)

The test compares three-dimensional errors at each reference timestamp. Limits are 0.01 m for position and 0.0001 m/s for velocity in both frames. These limits measure agreement with reference implementations under the stated assumptions. Agreement with a physical orbit depends on element quality, the time since epoch, and the chosen Earth-orientation model.

## Run the offline test

```sh
cargo test --locked -p hoth-orbits --test reference -- --nocapture
```

Failures identify the NORAD ID, UTC, elapsed seconds, frame, and measured error. The test prints the maximum position and velocity errors for both frames.

## Regenerate the fixtures

Regeneration starts from the original archive. The generator checks its SHA-256 and uses Python packages only during regeneration. Workspace tests use the committed files.

```sh
python -m pip install pyerfa==2.0.1.5 numpy==2.5.5 sgp4==2.27
python crates/hoth-orbits/tests/fixtures/vallado/generate.py /path/to/AIAA-2006-6753.zip
```
