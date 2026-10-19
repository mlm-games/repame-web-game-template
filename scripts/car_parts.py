#!/usr/bin/env python3
"""Write every car part's authored transform, relative to its car.

The export keeps each part as its own mesh but not the transform its GameObject
carries. Most bodies are authored in car space, so the loader placed every part
at the car's origin — which is why the rims of a car without wheel meshes flew
out of its body, and why spoilers sat in the middle of the car. This walks the
Garage scene (which holds all ten cars at the origin), composes each part's
transform against its car root and writes:

    assets/dd1/car_parts.json   "<car id>/<path under the car>" -> transform

    scripts/car_parts.py [data.unity3d] [assets/dd1]
"""
import json
import pathlib
import sys

from scene_xform import decompose, inverse, local_matrix, mul

BUNDLE = pathlib.Path(
    sys.argv[1] if len(sys.argv) > 1 else "/tmp/opencode/dd/data/data.unity3d"
)
DST = pathlib.Path(sys.argv[2] if len(sys.argv) > 2 else "assets/dd1")


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
        raise SystemExit("the Garage scene is not in the bundle")

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

    car_ids = {entry["id"] for entry in json.loads((DST / "cars.json").read_text())}
    roots = {}
    for pid, entry in transforms.items():
        path = path_of(pid)
        parts = path.split("/")
        if len(parts) == 2 and parts[0] == "Cars" and parts[1] in car_ids:
            roots[parts[1]] = world(pid)

    parts = {}
    for pid, entry in transforms.items():
        path = path_of(pid)
        pieces = path.split("/")
        if len(pieces) < 3 or pieces[0] != "Cars" or pieces[1] not in roots:
            continue
        car = pieces[1]
        relative = "/".join(pieces[2:])
        position, rotation, scale = decompose(mul(inverse(roots[car]), world(pid)))
        parts[f"{car}/{relative}"] = dict(
            position=position, rotation=rotation, scale=scale
        )

    cars = sorted({key.split("/")[0] for key in parts})
    (DST / "car_parts.json").write_text(
        json.dumps(parts, indent=1, sort_keys=True) + "\n"
    )
    print(f"car parts: {len(parts)} across {len(cars)} cars ({', '.join(cars)})")


if __name__ == "__main__":
    main()
