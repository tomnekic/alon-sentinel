#!/usr/bin/env node
// Parallel Kuma seeder — creates all monitors as inactive to avoid event-loop
// saturation during seeding, then resumes all at once when done.
//
// Env vars:
//   KUMA_URL       — default http://localhost:3101
//   KUMA_USER      — default bench
//   KUMA_PASS      — default Bench1234!
//   MONITOR_COUNT  — number of monitors to create (required)
//   SEED_PARALLEL  — concurrent socket connections, default 5
//   CHECK_INTERVAL — monitor interval in seconds, default 60
//   TARGET_URL     — http URL the monitors check, default http://target:8088/ok

"use strict";
const { io } = require("socket.io-client");

const KUMA_URL   = process.env.KUMA_URL       || "http://localhost:3101";
const USERNAME   = process.env.KUMA_USER      || "bench";
const PASSWORD   = process.env.KUMA_PASS      || "Bench1234!";
const COUNT      = parseInt(process.env.MONITOR_COUNT  || "0",   10);
const PARALLEL   = parseInt(process.env.SEED_PARALLEL  || "5",   10);
const INTERVAL   = parseInt(process.env.CHECK_INTERVAL || "60",  10);
const TARGET_URL = process.env.TARGET_URL     || "http://target:8088/ok";

if (!COUNT) { console.error("[kuma-seed] MONITOR_COUNT must be set"); process.exit(1); }

const log = msg => console.log(`[kuma-seed] ${msg}`);

function call(socket, event, ...args) {
    return new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error(`Timeout waiting for '${event}' response`)), 45000);
        socket.emit(event, ...args, (...resp) => { clearTimeout(timer); resolve(resp); });
    });
}

async function openSocket() {
    const socket = io(KUMA_URL, { transports: ["websocket"], reconnection: false, timeout: 15000 });
    await new Promise((resolve, reject) => {
        socket.on("connect", resolve);
        socket.on("connect_error", reject);
        setTimeout(() => reject(new Error("Socket connect timeout after 20s")), 20000);
    });
    await new Promise(r => setTimeout(r, 400)); // wait for server info burst
    return socket;
}

async function authenticate(socket) {
    // Kuma v1 accepted positional setup args; Kuma v2 accepts an object.
    // Both return an error/no-op when setup is already complete.
    try {
        await call(socket, "setup", {
            username: USERNAME,
            password: PASSWORD,
            confirmPassword: PASSWORD,
        });
    } catch (_) {
        try { await call(socket, "setup", USERNAME, PASSWORD); } catch (_) {}
    }
    const [res] = await call(socket, "login", { username: USERNAME, password: PASSWORD, token: "" });
    if (!res || !res.ok) throw new Error(`Login failed: ${JSON.stringify(res)}`);
}

// Returns the new monitor's id, or throws on failure.
async function addMonitor(socket, index) {
    const [res] = await call(socket, "add", {
        type:                  "http",
        name:                  `Bench-${index}`,
        url:                   TARGET_URL,
        interval:              INTERVAL,
        retryInterval:         INTERVAL,
        maxretries:            0,
        method:                "GET",
        active:                false,   // inactive during seeding — activate after
        notificationIDList:    {},
        accepted_statuscodes:  ["200-299"],
        conditions:            [],
        kafkaProducerBrokers:  [],
        kafkaProducerSaslOptions: {},
    });
    if (!res || !res.ok) throw new Error(`add returned: ${JSON.stringify(res)}`);
    return res.monitorID ?? res.monitor?.id;
}

async function resumeMonitor(socket, id) {
    const [res] = await call(socket, "resumeMonitor", id);
    if (!res || !res.ok) throw new Error(`resumeMonitor returned: ${JSON.stringify(res)}`);
}

async function main() {
    log(`Target: ${KUMA_URL}  count: ${COUNT}  parallel: ${PARALLEL}  interval: ${INTERVAL}s`);
    log(`Opening ${PARALLEL} socket connections ...`);

    const sockets = await Promise.all(
        Array.from({ length: PARALLEL }, () => openSocket())
    );
    await Promise.all(sockets.map(s => authenticate(s)));
    log(`${sockets.length} sockets authenticated.`);

    // Phase 1: create all monitors as inactive
    const queue = Array.from({ length: COUNT }, (_, i) => i + 1);
    const monitorIds = [];
    let ok = 0, failed = 0;
    const start = Date.now();

    async function createWorker(socket) {
        while (true) {
            const i = queue.shift();
            if (i === undefined) break;
            try {
                const id = await addMonitor(socket, i);
                if (id != null) monitorIds.push(id);
                ok++;
                if (ok % 200 === 0) {
                    const s = ((Date.now() - start) / 1000).toFixed(0);
                    log(`  ${ok}/${COUNT} created (${s}s elapsed)`);
                }
            } catch (e) {
                log(`WARNING monitor ${i}: ${e.message}`);
                failed++;
            }
        }
    }

    await Promise.all(sockets.map(s => createWorker(s)));

    const createElapsed = ((Date.now() - start) / 1000).toFixed(1);
    log(`Phase 1 done: ${ok} created (inactive), ${failed} failed in ${createElapsed}s`);

    if (ok === 0) { log("ERROR: no monitors created"); process.exit(1); }

    // Phase 2: resume all monitors now that seeding is complete
    log(`Phase 2: activating ${monitorIds.length} monitors ...`);
    const resumeQueue = [...monitorIds];
    let resumed = 0, resumeFailed = 0;

    async function resumeWorker(socket) {
        while (true) {
            const id = resumeQueue.shift();
            if (id === undefined) break;
            try {
                await resumeMonitor(socket, id);
                resumed++;
                if (resumed % 200 === 0) {
                    log(`  ${resumed}/${monitorIds.length} activated`);
                }
            } catch (e) {
                log(`WARNING resume ${id}: ${e.message}`);
                resumeFailed++;
            }
        }
    }

    await Promise.all(sockets.map(s => resumeWorker(s)));

    const totalElapsed = ((Date.now() - start) / 1000).toFixed(1);
    log(`Phase 2 done: ${resumed} activated, ${resumeFailed} failed`);
    log(`Total elapsed: ${totalElapsed}s`);

    for (const s of sockets) s.disconnect();
}

main().catch(e => { console.error(`[kuma-seed] FATAL: ${e.message}`); process.exit(1); });
