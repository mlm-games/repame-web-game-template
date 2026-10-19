#!/usr/bin/env python3
"""Dump per-submesh materials and the material table the port loads with.

Unity draws a mesh's submeshes with a renderer's material list, one material per
submesh. The export keeps the submeshes but not the list, so a truck body whose
five submeshes are paint, parts, mirror, windows and glass came out painted end to
end. This walks the shipped bundle's MeshFilter/MeshRenderer pairs and writes:

    assets/dd1/mesh_submaterials.json   "file|mesh path id" -> [material names]
    assets/dd1/materials.json           material name -> texture, cutoff, colour

    scripts/mesh_submaterials.py [data.unity3d] [refs/DeadlyDescent/extracted] [assets/dd1]
"""
import json
import pathlib
import sys

bundle = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "/tmp/opencode/dd/data/data.unity3d")
ref = pathlib.Path(
    sys.argv[2] if len(sys.argv) > 2 else pathlib.Path.home() / "Documents/refs/DeadlyDescent/extracted"
)
dst = pathlib.Path(sys.argv[3] if len(sys.argv) > 3 else "assets/dd1")


def material_table():
    """Union of the dumped materials: the older dump carries shaders, the fresh
    one the level materials it missed."""
    records = []
    for path in (ref / "materials" / "materials.json", pathlib.Path("/tmp/opencode/dd/data/materials_full.json")):
        if path.exists():
            records.extend(json.loads(path.read_text())["materials"])
    table = {}
    for record in records:
        name = record.get("file_rel_name") or record.get("name")
        if not name:
            continue
        entry = table.setdefault(name, {})
        main = (record.get("textures") or {}).get("_MainTex") or {}
        exported = (main.get("texture") or {}).get("exported_png")
        if exported:
            entry["texture"] = exported.rsplit(".", 1)[0]
        color = (record.get("colors") or {}).get("_Color")
        if color and "color" not in entry:
            entry["color"] = [round(v, 4) for v in color[:3]]
        shader = (record.get("shader_name") or "").lower()
        cutoff = (record.get("floats") or {}).get("_Cutoff")
        if "cutout" in shader:
            entry["cutoff"] = round(cutoff if cutoff is not None else 0.5, 4)
        elif "transparent" in shader or shader.startswith("particles"):
            entry["transparent"] = True
    return table


def mesh_materials():
    import UnityPy

    env = UnityPy.load(str(bundle))
    filters = {}
    renderers = {}
    for obj in env.objects:
        if obj.type.name not in ("MeshFilter", "MeshRenderer"):
            continue
        try:
            data = obj.read()
        except Exception:
            continue
        game_object = getattr(data, "m_GameObject", None)
        if game_object is None:
            continue
        key = (pathlib.Path(str(obj.assets_file.name)).name, game_object.path_id)
        if obj.type.name == "MeshFilter":
            # The mesh asset may live in another file of the bundle; key by the
            # file it is in, which is what the export manifest records.
            try:
                mesh = data.m_Mesh.read()
                mesh_file = pathlib.Path(str(mesh.assets_file.name)).name
                filters[key] = (mesh.path_id, mesh_file)
            except Exception:
                filters[key] = (data.m_Mesh.path_id, str(obj.assets_file.name))
        else:
            names = []
            for slot in data.m_Materials:
                try:
                    names.append(slot.read().m_Name)
                except Exception:
                    names.append(None)
            renderers[key] = names
    # Key by mesh id alone: the same mesh is copied into every scene that uses it,
    # so the copies agree — and any id the copies disagree on is dropped rather
    # than bound to the wrong material.
    seen = {}
    conflicts = 0
    for key, (mesh_id, _file_name) in filters.items():
        names = renderers.get(key)
        if not names or mesh_id is None or any(n is None for n in names):
            continue
        previous = seen.get(mesh_id)
        if previous is None:
            seen[mesh_id] = names
        elif previous != names:
            conflicts += 1
            seen[mesh_id] = None
    out = {str(mesh_id): names for mesh_id, names in seen.items() if names}
    print(f"mesh ids with submesh materials: {len(out)}, conflicting ids dropped: {conflicts}")
    return out


def main():
    table = material_table()
    (dst / "materials.json").write_text(json.dumps(table, indent=1, sort_keys=True) + "\n")
    submesh = mesh_materials()
    (dst / "mesh_submaterials.json").write_text(json.dumps(submesh, indent=1, sort_keys=True) + "\n")
    multi = sum(1 for names in submesh.values() if len(names) > 1)
    print(f"materials: {len(table)}, meshes with submesh materials: {len(submesh)} ({multi} multi-material)")


if __name__ == "__main__":
    main()
