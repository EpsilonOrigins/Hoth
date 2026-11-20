"use strict";

const element = (id) => document.getElementById(id);
const replay = JSON.parse(element("mission-data").textContent);
const timeline = replay.timeline;
const live = timeline?.live;
const radars = replay.radars || [];
const canvas = element("globe");
const pen = canvas.getContext("2d");
const terrain = element("earth-texture");
const slider = element("timeline");
const satellite = element("satellite");
const playButton = element("play");
const orbitLayer = element("orbit-layer");
const groundLayer = element("ground-layer");
const farLayer = element("far-layer");
const shownCount = element("shown-count");
const radarLayer = element("radar-layer");
const referenceLayer = element("reference-layer");
const metersPerRadius = 6378137;
const eccentricitySquared = 0.0066943799901413165;
const rad = Math.PI / 180;
const tau = 2 * Math.PI;
const last = timeline?.last ?? replay.times.length - 1;

let frame = 0;
let trackIndex = 0;
let longitude = -40 * rad;
let latitude = 22 * rad;
let zoom = 1;
let playing = false;
let startedAt;
let startedOffset;
let pointer;
let paintId = 0;
let playbackId = 0;
let planetPixels;
let cameraImageRevision = "";
let path = [];
let reference = new Map();
let referencePath = [];
let referenceName = "";
let referenceError = "";
let currentUtc = null;
let receivedStates = [];
let wantedFrame = 0;
let fetchingFrame = false;
let oriented = false;

const disk = document.createElement("canvas");
disk.width = disk.height = 620;
const diskPen = disk.getContext("2d");
const stars = Array.from({ length: 96 }, (_, i) => [
  ((i * 24917 + 4163) % 49157) / 49157,
  ((i * 8631 + 1585) % 37781) / 37781,
  i % 9 === 0 ? 1.2 : .62,
]);

const activeTrack = () => replay.tracks[trackIndex];
const stateFor = (track, i) => timeline ? receivedStates[i] : track.states[frame];
const currentState = () => stateFor(activeTrack(), trackIndex);
const clamp = (value, min, max) => Math.max(min, Math.min(max, value));
const norm = ([x, y, z]) => Math.hypot(x, y, z);
const radiusScale = 1 / metersPerRadius;
const offsetAt = (index) => timeline
  ? Math.min(index * timeline.step_ms, timeline.duration_ms) / 1000
  : replay.offsets[index];
let extent = populationExtent();

function populationExtent() {
  let maxRadius = metersPerRadius;
  for (const track of replay.tracks) {
    for (const state of track.states) {
      if (state) maxRadius = Math.max(maxRadius, norm(state));
    }
  }
  return Math.max(1.15, maxRadius / metersPerRadius * 1.06);
}

function basis() {
  const s = Math.sin(longitude);
  const c = Math.cos(longitude);
  const t = Math.sin(latitude);
  const u = Math.cos(latitude);
  return {
    right: [-s, c, 0],
    up: [-t * c, -t * s, u],
    forward: [u * c, u * s, t],
  };
}

function project([x, y, z], { right, up, forward }, scale = 1) {
  return [
    (x * right[0] + y * right[1]) * scale,
    (x * up[0] + y * up[1] + z * up[2]) * scale,
    (x * forward[0] + y * forward[1] + z * forward[2]) * scale,
  ];
}

function updateTexture() {
  if (!terrain.naturalWidth) return;
  const image = document.createElement("canvas");
  image.width = terrain.naturalWidth;
  image.height = terrain.naturalHeight;
  const context = image.getContext("2d");
  context.drawImage(terrain, 0, 0);
  planetPixels = context.getImageData(0, 0, image.width, image.height);
  requestPaint();
}

function paintDisk({ right, up, forward }) {
  const revision = `${latitude},${longitude}`;
  if (revision === cameraImageRevision || !planetPixels) return;
  cameraImageRevision = revision;

  const size = disk.width;
  const half = size / 2;
  const imageData = diskPen.createImageData(size, size);
  const { data, width, height } = planetPixels;

  // Map each visible orthographic ray to a longitude and latitude in the
  // equirectangular image. Draw the orbit separately from canonical ECEF.
  for (let row = 0; row < size; row++) {
    const y = (half - row - .5) / half;
    for (let column = 0; column < size; column++) {
      const x = (column - half + .5) / half;
      const squared = x * x + y * y;
      if (squared >= 1) continue;
      const z = Math.sqrt(1 - squared);
      const ecefX = x * right[0] + y * up[0] + z * forward[0];
      const ecefY = x * right[1] + y * up[1] + z * forward[1];
      const ecefZ = y * up[2] + z * forward[2];
      const u = Math.floor((Math.atan2(ecefY, ecefX) / tau + .5) * width) % width;
      const v = clamp(Math.floor((.5 - Math.asin(clamp(ecefZ, -1, 1)) / Math.PI) * height), 0, height - 1);
      const textureIndex = (v * width + u) * 4;
      const index = (row * size + column) * 4;
      const light = .53 + .49 * Math.max(0, -.38 * x + .39 * y + .84 * z);
      const rim = (1 - z) ** 4;
      imageData.data[index] = data[textureIndex] * light + 5 + 10 * rim;
      imageData.data[index + 1] = data[textureIndex + 1] * light + 9 + 30 * rim;
      imageData.data[index + 2] = data[textureIndex + 2] * light + 13 + 57 * rim;
      imageData.data[index + 3] = clamp((1 - Math.sqrt(squared)) * half * 255, 0, 255);
    }
  }
  diskPen.putImageData(imageData, 0, 0);
}

function visible([x, y, z]) {
  return z >= 0 || x * x + y * y > 1;
}

function projectedLine(points, radius, cx, cy, shown, onSurface) {
  pen.beginPath();
  let end = null;
  for (let i = 1; i < points.length; i++) {
    let a = points[i - 1];
    let b = points[i];
    if (!a || !b) {
      end = null;
      continue;
    }

    const onSide = (p) => onSurface ? p[2] >= 0 : visible(p);
    const aShown = onSide(a);
    const bShown = onSide(b);
    if (aShown !== bShown) {
      let from = a;
      let to = b;
      for (let j = 0; j < 8; j++) {
        const mid = from.map((value, axis) => (value + to[axis]) / 2);
        if (onSide(mid) === aShown) from = mid;
        else to = mid;
      }
      if (aShown === shown) b = from;
      else a = to;
    } else if (aShown !== shown) {
      end = null;
      continue;
    }
    if (end !== a) pen.moveTo(cx + a[0] * radius, cy - a[1] * radius);
    pen.lineTo(cx + b[0] * radius, cy - b[1] * radius);
    end = b;
  }
  pen.stroke();
}

function graticule(camera, radius, cx, cy) {
  pen.lineWidth = .65;
  pen.strokeStyle = "rgba(190, 221, 247, .18)";
  for (let lat = -60; lat <= 60; lat += 30) {
    const points = [];
    for (let lon = -180; lon <= 180; lon += 4) {
      points.push(project([
        Math.cos(lat * rad) * Math.cos(lon * rad),
        Math.cos(lat * rad) * Math.sin(lon * rad),
        Math.sin(lat * rad),
      ], camera));
    }
    projectedLine(points, radius, cx, cy, true, true);
  }
  for (let lon = -180; lon < 180; lon += 30) {
    const points = [];
    for (let lat = -90; lat <= 90; lat += 4) {
      points.push(project([
        Math.cos(lat * rad) * Math.cos(lon * rad),
        Math.cos(lat * rad) * Math.sin(lon * rad),
        Math.sin(lat * rad),
      ], camera));
    }
    projectedLine(points, radius, cx, cy, true, true);
  }
}

function draw() {
  paintId = 0;
  const { width, height } = canvas.getBoundingClientRect();
  const density = Math.min(window.devicePixelRatio || 1, 2);
  const backingWidth = Math.round(width * density);
  const backingHeight = Math.round(height * density);
  if (canvas.width !== backingWidth || canvas.height !== backingHeight) {
    canvas.width = backingWidth;
    canvas.height = backingHeight;
  }
  pen.setTransform(density, 0, 0, density, 0, 0);
  pen.clearRect(0, 0, width, height);

  const cx = width / 2;
  const cy = height / 2;
  const radius = Math.min(width, height) * .45 / extent * zoom;
  const camera = basis();

  pen.fillStyle = "rgba(168, 194, 218, .33)";
  for (const [x, y, size] of stars) {
    pen.fillRect(x * width, y * height, size, size);
  }
  pen.strokeStyle = "rgba(119, 160, 191, .14)";
  pen.lineWidth = 1;
  for (const circle of [1.09, 1.28]) {
    pen.beginPath();
    pen.arc(cx, cy, radius * circle, 0, tau);
    pen.stroke();
  }

  const glow = pen.createRadialGradient(cx, cy, radius * .97, cx, cy, radius * 1.15);
  glow.addColorStop(0, "rgba(75, 172, 245, .30)");
  glow.addColorStop(1, "rgba(75, 172, 245, 0)");
  pen.fillStyle = glow;
  pen.beginPath();
  pen.arc(cx, cy, radius * 1.15, 0, tau);
  pen.fill();
  pen.fillStyle = "#163247";
  pen.beginPath();
  pen.arc(cx, cy, Math.max(0, radius - .3), 0, tau);
  pen.fill();

  paintDisk(camera);
  if (planetPixels) pen.drawImage(disk, cx - radius, cy - radius, radius * 2, radius * 2);
  graticule(camera, radius, cx, cy);
  pen.strokeStyle = "rgba(129, 211, 251, .60)";
  pen.lineWidth = 1;
  pen.beginPath();
  pen.arc(cx, cy, radius, 0, tau);
  pen.stroke();

  if (radarLayer.checked) {
    for (const radar of radars) {
      pen.lineWidth = 1.35;
      pen.strokeStyle = "rgba(255, 206, 125, .75)";
      for (const line of radar.lines) {
        projectedLine(line.map((point) => project(point, camera, radiusScale)), radius, cx, cy, true, false);
      }
      const site = project(radar.origin, camera, radiusScale);
      if (visible(site)) {
        const x = cx + site[0] * radius;
        const y = cy - site[1] * radius;
        pen.fillStyle = "#ffce7d";
        pen.fillRect(x - 2, y - 2, 4, 4);
        pen.font = "600 11px ui-monospace, monospace";
        pen.fillText(radar.id, x + 6, y - 5);
      }
    }
  }

  if (orbitLayer.checked) {
    const orbit = path.map((point) => point && project(point, camera));
    pen.lineWidth = 1.4;
    pen.strokeStyle = "rgba(110, 239, 208, .46)";
    pen.setLineDash([4, 5]);
    projectedLine(orbit, radius, cx, cy, false, false);
    pen.setLineDash([]);
    pen.lineWidth = 2.2;
    pen.strokeStyle = "#6eefd0";
    projectedLine(orbit, radius, cx, cy, true, false);
  }
  if (groundLayer.checked) {
    const ground = path.map((point) => point && project(point.map((value) => value / norm(point)), camera));
    pen.lineWidth = 1.75;
    pen.strokeStyle = "#bdabff";
    pen.setLineDash([4, 3]);
    projectedLine(ground, radius, cx, cy, true, true);
    pen.setLineDash([]);
  }

  if (referenceLayer.checked && referencePath.length) {
    const refOrbit = referencePath.map((point) => project(point, camera, radiusScale));
    pen.lineWidth = 1.7;
    pen.setLineDash([5, 4]);
    pen.strokeStyle = "#ff9a83";
    projectedLine(refOrbit, radius, cx, cy, true, false);
    if (farLayer.checked) {
      pen.strokeStyle = "rgba(255, 154, 131, .5)";
      projectedLine(refOrbit, radius, cx, cy, false, false);
    }
    pen.setLineDash([]);
    const target = reference.get(activeTrack().norad)?.get(utcKey(currentUtc));
    if (target) {
      const point = project(target, camera, radiusScale);
      const x = cx + point[0] * radius;
      const y = cy - point[1] * radius;
      if (visible(point) || farLayer.checked) {
        pen.strokeStyle = "#ff9a83";
        pen.strokeRect(x - 8, y - 8, 16, 16);
      }
    }
  }

  const frontMarkers = new Path2D();
  const farMarkers = new Path2D();
  const markerSize = replay.tracks.length > 1000 ? 1.6 : 2.7;
  let plotted = 0;
  let selected;
  for (const [i, track] of replay.tracks.entries()) {
    const state = stateFor(track, i);
    if (!state) continue;
    const point = project(state, camera, radiusScale);
    const foreground = visible(point);
    if (!foreground && !farLayer.checked) continue;
    const x = cx + point[0] * radius;
    const y = cy - point[1] * radius;
    if (x < 0 || x > width || y < 0 || y > height) continue;
    plotted++;
    if (i === trackIndex) {
      selected = { x, y, foreground };
      continue;
    }
    const markers = foreground ? frontMarkers : farMarkers;
    markers.moveTo(x + markerSize, y);
    markers.arc(x, y, markerSize, 0, tau);
  }
  pen.lineWidth = 1;
  pen.strokeStyle = "rgba(189, 214, 238, .60)";
  pen.stroke(farMarkers);
  pen.fillStyle = "rgba(189, 214, 238, .8)";
  pen.fill(frontMarkers);
  shownCount.textContent = `${plotted} / ${replay.tracks.length} plotted`;

  if (!selected) return;
  const { x, y, foreground } = selected;
  pen.fillStyle = "rgba(110, 239, 208, .20)";
  pen.beginPath();
  pen.arc(x, y, 12, 0, tau);
  pen.fill();
  pen.fillStyle = "#6eefd0";
  pen.strokeStyle = foreground ? "#eaffe9" : "#6eefd0";
  pen.lineWidth = 1.4;
  pen.beginPath();
  pen.arc(x, y, 5.6, 0, tau);
  if (foreground) pen.fill();
  pen.stroke();

  const tag = `NORAD ${activeTrack().norad}${foreground ? "" : " · behind Earth"}`;
  pen.font = "600 11px ui-monospace, monospace";
  const boxWidth = pen.measureText(tag).width + 16;
  const labelX = clamp(x + 14, 7, width - boxWidth - 7);
  const labelY = clamp(y - 23, 7, height - 31);
  pen.fillStyle = "#102b38";
  pen.strokeStyle = "#3b8d85";
  pen.lineWidth = 1;
  pen.beginPath();
  pen.roundRect(labelX, labelY, boxWidth, 23, 5);
  pen.fill();
  pen.stroke();
  pen.fillStyle = "#c4fff0";
  pen.fillText(tag, labelX + 8, labelY + 15);
}

function requestPaint() {
  if (!paintId) paintId = requestAnimationFrame(draw);
}

function loadTrack() {
  path = [];
  const norad = activeTrack().norad;
  const stride = Math.max(1, Math.ceil(activeTrack().states.length / 2400));
  for (const [i, state] of activeTrack().states.entries()) {
    if (!state) {
      if (path.at(-1)) path.push(null);
      continue;
    }
    const point = state.slice(0, 3).map((value) => value / metersPerRadius);
    if (i % stride === 0 || i === last || path.at(-1) === null) path.push(point);
  }
  if (timeline) {
    getJson(`/api/track/${norad}`).then((points) => {
      if (norad !== activeTrack().norad) return;
      path = points.map((point) => point?.map((axis) => axis * radiusScale) || null);
      if (!live) extent = Math.max(extent, ...path.filter(Boolean).map((point) => norm(point) * 1.06));
      requestPaint();
    }).catch((error) => {
      if (norad === activeTrack().norad) sourceStatus(`Orbit unavailable. ${error.message}`);
    });
  }
  loadReferenceTrack();

  const provenance = activeTrack().provenance;
  element("initialization").textContent = provenance
    ? (provenance.initialization.kind === "synthetic" ? "Synthetic epoch" : "Source epoch")
    : "No valid samples";
  if (provenance?.initialization.epoch_utc) {
    element("initialization").title = provenance.initialization.epoch_utc;
  } else {
    element("initialization").removeAttribute("title");
  }
  element("source-epoch").textContent = provenance
    ? provenance.source_epoch_utc.replace("T", " ")
    : "—";
}

function loadReferenceTrack() {
  const points = reference.get(activeTrack().norad);
  if (points) {
    const start = utcKey(timeline?.start ?? replay.times[0]);
    const end = utcKey(timeline?.end ?? replay.times[last]);
    const sorted = Array.from(points.keys()).filter((utc) => utc >= start && utc <= end).sort();
    const stride = Math.max(1, Math.ceil(sorted.length / 2400));
    referencePath = sorted.filter((_, i) => i % stride === 0 || i === sorted.length - 1)
      .map((time) => points.get(time));
    for (const point of referencePath) extent = Math.max(extent, norm(point) * radiusScale * 1.06);
  } else {
    referencePath = [];
  }
  element("reference-control").hidden = !referencePath.length;
  updateReference();
  requestPaint();
}

function updateReference() {
  const expected = currentUtc && reference.get(activeTrack().norad)?.get(utcKey(currentUtc));
  const state = currentState();
  const status = element("reference-status");
  const matched = Boolean(expected && state);
  element("reference-errors").hidden = !matched;
  status.hidden = !referenceName && !referenceError;
  const matchStatus = matched
    ? "Exact NORAD and UTC match."
    : "No matching states at this NORAD and UTC.";
  status.textContent = referenceError || (referenceName ? `${referenceName}. ${matchStatus}` : "");
  if (matched) {
    const delta = state.map((value, i) => value - expected[i]);
    element("position-error").textContent = `${Math.hypot(...delta.slice(0, 3)).toPrecision(4)} m`;
    element("velocity-error").textContent = `${Math.hypot(...delta.slice(3, 6)).toPrecision(4)} m/s`;
  }
}

function geodetic([x, y, z]) {
  const p = Math.hypot(x, y);
  let lat = Math.atan2(z, p * (1 - eccentricitySquared));
  for (let i = 0; i < 6; i++) {
    const s = Math.sin(lat);
    const primeVertical = metersPerRadius / Math.sqrt(1 - eccentricitySquared * s * s);
    lat = Math.atan2(z + eccentricitySquared * primeVertical * s, p);
  }
  return {
    lat: lat / rad,
    lon: Math.atan2(y, x) / rad,
    alt: p * Math.cos(lat) + z * Math.sin(lat)
      - metersPerRadius * Math.sqrt(1 - eccentricitySquared * Math.sin(lat) ** 2),
  };
}

const km = (value, digits) => (value / 1000).toFixed(digits);
const deg = (value, positive, negative) => `${Math.abs(value).toFixed(2)}° ${value < 0 ? negative : positive}`;

function renderFrame(index, utc, states) {
  frame = index;
  currentUtc = utc;
  if (states) {
    receivedStates = states;
    for (const state of states) {
      if (state) extent = Math.max(extent, norm(state) * radiusScale * 1.06);
    }
  }
  slider.value = frame;
  slider.setAttribute("aria-valuetext", utc);
  element("sample-utc").dateTime = utc;
  element("sample-utc").textContent = utc.replace("T", " ");
  element("sample-count").textContent = `${frame + 1} / ${last + 1} samples`;
  element("previous").disabled = frame === 0;
  element("next").disabled = frame === last;

  const state = currentState();
  const warning = element("sample-warning");
  warning.hidden = Boolean(state);
  warning.textContent = state ? "" : live ? "No telemetry for this satellite in the latest batch." : "Propagation failed at this sample.";
  if (state) {
    const { alt, lat, lon } = geodetic(state);
    element("altitude").textContent = `${km(alt, 1)} km`;
    element("latitude").textContent = deg(lat, "N", "S");
    element("longitude").textContent = deg(lon, "E", "W");
  } else {
    for (const field of ["altitude", "latitude", "longitude"]) element(field).textContent = "—";
  }
  for (const [i, axis] of ["x", "y", "z"].entries()) {
    element(`position-${axis}`).textContent = state ? km(state[i], 3) : "—";
    element(`velocity-${axis}`).textContent = state ? km(state[i + 3], 5) : "—";
  }
  updateReference();
  if (!oriented && state) {
    oriented = true;
    frameOrbit();
  }
  requestPaint();
}

function sourceStatus(message) {
  const status = element("source-status");
  status.hidden = !message;
  if (status.textContent !== message) status.textContent = message;
}

async function getJson(path) {
  const response = await fetch(path, { signal: AbortSignal.timeout(10000) });
  if (!response.ok) throw new Error(`Server returned ${response.status}.`);
  return response.json();
}

function setFrame(index) {
  const next = clamp(index, 0, last);
  if (!timeline) {
    renderFrame(next, replay.times[next]);
    return;
  }
  wantedFrame = next;
  if (!fetchingFrame) void loadFrame();
}

async function loadFrame() {
  fetchingFrame = true;
  const index = wantedFrame;
  try {
    const data = await getJson(`/api/frame/${index}`);
    if (index === wantedFrame) {
      renderFrame(index, data.utc, data.states);
      sourceStatus("");
    }
  } catch (error) {
    if (index === wantedFrame) {
      setPlaying(false);
      sourceStatus(`Replay frame unavailable. ${error.message}`);
    }
  } finally {
    fetchingFrame = false;
    if (index !== wantedFrame) void loadFrame();
  }
}

async function pollLive() {
  try {
    const { age_ms: age, frame: latest } = await getJson("/api/live");
    if (latest) renderFrame(0, latest.utc, latest.states);
    sourceStatus(!latest ? "Waiting for telemetry…" : age > 1000 ? "Waiting for a new telemetry batch." : "Receiving Zenoh telemetry");
  } catch (error) {
    sourceStatus(`Live feed unavailable. ${error.message}`);
  } finally {
    setTimeout(pollLive, 200);
  }
}

function lookAt([x, y, z]) {
  longitude = Math.atan2(y, x);
  latitude = clamp(Math.atan2(z, Math.hypot(x, y)), -1.54, 1.54);
  zoom = 1;
  requestPaint();
}

function centerCamera() {
  const state = currentState() || activeTrack().states.find(Boolean);
  if (state) lookAt(state);
}

function frameOrbit() {
  const state = currentState() || activeTrack().states.find(Boolean);
  if (!state) return;
  const [x, y, z, vx, vy, vz] = state;
  const normal = [y * vz - z * vy, z * vx - x * vz, x * vy - y * vx];
  const magnitude = norm(normal);

  // Tilt the initial camera out of the local orbital plane so the path
  // appears as a curve and the current satellite stays in the foreground.
  if (magnitude > 0) {
    const radius = norm(state);
    lookAt(state.slice(0, 3).map((value, i) => value / radius - .69 * normal[i] / magnitude));
  } else {
    lookAt(state);
  }
}

function moveCamera(dx, dy) {
  longitude = ((longitude + dx) % tau + tau) % tau;
  latitude = clamp(latitude + dy, -1.54, 1.54);
  requestPaint();
}

function changeZoom(factor) {
  zoom = clamp(zoom * factor, .42, Math.max(2.8, extent * 2));
  requestPaint();
}

function setPlaying(value) {
  if (live || playing === value) return;
  playing = value;
  playButton.ariaLabel = playing ? "Pause" : "Play";
  playButton.title = playButton.ariaLabel;
  if (playing) {
    const start = frame === last ? 0 : frame;
    if (frame === last) setFrame(start);
    startedAt = performance.now();
    startedOffset = offsetAt(start);
    playbackId = requestAnimationFrame(advance);
  } else {
    cancelAnimationFrame(playbackId);
  }
}

function advance(now) {
  if (!playing) return;
  const elapsed = Math.max(0, now - startedAt) / 1000;
  const offset = startedOffset + elapsed * Number(element("speed").value);
  if (!fetchingFrame) {
    let next = frame;
    if (timeline) {
      const millis = offset * 1000;
      next = millis >= timeline.duration_ms ? last : Math.floor(millis / timeline.step_ms);
    } else {
      while (next < last && offsetAt(next + 1) <= offset) next++;
    }
    if (next !== frame) setFrame(next);
  }
  if (frame === last && !fetchingFrame) setPlaying(false);
  else playbackId = requestAnimationFrame(advance);
}

playButton.addEventListener("click", () => setPlaying(!playing));
slider.addEventListener("input", () => {
  setPlaying(false);
  setFrame(Number(slider.value));
});
element("previous").addEventListener("click", () => { setPlaying(false); setFrame(frame - 1); });
element("next").addEventListener("click", () => { setPlaying(false); setFrame(frame + 1); });
element("speed").addEventListener("change", () => {
  startedAt = performance.now();
  startedOffset = offsetAt(frame);
});
satellite.addEventListener("change", () => {
  trackIndex = satellite.selectedIndex;
  loadTrack();
  if (currentUtc) renderFrame(frame, currentUtc);
});
element("center").addEventListener("click", centerCamera);
element("fit-all").addEventListener("click", () => {
  zoom = 1;
  requestPaint();
});
element("zoom-in").addEventListener("click", () => changeZoom(1.17));
element("zoom-out").addEventListener("click", () => changeZoom(1 / 1.17));
orbitLayer.addEventListener("change", requestPaint);
groundLayer.addEventListener("change", requestPaint);
farLayer.addEventListener("change", requestPaint);
radarLayer.addEventListener("change", requestPaint);
referenceLayer.addEventListener("change", requestPaint);
element("reference-file").addEventListener("change", async (event) => {
  const file = event.target.files[0];
  try {
    if (file?.size > 10 * 1024 * 1024) throw new Error("Use a reference CSV of 10 MiB or less.");
    const text = file ? await file.text() : null;
    if (file !== event.target.files[0]) return;
    reference = text === null ? new Map() : parseReference(text);
    referenceName = file?.name || "";
    referenceError = "";
    loadReferenceTrack();
  } catch (error) {
    reference = new Map();
    referenceName = "";
    referenceError = error.message;
    loadReferenceTrack();
  }
});

canvas.addEventListener("pointerdown", (event) => {
  if (pointer || event.button !== 0) return;
  pointer = { id: event.pointerId, x: event.clientX, y: event.clientY };
  canvas.setPointerCapture(event.pointerId);
});
canvas.addEventListener("pointermove", (event) => {
  if (pointer?.id !== event.pointerId) return;
  moveCamera((pointer.x - event.clientX) * .007, (event.clientY - pointer.y) * .007);
  pointer.x = event.clientX;
  pointer.y = event.clientY;
});
canvas.addEventListener("lostpointercapture", () => { pointer = null; });
canvas.addEventListener("wheel", (event) => {
  event.preventDefault();
  changeZoom(Math.exp(-event.deltaY * .0015));
}, { passive: false });
canvas.addEventListener("keydown", (event) => {
  switch (event.key) {
    case "ArrowLeft": moveCamera(-.15, 0); break;
    case "ArrowRight": moveCamera(.15, 0); break;
    case "ArrowUp": moveCamera(0, .15); break;
    case "ArrowDown": moveCamera(0, -.15); break;
    case "+": case "=": changeZoom(1.17); break;
    case "-": changeZoom(1 / 1.17); break;
    case " ": setPlaying(!playing); break;
    default: return;
  }
  event.preventDefault();
});

element("download").addEventListener("click", () => {
  const track = activeTrack();
  if (timeline) {
    window.location.assign(`/api/csv/${track.norad}`);
    return;
  }
  const rows = ["utc,norad_id,x_ecef_m,y_ecef_m,z_ecef_m,vx_ecef_m_s,vy_ecef_m_s,vz_ecef_m_s,status"];
  for (const [i, state] of track.states.entries()) {
    rows.push([replay.times[i], track.norad, ...(state || ["", "", "", "", "", ""]), state ? "ok" : "unavailable"].join(","));
  }
  const url = URL.createObjectURL(new Blob([rows.join("\n") + "\n"], { type: "text/csv" }));
  const link = document.createElement("a");
  link.href = url;
  link.download = `hoth-${track.norad}-ecef.csv`;
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
});

element("mission-name").textContent = replay.name;
element("snapshot").textContent = replay.snapshot;
element("digest").textContent = replay.digest.slice(0, 12);
element("digest").title = replay.digest;
element("coverage").textContent = `${replay.tracks.length} of ${replay.population} objects`;
element("first-time").textContent = (timeline?.start ?? replay.times[0]).split("T")[1];
element("last-time").textContent = (timeline?.end ?? replay.times[last]).split("T")[1];
element("radar-control").hidden = radars.length === 0;
slider.max = last;
for (const track of replay.tracks) {
  satellite.add(new Option(String(track.norad), String(track.norad)));
}
loadTrack();
if (live) {
  element("mode").textContent = "Live telemetry";
  element("mode-note").textContent = "Latest Zenoh batch";
  element("orbit-label").textContent = "Predicted orbit";
  element("sample-utc").textContent = "Waiting for telemetry";
  element("transport").hidden = true;
  element("speed").closest("label").hidden = true;
  element("download").hidden = true;
  element("provenance-heading").textContent = "Manifest provenance";
  slider.hidden = true;
  element("sample-count").closest("div").hidden = true;
  void pollLive();
} else {
  if (timeline) element("mode").textContent = "On-demand replay";
  setFrame(0);
}
if (terrain.complete) updateTexture();
else terrain.addEventListener("load", updateTexture, { once: true });
new ResizeObserver(requestPaint).observe(canvas);
