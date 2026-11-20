#!/usr/bin/env python3
"""make_files.py <dir>: test inputs for the tool experiments (text, PNG, PDF, notebook, HTML)."""
import json
import os
import struct
import sys
import zlib

d = sys.argv[1]
os.makedirs(d, exist_ok=True)
open(os.path.join(d, "README.txt"), "w").write("tracer readme line one\ntracer readme line two\n")
open(os.path.join(d, "page.html"), "w").write("<html><body><h1>tracer page</h1><p>tracer page text</p></body></html>\n")


def chunk(t, data):
    c = t + data
    return struct.pack(">I", len(data)) + c + struct.pack(">I", zlib.crc32(c) & 0xFFFFFFFF)


raw = b"".join(b"\x00" + b"\xff\x00\x00" * 8 for _ in range(8))
png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 8, 8, 8, 2, 0, 0, 0)) + \
    chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")
open(os.path.join(d, "img.png"), "wb").write(png)

objs = [b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>",
        None,
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"]
stream = b"BT /F1 12 Tf 10 50 Td (tracer pdf text) Tj ET"
objs[3] = b"<< /Length %d >>\nstream\n" % len(stream) + stream + b"\nendstream"
out = b"%PDF-1.4\n"
offs = []
for i, o in enumerate(objs, 1):
    offs.append(len(out))
    out += b"%d 0 obj\n" % i + o + b"\nendobj\n"
xref = len(out)
out += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objs) + 1)
out += b"".join(b"%010d 00000 n \n" % o for o in offs)
out += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (len(objs) + 1, xref)
open(os.path.join(d, "doc.pdf"), "wb").write(out)

nb = {"cells": [{"cell_type": "code", "id": "c1", "metadata": {}, "execution_count": None,
                 "outputs": [], "source": ["print('tracer notebook')"]}],
      "metadata": {}, "nbformat": 4, "nbformat_minor": 5}
json.dump(nb, open(os.path.join(d, "nb.ipynb"), "w"), indent=1)
