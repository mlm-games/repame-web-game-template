#!/usr/bin/env python3
"""Complete the port's material tables from the reference's dumped materials.

`assets/dd1/mesh_materials.json` is keyed by the GameObject names the exporter
saw, which leaves meshes named after their own asset (the school bus, the train,
the road blocks, signs) without an entry — they draw untextured. This resolves
those through the export manifest's mesh names, then stamps the reference's alpha
mode (cutout for foliage, fences, decals; blend where the shader blends).

    scripts/materials.py [refs/DeadlyDescent/extracted] [assets/dd1]
"""
import json
import pathlib
import sys

ref = pathlib.Path(
    sys.argv[1] if len(sys.argv) > 1 else pathlib.Path.home() / "Documents/refs/DeadlyDescent/extracted"
)
dst = pathlib.Path(sys.argv[2] if len(sys.argv) > 2 else "assets/dd1")


def material_records():
    dump = json.loads((ref / "materials" / "materials.json").read_text())
    return {r.get("file_rel_name") or r.get("name"): r for r in dump["materials"]}


def mesh_record(record):
    """The port's entry for one dumped material."""
    textures = record.get("textures") or {}
    main = (textures.get("_MainTex") or {}).get("texture") or {}
    colors = {}
    if "_Color" in (record.get("colors") or {}):
        colors["_Color"] = record["colors"]["_Color"]
    entry = {"material": record.get("file_rel_name") or record.get("name")}
    texture = main.get("exported_png")
    if texture:
        entry["texture"] = texture.rsplit(".", 1)[0]
    if colors:
        entry["colors"] = colors
    alpha = alpha_of(record)
    entry.update(alpha)
    return entry


def alpha_of(record):
    shader = (record.get("shader_name") or "").lower()
    floats = record.get("floats") or {}
    cutoff = floats.get("_Cutoff")
    if "cutout" in shader:
        return {"cutoff": round(cutoff if cutoff is not None else 0.5, 4)}
    if "transparent" in shader or shader.startswith("particles"):
        return {"transparent": True}
    return {}


def main():
    records = material_records()
    manifest = json.loads((ref / "MANIFEST.json").read_text())
    path = dst / "mesh_materials.json"
    table = json.loads(path.read_text())

    # 1. Fill entries the owner-name keying missed, by the export's mesh names.
    filled = 0
    for mesh in manifest["meshes"]:
        file_rel = mesh.get("file_rel")
        if not file_rel:
            continue
        stem = pathlib.Path(file_rel).stem
        if stem in table:
            continue
        record = records.get(mesh.get("mesh_name") or "")
        if record is None:
            continue
        table[stem] = mesh_record(record)
        filled += 1

    # 2. Stamp the alpha mode on every entry that names a dumped material.
    stamped = 0
    unmatched = 0
    for entry in table.values():
        name = entry.get("material")
        if not name:
            continue
        record = records.get(name)
        if record is None:
            unmatched += 1
            continue
        alpha = alpha_of(record)
        if alpha:
            entry.update(alpha)
            stamped += 1
    path.write_text(json.dumps(table, indent=1) + "\n")

    cars_path = dst / "car_materials.json"
    cars = json.loads(cars_path.read_text())
    car_done = 0
    for entries in cars.values():
        for entry in entries:
            record = records.get(entry.get("material") or "")
            if record is None:
                continue
            alpha = alpha_of(record)
            if alpha:
                entry.update(alpha)
                car_done += 1
    cars_path.write_text(json.dumps(cars, indent=1) + "\n")
    print(
        f"materials: {len(records)}, entries filled: {filled}, alpha modes: "
        f"{stamped} prop / {car_done} car, entries naming no dumped material: {unmatched}"
    )


if __name__ == "__main__":
    main()
