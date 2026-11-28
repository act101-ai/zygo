#!/usr/bin/env python3
"""https_static.py <dir> <cert.pem> <key.pem> <port file>: static HTTPS server on 127.0.0.1
(WebFetch upgrades http:// to https://; the experiment trusts the cert via NODE_EXTRA_CA_CERTS)."""
import functools
import os
import socketserver
import ssl
import sys
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer


class Server(ThreadingHTTPServer):
    """ThreadingHTTPServer with no host name lookup (http.server's server_bind calls socket.getfqdn,
    which can stall); server_name is the host string."""

    def server_bind(self):
        socketserver.TCPServer.server_bind(self)
        self.server_name, self.server_port = self.server_address[0], self.server_address[1]


d, cert, key, port_file = sys.argv[1:5]
srv = Server(("127.0.0.1", 0), functools.partial(SimpleHTTPRequestHandler, directory=d))
ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
ctx.load_cert_chain(cert, key)
srv.socket = ctx.wrap_socket(srv.socket, server_side=True)
open(port_file + ".tmp", "w").write(str(srv.server_address[1]))
os.rename(port_file + ".tmp", port_file)
srv.serve_forever()
