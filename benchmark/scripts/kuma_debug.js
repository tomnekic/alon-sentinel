#!/usr/bin/env node
const { io } = require("socket.io-client");

const KUMA_URL = process.argv[2] || "http://localhost:3101";
const socket = io(KUMA_URL, { transports: ["websocket"], reconnection: false, timeout: 10000 });

socket.onAny((event, ...args) => {
    const preview = JSON.stringify(args).slice(0, 200);
    console.log(`EVENT: ${event}  =>  ${preview}`);
});

socket.on("connect", () => {
    console.log("Connected, waiting for events...");
    setTimeout(() => {
        console.log("--- trying setup ---");
        socket.emit("setup", { username: "bench", password: "Bench1234!", confirmPassword: "Bench1234!" }, (...r) => {
            console.log("setup response:", JSON.stringify(r));
        });
    }, 1000);
    setTimeout(() => { socket.disconnect(); process.exit(0); }, 8000);
});

socket.on("connect_error", (e) => { console.error("connect_error:", e.message); process.exit(1); });
