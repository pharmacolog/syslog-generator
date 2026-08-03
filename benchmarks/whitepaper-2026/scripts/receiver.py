#!/usr/bin/env python3
import json
import os
import re
import socket
import ssl
import sys
import threading
import time


_OCTET_RE = re.compile(rb"^(\d+) ")


def _now():
    return time.monotonic()


class Receiver:
    def __init__(self):
        self.body_bytes = 0
        self.wire_bytes = 0
        self.messages = 0
        self.error = None
        self.lock = threading.Lock()
        self.stop = threading.Event()


def _record(r, body, wire, msgs):
    with r.lock:
        r.body_bytes += body
        r.wire_bytes += wire
        r.messages += msgs


def udp_receiver(host, port, duration):
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    sock.bind((host, port))
    sock.settimeout(0.5)

    r = Receiver()
    start = _now()
    deadline = start + duration

    while _now() < deadline and not r.stop.is_set():
        try:
            data, _addr = sock.recvfrom(65535)
        except socket.timeout:
            continue
        except OSError as e:
            r.error = str(e)
            break
        _record(r, body=len(data), wire=len(data), msgs=1)

    sock.close()
    return _recv_result(r, start, "udp", "datagram")


def _parse_octet_counting_buffer(buf, r):
    while True:
        m = _OCTET_RE.match(bytes(buf))
        if not m:
            break
        length = int(m.group(1))
        if length <= 0 or length > 65535:
            r.error = f"octet-counting: implausible length {length}"
            del buf[:m.end()]
            return buf
        body_start = m.end()
        body_end = body_start + length
        if len(buf) < body_end:
            break
        _record(r, body=length, wire=body_end, msgs=1)
        del buf[:body_end]
    return buf


def _parse_non_transparent_buffer(buf, r):
    while True:
        idx = buf.find(b"\n")
        if idx < 0:
            break
        _record(r, body=idx, wire=idx + 1, msgs=1)
        del buf[:idx + 1]
    return buf


def _tcp_loop(conn, framing, r, deadline):
    if framing == "octet-counting":
        parser = _parse_octet_counting_buffer
    else:
        parser = _parse_non_transparent_buffer
    buf = bytearray()
    while _now() < deadline and not r.stop.is_set():
        try:
            chunk = conn.recv(262144)
        except (socket.timeout, ssl.SSLError, OSError):
            break
        if not chunk:
            break
        buf.extend(chunk)
        buf = parser(buf, r)
        if r.error:
            break
        if len(buf) > 1 << 20:
            r.error = f"buffer overflow at {len(buf)} bytes"
            break


def _tcp_receiver_common(host, port, duration, framing, ssl_ctx=None):
    server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    server.bind((host, port))
    server.listen(8)
    server.settimeout(0.5)

    r = Receiver()
    start = _now()
    deadline = start + duration
    threads = []

    try:
        while _now() < deadline and not r.stop.is_set():
            try:
                conn, _addr = server.accept()
            except socket.timeout:
                continue
            conn.settimeout(0.5)
            if ssl_ctx is not None:
                def handler(c=conn):
                    try:
                        tls_conn = ssl_ctx.wrap_socket(c, server_side=True)
                    except (ssl.SSLError, OSError) as e:
                        r.error = f"TLS handshake failed: {e}"
                        return
                    try:
                        _tcp_loop(tls_conn, framing, r, deadline)
                    finally:
                        try:
                            tls_conn.close()
                        except OSError:
                            pass
            else:
                def handler(c=conn):
                    try:
                        _tcp_loop(c, framing, r, deadline)
                    finally:
                        try:
                            c.close()
                        except OSError:
                            pass
            t = threading.Thread(target=handler, daemon=True)
            t.start()
            threads.append(t)
        for t in threads:
            t.join(timeout=max(0.0, deadline - _now()))
    finally:
        r.stop.set()
        for t in threads:
            t.join(timeout=1.0)
        try:
            server.close()
        except OSError:
            pass

    return _recv_result(r, start, "tcp" if ssl_ctx is None else "tls", framing)


def _recv_result(r, start, proto, framing):
    return {
        "bytes": r.body_bytes,
        "wire_bytes": r.wire_bytes,
        "messages": r.messages,
        "duration_secs": _now() - start,
        "framing": framing,
        "protocol": proto,
        "error": r.error,
    }


def tcp_receiver(host, port, duration, framing="octet-counting"):
    return _tcp_receiver_common(host, port, duration, framing, ssl_ctx=None)


def tls_receiver(host, port, duration, cert, key, framing="octet-counting"):
    if not os.path.isfile(cert):
        return {"bytes": 0, "wire_bytes": 0, "messages": 0,
                "duration_secs": 0,
                "framing": framing, "protocol": "tls",
                "error": f"cert not found: {cert}"}
    if not os.path.isfile(key):
        return {"bytes": 0, "wire_bytes": 0, "messages": 0,
                "duration_secs": 0,
                "framing": framing, "protocol": "tls",
                "error": f"key not found: {key}"}
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ctx.load_cert_chain(certfile=cert, keyfile=key)
    return _tcp_receiver_common(host, port, duration, framing, ssl_ctx=ctx)


def kafka_stub_receiver(host, port, duration, topic):
    start = _now()
    deadline = start + duration
    sock = None
    try:
        sock = socket.create_connection((host, port), timeout=5)
    except OSError as e:
        return {"bytes": 0, "wire_bytes": 0, "messages": 0,
                "duration_secs": 0,
                "framing": "kafka-protocol", "protocol": "kafka",
                "error": f"kafka broker unreachable: {e}"}
    try:
        while _now() < deadline:
            time.sleep(0.5)
    finally:
        if sock:
            try:
                sock.close()
            except OSError:
                pass
    return {"bytes": 0, "wire_bytes": 0, "messages": 0,
            "duration_secs": _now() - start,
            "framing": "kafka-protocol", "protocol": "kafka",
            "error": "kafka_stub: full consumer requires librdkafka; v1 never marks Kafka cells as completed"}


def main(argv):
    if len(argv) < 4:
        print("usage: receiver.py <udp|tcp|tls|kafka> <host> <port> <duration_secs> [cert] [key] [framing|topic]", file=sys.stderr)
        return 2
    proto = argv[0]
    host = argv[1]
    try:
        port = int(argv[2])
        duration = float(argv[3])
    except ValueError as e:
        print(json.dumps({"error": f"bad port/duration: {e}"}))
        return 2

    try:
        if proto == "udp":
            result = udp_receiver(host, port, duration)
        elif proto == "tcp":
            framing = argv[4] if len(argv) > 4 else "octet-counting"
            result = tcp_receiver(host, port, duration, framing)
        elif proto == "tls":
            if len(argv) < 6:
                print(json.dumps({"error": "tls requires cert and key paths"}))
                return 2
            cert, key = argv[4], argv[5]
            framing = argv[6] if len(argv) > 6 else "octet-counting"
            result = tls_receiver(host, port, duration, cert, key, framing)
        elif proto == "kafka":
            topic = argv[4] if len(argv) > 4 else "default"
            result = kafka_stub_receiver(host, port, duration, topic)
        else:
            print(json.dumps({"error": f"unknown protocol: {proto}"}))
            return 2
    except Exception as e:
        result = {"bytes": 0, "wire_bytes": 0, "messages": 0,
                  "duration_secs": 0,
                  "framing": "unknown", "protocol": proto,
                  "error": f"receiver exception: {e!r}"}

    print(json.dumps(result))
    return 0 if result.get("error") is None else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
