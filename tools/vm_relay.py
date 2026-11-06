"""Run on another machine (here the WSL VM). A UDP relay that behaves like the internet: each datagram is delayed (both ways), with jitter that reorders, and some are lost.

    python3 -I vm_relay.py LISTEN_PORT HOST_ADDR HOST_PORT DELAY_MS JITTER_MS LOSS SECONDS

Clients send to LISTEN_PORT; each client gets its own upstream socket, so the host sees one address per client.
"""
import heapq
import random
import select
import socket
import sys
import time

listen_port, host = int(sys.argv[1]), (sys.argv[2], int(sys.argv[3]))
delay, jitter, loss, seconds = float(sys.argv[4]) / 1000, float(sys.argv[5]) / 1000, float(sys.argv[6]), float(sys.argv[7])
lsock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
lsock.bind(("0.0.0.0", listen_port))
ups = {}
back = {}
queue = []
seq = 0
stats = {"in": 0, "lost": 0, "out": 0, "reordered": 0}
last_sent = {}
end = time.time() + seconds
while time.time() < end:
    now = time.time()
    wait = 0.002 if not queue else max(0.0, min(0.002, queue[0][0] - now))
    socks = [lsock] + list(ups.values())
    ready, _, _ = select.select(socks, [], [], wait)
    for s in ready:
        try:
            data, addr = s.recvfrom(4096)
        except OSError:
            continue
        stats["in"] += 1
        if s is lsock:
            up = ups.get(addr)
            if up is None:
                up = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
                up.bind(("0.0.0.0", 0))
                print(f"vm_relay: client {addr} -> host {host} from {up.getsockname()}", flush=True)
                ups[addr] = up
                back[up.fileno()] = addr
            out, dest = up, host
        else:
            out, dest = lsock, back[s.fileno()]
        if random.random() < loss:
            stats["lost"] += 1
            continue
        seq += 1
        heapq.heappush(queue, (now + delay + random.uniform(-jitter, jitter), seq, out, data, dest))
    now = time.time()
    while queue and queue[0][0] <= now:
        _, n, out, data, dest = heapq.heappop(queue)
        key = (out.fileno(), dest)
        if last_sent.get(key, 0) > n:
            stats["reordered"] += 1
        last_sent[key] = max(last_sent.get(key, 0), n)
        try:
            out.sendto(data, dest)
            stats["out"] += 1
        except OSError:
            pass
print(f"netem: {stats}, clients {len(ups)}")
