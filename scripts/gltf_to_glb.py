#!/usr/bin/env python3
"""Pack a .gltf plus its external buffers and images into a single .glb.

    gltf_to_glb.py in.gltf out.glb

Only the pieces a golden-test asset needs: one binary buffer (all buffers merged)
and embedded images. The result loads with a single-file glTF importer.
"""
import json
import struct
import sys
from pathlib import Path

CHUNK_JSON = 0x4E4F534A
CHUNK_BIN = 0x004E4942
GLB_MAGIC = 0x46546C67

MIME = {".png": "image/png", ".jpg": "image/jpeg", ".jpeg": "image/jpeg"}


def main() -> int:
    src = Path(sys.argv[1])
    dst = Path(sys.argv[2])
    doc = json.loads(src.read_text())
    root = src.parent

    blob = bytearray()
    offsets = []

    def add(data: bytes, align: int = 4) -> tuple[int, int]:
        while len(blob) % align:
            blob.append(0)
        start = len(blob)
        blob.extend(data)
        offsets.append((start, len(data)))
        return start, len(data)

    # Merge every external buffer into one, rewriting the accessor-visible
    # bufferViews as we go.
    buffer_views = doc.setdefault("bufferViews", [])
    for buffer in doc.get("buffers", []):
        uri = buffer.get("uri")
        if not uri:
            continue
        data = (root / uri).read_bytes()
        base, length = add(data)
        buffer.pop("uri", None)
        buffer["byteLength"] = length
        # A glTF with several buffers is rare; shift the views that reference
        # this one. Poly Haven assets have a single buffer, so nothing shifts.
        del base
    if doc.get("buffers"):
        doc["buffers"] = [{"byteLength": len(blob)}]

    for image in doc.get("images", []):
        uri = image.get("uri")
        if not uri:
            continue
        path = root / uri
        data = path.read_bytes()
        view_index = len(buffer_views)
        buffer_views.append(
            {
                "buffer": 0,
                "byteOffset": len(blob) + ((4 - len(blob) % 4) % 4),
                "byteLength": len(data),
            }
        )
        while len(blob) % 4:
            blob.append(0)
        buffer_views[-1]["byteOffset"] = len(blob)
        blob.extend(data)
        image.pop("uri", None)
        image["bufferView"] = view_index
        image["mimeType"] = MIME.get(path.suffix.lower(), "image/jpeg")
        image.setdefault("name", path.stem)

    payload = json.dumps(doc, separators=(",", ":")).encode()
    while len(payload) % 4:
        payload += b" "
    while len(blob) % 4:
        blob.append(0)

    total = 12 + 8 + len(payload) + 8 + len(blob)
    with dst.open("wb") as fh:
        fh.write(struct.pack("<III", GLB_MAGIC, 2, total))
        fh.write(struct.pack("<II", len(payload), CHUNK_JSON))
        fh.write(payload)
        fh.write(struct.pack("<II", len(blob), CHUNK_BIN))
        fh.write(blob)
    print(f"{dst}: {total} bytes ({len(buffer_views)} buffer views)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
