"use client";

import { useEffect, useRef } from "react";
import type { Terminal as XTerm } from "xterm";
import type { FitAddon } from "xterm-addon-fit";

type TerminalProps = {
  processId: string | null;
  wsBase: string;
};

export default function Terminal({ processId, wsBase }: TerminalProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<XTerm | null>(null);
  const socketRef = useRef<WebSocket | null>(null);

  useEffect(() => {
    let active = true;
    let term: XTerm | null = null;
    let fitAddon: FitAddon | null = null;
    let removeResize: (() => void) | null = null;

    const setup = async () => {
      if (!containerRef.current) {
        return;
      }

      const [{ Terminal }, { FitAddon }] = await Promise.all([
        import("xterm"),
        import("xterm-addon-fit")
      ]);

      if (!active || !containerRef.current) {
        return;
      }

      term = new Terminal({
        cursorBlink: true,
        fontSize: 12,
        fontFamily:
          "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, Liberation Mono, Courier New, monospace",
        theme: {
          background: "#050505",
          foreground: "#FAFAFA",
          cursor: "#FAFAFA",
          selectionBackground: "#262626"
        }
      });

      fitAddon = new FitAddon();
      term.loadAddon(fitAddon);
      term.open(containerRef.current);
      fitAddon.fit();
      termRef.current = term;

      const handleResize = () => fitAddon?.fit();
      window.addEventListener("resize", handleResize);
      removeResize = () => window.removeEventListener("resize", handleResize);
    };

    void setup();

    return () => {
      active = false;
      removeResize?.();
      term?.dispose();
      termRef.current = null;
    };
  }, []);

  useEffect(() => {
    const term = termRef.current;
    if (!term) {
      return;
    }

    if (socketRef.current) {
      socketRef.current.close();
      socketRef.current = null;
    }

    term.reset();

    if (!processId) {
      return;
    }

    const socket = new WebSocket(`${wsBase}/ws/${processId}`);
    socket.binaryType = "arraybuffer";
    socketRef.current = socket;

    socket.onmessage = (event) => {
      if (!termRef.current) {
        return;
      }
      if (typeof event.data === "string") {
        termRef.current.write(event.data);
      } else {
        termRef.current.write(new Uint8Array(event.data));
      }
    };

    socket.onclose = () => {
      termRef.current?.writeln("\r\n[disconnected]\r\n");
    };

    const disposable = term.onData((data) => {
      if (socket.readyState === WebSocket.OPEN) {
        socket.send(data);
      }
    });

    return () => {
      disposable.dispose();
      socket.close();
    };
  }, [processId, wsBase]);

  return <div ref={containerRef} className="h-full w-full" />;
}
