#!/usr/bin/env python3
"""https_static.py <dir> <cert.pem> <key.pem> <port file>: static HTTPS server on 127.0.0.1
(WebFetch upgrades http:// to https://; the experiment trusts the cert via NODE_EXTRA_CA_CERTS)."""
import functools
import os
import ssl
import sys
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer

d, cert, key, port_file = sys.argv[1:5]
srv = ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(SimpleHTTPRequestHandler, directory=d))
ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
ctx.load_cert_chain(cert, key)
srv.socket = ctx.wrap_socket(srv.socket, server_side=True)
open(port_file + ".tmp", "w").write(str(srv.server_address[1]))
os.rename(port_file + ".tmp", port_file)
srv.serve_forever()
