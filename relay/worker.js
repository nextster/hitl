// The room where a Mac and its phone meet when they are not in one network.
//
// Each comes by a WebSocket to /room/<room>/mac or /room/<room>/phone, saying the word this server lets in
// by (its secret KEY). What one says, the other is handed, as it was said: the two seal their words with a
// key this server never sees, so it carries frames it can neither read nor make. Its own words are few and
// in the open, as text: to each side whether the other is in the room ({"mac":"here"}, {"phone":"gone"}),
// and of a Mac that left on purpose with nothing in its queue, {"mac":"resting"}. Asked plainly, with no
// WebSocket, it says who is in the room, and nobody's place there is taken by the asking.
//
// What the phone says while the Mac is not in the room is held for it, up to a day and within bounds, and handed
// to the Mac first thing when it comes in: his pick of 2026-10-16, «если есть связь с сервером прихранить на
// сервере, если нет то прихранить на клиенте». The phone is told on coming in that this server holds
// ({"mac":…,"holds":true}), and of each frame it held ({"held":1}) or could not ({"held":0}): what is not held
// stays on the phone. The frames stay sealed: held or carried, they are the same bytes.

// A day, as long as the Mac takes a held frame; and bounds a phone cannot pass: a few hundred presses, or some
// pictures.
const HOLD_MS = 24 * 3600 * 1000;
const HOLD_FRAMES = 500;
const HOLD_BYTES = 16 * 1024 * 1024;

export class Room {
  constructor(state) {
    this.state = state;
    // A "ping" now and then keeps a way open; it is answered without waking the room.
    state.setWebSocketAutoResponse(new WebSocketRequestResponsePair("ping", "pong"));
  }

  async fetch(request) {
    if (request.headers.get("Upgrade") !== "websocket") {
      return Response.json({ mac: await this.presence("mac"), phone: await this.presence("phone") });
    }
    const role = new URL(request.url).pathname.split("/")[3];
    const other = role === "mac" ? "phone" : "mac";
    const [client, server] = Object.values(new WebSocketPair());
    // One Mac and one phone: who comes again takes the place of the earlier one.
    for (const earlier of this.state.getWebSockets(role)) close(earlier, 4000, "another came in its place");
    this.state.acceptWebSocket(server, [role]);
    if (role === "mac") await this.state.storage.put("resting", false);
    tell(server, role === "phone" ? { mac: await this.presence("mac"), holds: true } : { phone: await this.presence("phone") });
    if (role === "mac") await this.handHeld(server);
    for (const peer of this.state.getWebSockets(other)) tell(peer, { [role]: "here" });
    return new Response(null, { status: 101, webSocket: client });
  }

  // "here", or how the side that is not in the room left it.
  async presence(role, except) {
    if (this.state.getWebSockets(role).some((socket) => socket !== except && socket.readyState === 1)) return "here";
    return role === "mac" && (await this.state.storage.get("resting")) ? "resting" : "gone";
  }

  async webSocketMessage(socket, message) {
    const [role] = this.state.getTags(socket);
    if (typeof message === "string") {
      // The Mac's last word before it leaves with nothing in its queue.
      if (role === "mac" && message === "resting") await this.state.storage.put("resting", true);
      return;
    }
    const peers = this.state.getWebSockets(role === "mac" ? "phone" : "mac").filter((peer) => peer.readyState === 1);
    if (role === "phone" && peers.length === 0) {
      tell(socket, { held: (await this.hold(message)) ? 1 : 0 });
      return;
    }
    for (const peer of peers) {
      try {
        peer.send(message);
      } catch {
        // a peer that is leaving: it hears of nothing more
      }
    }
  }

  // Keeps one frame of the phone's for the Mac; false when the bounds do not let it in.
  async hold(frame) {
    const size = frame.byteLength;
    const [count, bytes, next] = await Promise.all(["heldCount", "heldBytes", "heldNext"].map((key) => this.state.storage.get(key)));
    if ((count || 0) >= HOLD_FRAMES || (bytes || 0) + size > HOLD_BYTES) return false;
    const key = "held:" + String(next || 0).padStart(12, "0");
    await this.state.storage.put({
      [key]: { at: Date.now(), frame },
      heldCount: (count || 0) + 1,
      heldBytes: (bytes || 0) + size,
      heldNext: (next || 0) + 1,
    });
    return true;
  }

  // What was held for the Mac, in the order it was said, to the Mac that came in; then nothing of it is kept.
  async handHeld(mac) {
    const held = await this.state.storage.list({ prefix: "held:" });
    if (held.size === 0) return;
    for (const { at, frame } of held.values()) {
      if (Date.now() - at > HOLD_MS) continue;
      try {
        mac.send(frame);
      } catch {
        return; // the Mac is gone again: what was held waits for it
      }
    }
    const keys = [...held.keys()];
    for (let at = 0; at < keys.length; at += 128) await this.state.storage.delete(keys.slice(at, at + 128));
    await this.state.storage.put({ heldCount: 0, heldBytes: 0 });
  }

  async webSocketClose(socket, code) {
    close(socket, code === 1005 || code === 1006 ? 1000 : code, "");
    await this.left(socket);
  }

  async webSocketError(socket) {
    await this.left(socket);
  }

  async left(socket) {
    const [role] = this.state.getTags(socket);
    const now = await this.presence(role, socket);
    if (now === "here") return;
    for (const peer of this.state.getWebSockets(role === "mac" ? "phone" : "mac")) tell(peer, { [role]: now });
  }
}

function tell(socket, word) {
  try {
    socket.send(JSON.stringify(word));
  } catch {
    // it is gone already
  }
}

function close(socket, code, reason) {
  try {
    socket.close(code, reason);
  } catch {
    // closed already
  }
}

// Compared as digests, so that the time it takes says nothing of where two words differ.
async function same(one, other) {
  const [left, right] = await Promise.all(
    [one, other].map(async (text) => new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text)))),
  );
  let differ = 0;
  for (let at = 0; at < left.length; at += 1) differ |= left[at] ^ right[at];
  return differ === 0;
}

export default {
  async fetch(request, env) {
    const [, kind, room, role] = new URL(request.url).pathname.split("/");
    if (kind !== "room" || !/^[0-9a-f]{32}$/.test(room || "") || !["mac", "phone"].includes(role)) {
      return new Response("nothing here", { status: 404 });
    }
    if (!env.KEY || !(await same(request.headers.get("Authorization") || "", "Bearer " + env.KEY))) {
      return new Response("the word is not the one", { status: 401 });
    }
    return env.ROOMS.get(env.ROOMS.idFromName(room)).fetch(request);
  },
};
