#!/usr/bin/env python3
"""Write the Garage scene's layout: room pieces and car display, as shipped.

The reference's Garage scene (BuildSettings entry 1, `level1` in the bundle) holds
the room the car is shown in: a floor, walls, a window and garage props, plus all
ten cars stacked at the origin with only the selected one enabled. This walks the
scene's transform hierarchy and writes the world transforms the port draws with.

    scripts/garage_layout.py [data.unity3d] [assets/dd1]

Needs `UnityPy` and the decoded WebGL data bundle (see scripts/extract_notes.md).
"""
import json
import pathlib
import sys

from scene_xform import decompose, local_matrix, mul

BUNDLE = pathlib.Path(
    sys.argv[1] if len(sys.argv) > 1 else "/tmp/opencode/dd/data/data.unity3d"
)
DST = pathlib.Path(sys.argv[2] if len(sys.argv) > 2 else "assets/dd1")

ROOM = {
    "Cube": "Cube",
    "Plane001": "Plane001",
    "Box001": "Box001",
    "Object007": "Object007",
    "Object013": "Object013",
    "Object035": "Object035",
    "Object051": "Object051",
    "Object057": "Object057",
    "Object063": "Object063",
}


def main():
    import UnityPy

    env = UnityPy.load(str(BUNDLE))
    scene_file = None
    for obj in env.objects:
        if obj.type.name != "GameObject":
            continue
        try:
            data = obj.read()
        except Exception:
            continue
        if data.m_Name == "Box001":
            scene_file = pathlib.Path(str(obj.assets_file.name)).name
            break
    if scene_file is None:
        raise SystemExit("Box001 is not in the bundle: is this the Garage build?")

    transforms, names = {}, {}
    for obj in env.objects:
        if pathlib.Path(str(obj.assets_file.name)).name != scene_file:
            continue
        if obj.type.name not in ("Transform", "GameObject"):
            continue
        try:
            data = obj.read()
        except Exception:
            continue
        if obj.type.name == "Transform":
            position, rotation, scale = (
                data.m_LocalPosition,
                data.m_LocalRotation,
                data.m_LocalScale,
            )
            transforms[obj.path_id] = dict(
                go=data.m_GameObject.path_id,
                position=[position.x, position.y, position.z],
                rotation=[rotation.x, rotation.y, rotation.z, rotation.w],
                scale=[scale.x, scale.y, scale.z],
                father=data.m_Father.path_id if data.m_Father else None,
            )
        else:
            names[obj.path_id] = data.m_Name

    def world(pid, seen=()):
        entry = transforms.get(pid)
        if entry is None or pid in seen:
            return None
        m = local_matrix(entry["position"], entry["rotation"], entry["scale"])
        father = entry["father"]
        if father and father in transforms:
            parent = world(father, seen + (pid,))
            if parent:
                return mul(parent, m)
        return m

    def path_of(pid):
        entry = transforms.get(pid)
        if not entry:
            return ""
        parts = [names.get(entry["go"], "")]
        p = entry["father"]
        while p and p in transforms:
            parts.append(names.get(transforms[p]["go"], ""))
            p = transforms[p]["father"]
        return "/".join(reversed(parts))

    pieces, car = [], None
    for pid, entry in transforms.items():
        name = names.get(entry["go"], "")
        path = path_of(pid)
        if name in ROOM and path.count("/") <= 1:
            position, rotation, scale = decompose(world(pid))
            pieces.append(
                dict(
                    file=ROOM[name],
                    name=name,
                    position=[round(v, 4) for v in position],
                    rotation=rotation,
                    scale=[round(v, 4) for v in scale],
                )
            )
        elif name == "Ford_F150_Raptor" and car is None:
            position, rotation, scale = decompose(world(pid))
            car = dict(
                position=[round(v, 4) for v in position],
                rotation=rotation,
                scale=[round(v, 4) for v in scale],
                path=path,
            )
    pieces.sort(key=lambda p: p["name"])
    layout = dict(scene=scene_file, pieces=pieces, car=car)
    (DST / "garage.json").write_text(json.dumps(layout, indent=1, sort_keys=True) + "\n")
    print(f"garage: {len(pieces)} pieces from {scene_file}, car display {car}")


if __name__ == "__main__":
    main()
