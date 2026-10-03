// openOMSI Voice - a GreenTeaSpeak 2 plugin (@greentea/plugin-sdk) for positional voice in
// openOMSI multiplayer, the way SaltyChat does it for FiveM.
//
// The game (crates/omsi-app/src/voice.rs) connects to 127.0.0.1:38088 and sends one JSON
// object a line:
//   initiate  {serverUid, channel, password, nickname, range}: go to the session's channel
//   self      {x, y, z, yaw}: the listener (yaw in degrees, SaltyChat's Rotation)
//   players   {players: [{nickname, x, y, z, range, volume}]}: everybody else who can be heard
//   reset     the session is over
// and is told back:
//   state     {connected, inChannel, error}
//   talk      {nickname, talking}
//   mute      {microphoneMuted, soundMuted}
//
// Plain JavaScript (ES module, no dependencies, no build step): the SDK's types are only
// needed to write a plugin, not to run one.

import net from "node:net";

const DEFAULT_PORT = 38088;
/** A nickname not found in the channel is looked for again after this long (ms). */
const MISS_RETRY_MS = 2000;
/** One line from the game is at most this long (a players message of 64 players fits). */
const MAX_LINE = 64 * 1024;

let ctx = null;
let server = null;
const games = new Set();
const unsubscribe = [];

/** The session the game asked for (its initiate), null when none. */
let session = null;
let inChannel = false;
let problem = "";
/** nickname -> {clientId, at}; clientId null for a nickname not found (looked for again later). */
const clients = new Map();
/** Client ids given a 3D pose, to clear when they leave. */
let posed = new Set();

function send(sock, msg) {
  if (!sock.destroyed) sock.write(JSON.stringify(msg) + "\n");
}

function broadcast(msg) {
  for (const g of games) send(g, msg);
}

function report() {
  broadcast({ type: "state", connected: true, inChannel, error: problem });
}

/** A channel as GreenTeaSpeak takes it: an id when it is a number, else its name. */
function channelRef(channel) {
  return /^\d+$/.test(String(channel)) ? Number(channel) : String(channel);
}

async function initiate() {
  if (!session) return;
  inChannel = false;
  problem = "";
  try {
    const state = await ctx.connection.getState();
    if (!state.connected) {
      problem = "GreenTeaSpeak is not connected to a voice server";
      return report();
    }
    if (session.serverUid) {
      const uid = await ctx.connection.getServerUid();
      if (uid !== session.serverUid) {
        problem = "GreenTeaSpeak is on another voice server than the session's";
        return report();
      }
    }
    try {
      await ctx.connection.setOwnNickname(session.nickname);
    } catch (e) {
      ctx.log.warn("could not take the nickname", session.nickname, e);
    }
    await ctx.connection.moveSelfToChannel(channelRef(session.channel), session.password || undefined);
    await ctx.voice.setSpatialEnabled(true);
    clients.clear();
    inChannel = true;
    ctx.log.info("in the session's channel", session.channel, "as", session.nickname);
  } catch (e) {
    problem = "could not join the voice channel: " + (e && e.message ? e.message : String(e));
    ctx.log.error(problem);
  }
  report();
}

async function reset() {
  session = null;
  inChannel = false;
  clients.clear();
  posed = new Set();
  try {
    await ctx.voice.resetSpatial();
    await ctx.voice.setSpatialEnabled(false);
  } catch (e) {
    ctx.log.warn("reset", e);
  }
}

async function clientIdOf(nickname) {
  const known = clients.get(nickname);
  const now = Date.now();
  if (known && (known.clientId !== null || now - known.at < MISS_RETRY_MS)) return known.clientId;
  let clientId = null;
  try {
    const c = await ctx.connection.findClientByName(nickname);
    clientId = c ? c.clientId : null;
  } catch (e) {
    ctx.log.warn("findClientByName", nickname, e);
  }
  clients.set(nickname, { clientId, at: now });
  return clientId;
}

async function placeSelf(m) {
  await ctx.voice.setListenerPose({ x: m.x, y: m.y, z: m.z, yawDeg: m.yaw });
}

async function placePlayers(m) {
  const now = new Set();
  for (const p of m.players || []) {
    const id = await clientIdOf(String(p.nickname));
    if (id === null || id === undefined) continue;
    now.add(id);
    await ctx.voice.setClientPose(id, {
      x: p.x,
      y: p.y,
      z: p.z,
      voiceRange: p.range,
      alive: true,
      volumeOverride: typeof p.volume === "number" ? p.volume : null,
    });
  }
  // (a player gone from the session or out of reach: heard as without the plugin no more)
  for (const id of posed) {
    if (!now.has(id)) {
      try {
        await ctx.voice.clearClientPose(id);
      } catch (e) {
        ctx.log.warn("clearClientPose", id, e);
      }
    }
  }
  posed = now;
}

// The game sends ten positions a second; GreenTeaSpeak's calls are asynchronous. One of each
// kind is worked on at a time and only the latest waiting one is kept.
const latest = { self: null, players: null };
const busy = { self: false, players: false };

function queue(kind, msg, work) {
  latest[kind] = msg;
  if (busy[kind]) return;
  busy[kind] = true;
  (async () => {
    while (latest[kind]) {
      const m = latest[kind];
      latest[kind] = null;
      if (!inChannel) continue;
      try {
        await work(m);
      } catch (e) {
        ctx.log.warn(kind, e);
      }
    }
    busy[kind] = false;
  })();
}

function onMessage(sock, m) {
  switch (m.type) {
    case "initiate": {
      const next = {
        serverUid: String(m.serverUid || ""),
        channel: String(m.channel || ""),
        password: String(m.password || ""),
        nickname: String(m.nickname || ""),
        range: Number(m.range) || 20,
      };
      const same = session && JSON.stringify(session) === JSON.stringify(next);
      session = next;
      if (same && inChannel) send(sock, { type: "state", connected: true, inChannel, error: problem });
      else initiate();
      break;
    }
    case "self":
      queue("self", m, placeSelf);
      break;
    case "players":
      queue("players", m, placePlayers);
      break;
    case "reset":
      reset();
      break;
    default:
      break;
  }
}

function onGame(sock) {
  games.add(sock);
  sock.setEncoding("utf8");
  sock.setNoDelay(true);
  ctx.log.info("openOMSI connected");
  let buf = "";
  sock.on("data", (chunk) => {
    buf += chunk;
    if (buf.length > MAX_LINE && buf.indexOf("\n") < 0) {
      buf = "";
      return;
    }
    let nl;
    while ((nl = buf.indexOf("\n")) >= 0) {
      const line = buf.slice(0, nl).trim();
      buf = buf.slice(nl + 1);
      if (!line) continue;
      try {
        onMessage(sock, JSON.parse(line));
      } catch (e) {
        ctx.log.warn("bad line from openOMSI", e);
      }
    }
  });
  const gone = () => {
    if (!games.delete(sock)) return;
    ctx.log.info("openOMSI disconnected");
    if (games.size === 0) reset();
  };
  sock.on("close", gone);
  sock.on("error", gone);
}

export default {
  async activate(context) {
    ctx = context;
    const port = Number(await ctx.storage.get("port")) || DEFAULT_PORT;
    unsubscribe.push(
      ctx.events.onTalkState((ev) => broadcast({ type: "talk", nickname: ev.name, talking: ev.talking })),
      ctx.events.onMuteState((ev) => broadcast({ type: "mute", microphoneMuted: ev.microphoneMuted, soundMuted: ev.soundMuted })),
      ctx.events.onConnectionChange((ev) => {
        clients.clear();
        if (ev.connected) initiate();
        else {
          inChannel = false;
          problem = "GreenTeaSpeak is not connected to a voice server";
          report();
        }
      }),
    );
    server = net.createServer(onGame);
    server.on("error", (e) => ctx.log.error("cannot listen on 127.0.0.1:" + port, e));
    // (this machine only: a game elsewhere has no business moving us about)
    server.listen(port, "127.0.0.1", () => ctx.log.info("waiting for openOMSI on 127.0.0.1:" + port));
  },

  async deactivate() {
    for (const u of unsubscribe.splice(0)) {
      try {
        u();
      } catch (_) {}
    }
    for (const g of games) g.destroy();
    games.clear();
    if (server) server.close();
    server = null;
    await reset();
  },
};
