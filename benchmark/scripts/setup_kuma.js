#!/usr/bin/env node
// Configures Uptime Kuma via its Socket.io API and seeds HTTP monitors.
// Usage: node setup_kuma.js [kuma_url] [username] [password] [monitor_count] [check_interval] [target_url] [token_out_file]

const { io } = require("socket.io-client");
const fs = require("fs");
const path = require("path");

const KUMA_URL       = process.argv[2] || "http://localhost:3101";
const USERNAME       = process.argv[3] || "bench";
const PASSWORD       = process.argv[4] || "Bench1234!";
const MONITOR_COUNT  = parseInt(process.argv[5] || "50", 10);
const CHECK_INTERVAL = parseInt(process.argv[6] || "60", 10);
const TARGET_URL     = process.argv[7] || "http://target:8088/ok";
const TOKEN_OUT_FILE = process.argv[8] || "";

function log(msg) { console.log(`[kuma-setup] ${msg}`); }

function call(socket, event, ...args) {
    return new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error(`Timeout waiting for ${event} response`)), 10000);
        socket.emit(event, ...args, (...response) => {
            clearTimeout(timer);
            resolve(response);
        });
    });
}

async function main() {
    log(`Connecting to ${KUMA_URL} ...`);
    const socket = io(KUMA_URL, {
        transports: ["websocket"],
        reconnection: false,
        timeout: 10000,
    });

    await new Promise((resolve, reject) => {
        socket.on("connect", resolve);
        socket.on("connect_error", reject);
        setTimeout(() => reject(new Error("Socket connect timeout")), 15000);
    });
    log("Connected.");

    // Wait for info event (Kuma sends server info on connect)
    await new Promise(resolve => setTimeout(resolve, 500));

    // Try to setup admin user (first-run only; silently ignore if already done)
    log(`Creating admin account (user: ${USERNAME}) ...`);
    try {
        const [res] = await call(socket, "setup", USERNAME, PASSWORD);
        if (res && res.ok) {
            log("Admin account created.");
        } else if (res && res.msg && res.msg.toLowerCase().includes("exist")) {
            log("Admin account already exists — continuing.");
        } else {
            log(`Setup response: ${JSON.stringify(res)} — will attempt login anyway.`);
        }
    } catch (e) {
        log(`Setup call failed (${e.message}) — will attempt login anyway.`);
    }

    // Login
    log("Logging in ...");
    const [loginRes] = await call(socket, "login", { username: USERNAME, password: PASSWORD, token: "" });
    if (!loginRes || !loginRes.ok) {
        throw new Error(`Login failed: ${JSON.stringify(loginRes)}`);
    }
    log("Logged in.");

    const token = loginRes.token || "";
    if (TOKEN_OUT_FILE && token) {
        fs.writeFileSync(TOKEN_OUT_FILE, token);
        log(`Bearer token saved to ${TOKEN_OUT_FILE}`);
    }

    // Seed monitors
    log(`Seeding ${MONITOR_COUNT} HTTP monitors (interval: ${CHECK_INTERVAL}s) ...`);
    let ok = 0, skipped = 0;
    for (let i = 1; i <= MONITOR_COUNT; i++) {
        try {
            const [res] = await call(socket, "add", {
                type: "http",
                name: `Bench Monitor ${i}`,
                url: TARGET_URL,
                interval: CHECK_INTERVAL,
                retryInterval: 20,
                maxretries: 0,
                method: "GET",
                active: true,
                notificationIDList: {},
                accepted_statuscodes: ["200-299"],
                kafkaProducerBrokers: [],
                kafkaProducerSaslOptions: {},
            });
            if (res && res.ok) {
                ok++;
            } else {
                log(`WARNING: monitor ${i} — ${JSON.stringify(res)}`);
                skipped++;
            }
        } catch (e) {
            log(`WARNING: monitor ${i} failed: ${e.message}`);
            skipped++;
        }
    }
    log(`Done. ${ok} monitors created, ${skipped} skipped.`);

    socket.disconnect();
}

main().catch(err => {
    console.error(`[kuma-setup] FATAL: ${err.message}`);
    process.exit(1);
});
