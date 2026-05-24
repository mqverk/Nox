# NOX

> Extremely fast, native process management and monitoring for servers.

I built this on too little sleep and too much coffee. It still works. Mostly.

## What it is

NOX is a bare-metal process supervisor + web panel:

- **noxd (Rust/Tokio)**: spawns native processes, tracks PID, streams logs, accepts stdin, restarts on crash.
- **nox-ui (Next.js + Tailwind + xterm.js)**: midnight-clinical dashboard + live terminal.

No Docker. No VM. Just raw OS processes.

## Run it

### 1) Daemon

```bash
cargo build
./target/debug/noxd
```

The daemon listens on **http://localhost:8080**.

### 2) UI

```bash
cd nox-ui
npm install
npm run dev
```

Open **http://localhost:3000**.

If the daemon is elsewhere, set:

```bash
export NEXT_PUBLIC_NOX_API_BASE="http://your-host:8080"
```

## API (tiny, sharp, adequate)

### List processes

```
GET /api/processes
```

### Start a process

```
POST /api/processes
{
  "name": "my-server",
  "command": "/usr/bin/my-server",
  "args": ["--port", "9001"],
  "auto_restart": true
}
```

### Stop a process

```
POST /api/processes/:id/stop
```

### WebSocket (stdout/stderr + stdin)

```
WS /ws/:id
```

## Notes I will regret later

- **Uptime/CPU/RAM** are for the child PID only.
- **Restart policy** is on by default.
- If you kill the daemon, the world ends. That’s on you.
