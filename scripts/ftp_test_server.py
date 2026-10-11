#!/usr/bin/env python3
"""FTP/FTPS server tối giản để chạy test tích hợp và thử app ở máy dev.

    pip install pyftpdlib pyopenssl
    python3 scripts/ftp_test_server.py --root /tmp/ftp-root            # FTP thường
    python3 scripts/ftp_test_server.py --root /tmp/ftp-root --tls      # FTPS explicit, chứng chỉ tự ký

User mặc định: backup / backup123, cổng 2121.
Chạy test:
    AIO_TEST_FTP=127.0.0.1:2121:backup:backup123 AIO_TEST_FTP_ROOT=/tmp/ftp-root \
        cargo test -p backup-core --test ftp
"""
import argparse
import os
import subprocess
import tempfile

from pyftpdlib.authorizers import DummyAuthorizer
from pyftpdlib.servers import FTPServer


def self_signed_cert(path: str) -> None:
    subprocess.run(
        ["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "30",
         "-subj", "/CN=localhost", "-keyout", path, "-out", path],
        check=True, capture_output=True,
    )


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", required=True)
    ap.add_argument("--port", type=int, default=2121)
    ap.add_argument("--user", default="backup")
    ap.add_argument("--password", default="backup123")
    ap.add_argument("--tls", action="store_true")
    args = ap.parse_args()

    os.makedirs(args.root, exist_ok=True)
    auth = DummyAuthorizer()
    auth.add_user(args.user, args.password, args.root, perm="elradfmwMT")

    if args.tls:
        from pyftpdlib.handlers import TLS_FTPHandler as Handler
        cert = os.path.join(tempfile.gettempdir(), "aio-ftp-test.pem")
        if not os.path.exists(cert):
            self_signed_cert(cert)
        Handler.certfile = cert
        Handler.tls_control_required = True
        Handler.tls_data_required = True
    else:
        from pyftpdlib.handlers import FTPHandler as Handler

    Handler.authorizer = auth
    Handler.passive_ports = range(60000, 60100)
    Handler.encoding = "utf-8"
    FTPServer(("127.0.0.1", args.port), Handler).serve_forever()


if __name__ == "__main__":
    main()
