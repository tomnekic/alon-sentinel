const http = require("http");

let flap = false;

const server = http.createServer((req, res) => {
  if (req.url === "/ok") {
    res.writeHead(200, { "Content-Type": "application/json", "X-Alon-Test": "ok" });
    return res.end(JSON.stringify({ status: "ok" }));
  }

  if (req.url === "/fail") {
    res.writeHead(500, { "Content-Type": "application/json" });
    return res.end(JSON.stringify({ status: "fail" }));
  }

  if (req.url === "/slow") {
    return setTimeout(() => {
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ status: "slow" }));
    }, 500);
  }

  if (req.url === "/flap") {
    flap = !flap;
    res.writeHead(flap ? 200 : 500, { "Content-Type": "application/json" });
    return res.end(JSON.stringify({ status: flap ? "ok" : "fail" }));
  }

  res.writeHead(404);
  res.end("not found");
});

server.listen(8088, "0.0.0.0", () => {
  console.log("target service running on :8088");
});
