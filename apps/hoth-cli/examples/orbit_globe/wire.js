"use strict";

if (replay.wire_monitor) {
  const panel = element("wire-monitor");
  const freeze = element("wire-freeze");
  let frozen = false;
  let pending = false;

  function showPacket(packet) {
    element("wire-packet").hidden = !packet;
    freeze.disabled = !packet;
    if (!packet) return;
    const { pdu } = packet;
    const { site, application, entity } = pdu.entity_id;
    element("wire-norad").textContent = packet.norad_id;
    element("wire-entity").textContent = `${site} : ${application} : ${entity}`;
    element("wire-exercise").textContent = pdu.exercise_id;
    element("wire-source").textContent = packet.source;
    element("wire-received").textContent = packet.received_at_utc;
    element("wire-decoded").textContent = JSON.stringify(pdu, null, 2);
    element("wire-hex").textContent = packet.hex;
  }

  function wireStatus(message) {
    const status = element("wire-status");
    if (status.textContent !== message) status.textContent = message;
  }

  async function pollWire() {
    if (pending) return;
    pending = true;
    const selected = activeTrack().norad;
    try {
      const { counters, packet } = await getJson(`/api/dis/${selected}`);
      element("wire-accepted").textContent = counters.accepted.toLocaleString();
      element("wire-entities").textContent = counters.entities_seen.toLocaleString();
      element("wire-ignored").textContent = counters.ignored.toLocaleString();
      if (frozen) {
        wireStatus("Packet frozen for inspection");
      } else if (selected === activeTrack().norad) {
        showPacket(packet);
        wireStatus(packet
          ? (packet.age_ms > 2000 ? "Showing the last received packet" : "Receiving DIS datagrams")
          : `Waiting for a DIS packet for NORAD ${selected}`);
      }
    } catch (error) {
      wireStatus(`DIS monitor unavailable. ${error.message}`);
    } finally {
      pending = false;
    }
  }

  freeze.addEventListener("click", () => {
    frozen = !frozen;
    freeze.setAttribute("aria-pressed", frozen);
    freeze.textContent = frozen ? "Resume packet view" : "Freeze packet";
    void pollWire();
  });
  satellite.addEventListener("change", () => {
    if (!frozen) showPacket(null);
    void pollWire();
  });
  panel.hidden = false;
  element("wire-nav").hidden = false;
  void pollWire();
  setInterval(pollWire, 500);
}
