#!/usr/bin/env python3
import importlib.util
import json
import socket
import sys
import threading
import time
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
RECEIVER_PY = SCRIPT_DIR.parent / "scripts" / "receiver.py"

_spec = importlib.util.spec_from_file_location("receiver_mod", RECEIVER_PY)
receiver = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(receiver)


def _free_port(host="127.0.0.1"):
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.bind((host, 0))
    port = s.getsockname()[1]
    s.close()
    return port


class TestReceiver:
    def __init__(self):
        self.passes = 0
        self.fails = 0

    def assert_eq(self, got, expected, msg):
        if got == expected:
            self.passes += 1
            print(f"PASS: {msg}")
        else:
            self.fails += 1
            print(f"FAIL: {msg} (got {got!r}, expected {expected!r})")

    def assert_true(self, cond, msg):
        if cond:
            self.passes += 1
            print(f"PASS: {msg}")
        else:
            self.fails += 1
            print(f"FAIL: {msg}")

    def assert_false(self, cond, msg):
        if not cond:
            self.passes += 1
            print(f"PASS: {msg}")
        else:
            self.fails += 1
            print(f"FAIL: {msg}")

    def test_udp_basic(self):
        port = _free_port()
        host = "127.0.0.1"

        def sender():
            sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
            msg = b"X" * 250 + b"\n"
            for _ in range(50):
                sock.sendto(msg, (host, port))
                time.sleep(0.01)
            sock.close()

        t = threading.Thread(target=sender, daemon=True)
        t.start()
        result = receiver.udp_receiver(host, port, 2.0)
        t.join(timeout=5)

        self.assert_eq(result["error"], None, "udp returns no error")
        self.assert_eq(result["protocol"], "udp", "udp protocol")
        self.assert_eq(result["framing"], "datagram", "udp framing")
        self.assert_true(result["messages"] >= 40,
                        f"received >=40 msgs (got {result['messages']})")
        self.assert_true(result["bytes"] >= 40 * 100,
                        f"received >=4000 bytes (got {result['bytes']})")
        self.assert_eq(result["wire_bytes"], result["bytes"],
                      "udp wire_bytes == bytes (no framing overhead)")

    def test_tcp_octet_counting_basic(self):
        port = _free_port()
        host = "127.0.0.1"

        result_holder = []

        def run_recv():
            result_holder.append(receiver.tcp_receiver(host, port, 2.0,
                                                        framing="octet-counting"))

        t = threading.Thread(target=run_recv, daemon=True)
        t.start()
        time.sleep(0.3)

        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.connect((host, port))
        body = b"x" * 200
        for _ in range(20):
            sock.sendall(b"200 " + body)
            time.sleep(0.05)
        sock.close()
        t.join(timeout=5)

        self.assert_true(len(result_holder) == 1, "receiver returned once")
        if result_holder:
            r = result_holder[0]
            self.assert_eq(r["error"], None, "tcp octet-counting no error")
            self.assert_eq(r["framing"], "octet-counting", "framing")
            self.assert_eq(r["messages"], 20, "20 messages parsed")
            self.assert_eq(r["bytes"], 20 * 200, "20*200=4000 body bytes")
            self.assert_eq(r["wire_bytes"], 20 * (4 + 200),
                          "wire_bytes = body + length prefix only (no LF)")

    def test_tcp_non_transparent_basic(self):
        port = _free_port()
        host = "127.0.0.1"

        result_holder = []

        def run_recv():
            result_holder.append(receiver.tcp_receiver(host, port, 2.0,
                                                        framing="non-transparent"))

        t = threading.Thread(target=run_recv, daemon=True)
        t.start()
        time.sleep(0.3)

        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.connect((host, port))
        for _ in range(15):
            sock.sendall(b"line-content\n")
            time.sleep(0.05)
        sock.close()
        t.join(timeout=5)

        self.assert_true(len(result_holder) == 1, "receiver returned once")
        if result_holder:
            r = result_holder[0]
            self.assert_eq(r["error"], None, "tcp non-trans no error")
            self.assert_eq(r["framing"], "non-transparent", "framing")
            self.assert_eq(r["messages"], 15, "15 messages parsed")

    def test_tcp_octet_counting_violation(self):
        r = receiver.Receiver()
        buf = bytearray(b"0 ")
        leftover = receiver._parse_octet_counting_buffer(buf, r)
        self.assert_true(r.error is not None, "error set on zero-length header")
        self.assert_true("implausible" in r.error.lower() or "framing" in r.error.lower(),
                        f"error mentions implausible/framing: {r.error}")

    def test_tls_no_cert(self):
        result = receiver.tls_receiver("127.0.0.1", 16500, 0.5,
                                        "/nonexistent/cert.pem",
                                        "/nonexistent/key.pem")
        self.assert_true(result["error"] is not None,
                        "tls returns error on missing cert")
        self.assert_true("cert" in result["error"].lower(),
                        f"error mentions cert: {result['error']}")

    def test_kafka_stub_unreachable(self):
        result = receiver.kafka_stub_receiver("127.0.0.1", 1, 1.0, "topic")
        self.assert_true(result["error"] is not None,
                        "kafka_stub returns error on unreachable broker")
        self.assert_true("unreachable" in result["error"].lower() or
                        "refused" in result["error"].lower() or
                        "librdkafka" in result["error"].lower(),
                        f"error mentions unreachable/refused/librdkafka: {result['error']}")

    def test_kafka_stub_never_completes(self):
        result = receiver.kafka_stub_receiver("127.0.0.1", 1, 0.5, "topic")
        self.assert_eq(result["messages"], 0,
                      "kafka_stub.messages is always 0 (no fabricated completion)")

    def test_main_usage_error(self):
        rc = receiver.main([])
        self.assert_eq(rc, 2, "main() with no args returns 2")

    def test_main_unknown_proto(self):
        rc = receiver.main(["unicorn", "127.0.0.1", "1", "1"])
        self.assert_eq(rc, 2, "main() with unknown protocol returns 2")

    def test_parse_octet_counting_buffer(self):
        r = receiver.Receiver()
        buf = bytearray(b"3 foo5 hello")
        leftover = receiver._parse_octet_counting_buffer(buf, r)
        self.assert_eq(r.messages, 2, "2 messages parsed")
        self.assert_eq(r.body_bytes, 3 + 5, "8 body bytes")
        self.assert_eq(len(leftover), 0, "no leftover")
        self.assert_eq(r.error, None, "no error")

    def test_parse_octet_counting_incomplete(self):
        r = receiver.Receiver()
        buf = bytearray(b"3 foo5 hel")
        leftover = receiver._parse_octet_counting_buffer(buf, r)
        self.assert_eq(r.messages, 1, "1 message parsed (foo)")
        self.assert_eq(r.body_bytes, 3, "3 body bytes")
        self.assert_eq(bytes(leftover), b"5 hel", "leftover kept")

    def test_parse_octet_counting_lf_violation(self):
        r = receiver.Receiver()
        buf = bytearray(b"3 foo")
        leftover = receiver._parse_octet_counting_buffer(buf, r)
        self.assert_eq(r.messages, 1, "1 message parsed (no LF expected)")
        self.assert_eq(r.body_bytes, 3, "3 body bytes")
        self.assert_eq(r.error, None, "no error (octet-counting has no LF)")


def main():
    t = TestReceiver()
    methods = [
        t.test_udp_basic,
        t.test_tcp_octet_counting_basic,
        t.test_tcp_non_transparent_basic,
        t.test_tcp_octet_counting_violation,
        t.test_tls_no_cert,
        t.test_kafka_stub_unreachable,
        t.test_kafka_stub_never_completes,
        t.test_main_usage_error,
        t.test_main_unknown_proto,
        t.test_parse_octet_counting_buffer,
        t.test_parse_octet_counting_incomplete,
        t.test_parse_octet_counting_lf_violation,
    ]
    for m in methods:
        try:
            print(f"\n--- {m.__name__} ---")
            m()
        except Exception as e:
            t.fails += 1
            print(f"FAIL: {m.__name__} raised {e!r}")

    print(f"\n=== harness/test_receiver.py summary ===")
    print(f"PASSES: {t.passes}")
    print(f"FAILS:  {t.fails}")
    return 0 if t.fails == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
