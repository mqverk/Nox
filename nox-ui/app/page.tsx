"use client";

import { useEffect, useMemo, useState } from "react";
import Terminal from "../components/Terminal";

type ProcessInfo = {
  id: string;
  name: string;
  status: "Starting" | "Online" | "Stopping" | "Offline" | "Crashed";
  pid: number | null;
  cpu: number;
  memory: number;
  uptime: number;
  restarts: number;
};

const API_BASE = process.env.NEXT_PUBLIC_NOX_API_BASE ?? "http://localhost:8080";
const WS_BASE = API_BASE.replace(/^http/, "ws");

function formatUptime(seconds: number) {
  const hrs = Math.floor(seconds / 3600);
  const mins = Math.floor((seconds % 3600) / 60);
  const secs = seconds % 60;
  return [
    hrs.toString().padStart(2, "0"),
    mins.toString().padStart(2, "0"),
    secs.toString().padStart(2, "0")
  ].join(":");
}

function formatBytes(bytes: number) {
  const mb = bytes / 1024 / 1024;
  if (mb < 1024) {
    return `${mb.toFixed(1)} MB`;
  }
  return `${(mb / 1024).toFixed(2)} GB`;
}

function statusTone(status: ProcessInfo["status"]) {
  switch (status) {
    case "Online":
      return "text-emerald-400";
    case "Starting":
      return "text-sky-400";
    case "Stopping":
      return "text-amber-400";
    case "Crashed":
      return "text-rose-400";
    default:
      return "text-zinc-400";
  }
}

export default function Page() {
  const [processes, setProcesses] = useState<ProcessInfo[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    const load = async () => {
      try {
        const res = await fetch(`${API_BASE}/api/processes`, {
          cache: "no-store"
        });
        if (!res.ok) {
          return;
        }
        const data = (await res.json()) as ProcessInfo[];
        if (active) {
          setProcesses(data);
        }
      } catch {
        if (active) {
          setProcesses([]);
        }
      }
    };

    load();
    const timer = setInterval(load, 1000);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, []);

  useEffect(() => {
    if (!selectedId && processes.length > 0) {
      setSelectedId(processes[0].id);
      return;
    }
    if (selectedId && !processes.find((process) => process.id === selectedId)) {
      setSelectedId(processes[0]?.id ?? null);
    }
  }, [processes, selectedId]);

  const maxMemory = useMemo(() => {
    return Math.max(...processes.map((process) => process.memory), 1);
  }, [processes]);

  return (
    <main className="min-h-screen bg-midnight text-ink font-mono px-6 py-6">
      <div className="grid grid-cols-1 gap-6 lg:grid-cols-[1.2fr_1fr]">
        <section className="border border-graphite p-4">
          <div className="flex items-center justify-between border-b border-graphite pb-3">
            <div>
              <div className="text-xs uppercase tracking-[0.3em] text-zinc-500">
                NOX CONTROL
              </div>
              <div className="text-xl tracking-[0.3em]">Process Grid</div>
            </div>
            <div className="text-xs text-zinc-500">{API_BASE}</div>
          </div>
          <div className="mt-4 grid grid-cols-[2fr_1fr_1.4fr_1.4fr_1fr] text-[10px] uppercase text-zinc-500">
            <div>Process</div>
            <div>Status</div>
            <div>CPU</div>
            <div>RAM</div>
            <div>Uptime</div>
          </div>
          <div className="mt-2 divide-y divide-graphite">
            {processes.map((process) => {
              const cpuWidth = Math.min(process.cpu, 100);
              const memWidth = Math.min((process.memory / maxMemory) * 100, 100);
              return (
                <button
                  key={process.id}
                  type="button"
                  onClick={() => setSelectedId(process.id)}
                  className={`grid w-full grid-cols-[2fr_1fr_1.4fr_1.4fr_1fr] items-center gap-2 py-3 text-left text-sm transition-colors ${
                    selectedId === process.id ? "bg-steel" : "bg-transparent"
                  }`}
                >
                  <div>
                    <div className="text-base">{process.name}</div>
                    <div className="text-[10px] uppercase text-zinc-600">
                      PID {process.pid ?? "--"} · Restarts {process.restarts}
                    </div>
                  </div>
                  <div className={`text-xs uppercase ${statusTone(process.status)}`}>
                    {process.status}
                  </div>
                  <div className="flex flex-col gap-1">
                    <div className="h-2 w-full bg-graphite">
                      <div
                        className="h-2 bg-ink"
                        style={{ width: `${cpuWidth}%` }}
                      />
                    </div>
                    <div className="text-[10px] text-zinc-500">
                      {process.cpu.toFixed(1)}%
                    </div>
                  </div>
                  <div className="flex flex-col gap-1">
                    <div className="h-2 w-full bg-graphite">
                      <div
                        className="h-2 bg-ink"
                        style={{ width: `${memWidth}%` }}
                      />
                    </div>
                    <div className="text-[10px] text-zinc-500">
                      {formatBytes(process.memory)}
                    </div>
                  </div>
                  <div className="text-xs text-zinc-400">
                    {formatUptime(process.uptime)}
                  </div>
                </button>
              );
            })}
            {processes.length === 0 && (
              <div className="py-6 text-sm text-zinc-600">
                No managed processes.
              </div>
            )}
          </div>
        </section>
        <section className="border border-graphite p-4">
          <div className="flex items-center justify-between border-b border-graphite pb-3">
            <div>
              <div className="text-xs uppercase tracking-[0.3em] text-zinc-500">
                Terminal
              </div>
              <div className="text-base tracking-[0.2em] text-zinc-200">
                Live I/O
              </div>
            </div>
            <div className="text-[10px] uppercase text-zinc-500">
              {selectedId ?? "idle"}
            </div>
          </div>
          <div className="mt-4 h-[520px] border border-graphite bg-black">
            <Terminal processId={selectedId} wsBase={WS_BASE} />
          </div>
        </section>
      </div>
    </main>
  );
}
