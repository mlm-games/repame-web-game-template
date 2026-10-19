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
import math
import pathlib
import sys

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


def decompose(m):
    """Position, quaternion and scale out of a 4x4 row-major matrix."""
    pos = [m[0][3], m[1][3], m[2][3]]
    cols = [[m[0][i], m[1][i], m[2][i]] for i in range(3)]
    scale = [math.sqrt(sum(c * c for c in col)) for col in cols]
    rot = [[col[i] / (scale[i] or 1.0) for i in range(3)] for col in cols]
    tr = rot[0][0] + rot[1][1] + rot[2][2]
    if tr > 0:
        s = math.sqrt(tr + 1.0) * 2
        q = [
            (rot[2][1] - rot[1][2]) / s,
            (rot[0][2] - rot[2][0]) / s,
            (rot[1][0] - rot[0][1]) / s,
            0.25 * s,
        ]
    elif rot[0][0] > rot[1][1] and rot[0][0] > rot[2][2]:
        s = math.sqrt(1.0 + rot[0][0] - rot[1][1] - rot[2][2]) * 2
        q = [
            0.25 * s,
            (rot[0][1] + rot[1][0]) / s,
            (rot[0][2] + rot[2][0]) / s,
            (rot[2][1] - rot[1][2]) / s,
        ]
    elif rot[1][1] > rot[2][2]:
        s = math.sqrt(1.0 + rot[1][1] - rot[0][0] - rot[2][2]) * 2
        q = [
            (rot[0][1] + rot[1][0]) / s,
            0.25 * s,
            (rot[1][2] + rot[2][1]) / s,
            (rot[0][2] - rot[2][0]) / s,
        ]
    else:
        s = math.sqrt(1.0 + rot[2][2] - rot[0][0] - rot[1][1]) * 2
        q = [
            (rot[0][2] + rot[2][0]) / s,
            (rot[1][2] + rot[2][1]) / s,
            0.25 * s,
            (rot[1][0] - rot[0][1]) / s,
        ]
    return pos, [round(v, 6) for v in q], [round(v, 6) for v in scale]


def mul(a, b):
    return [
        [sum(a[r][k] * b[k][c] for k in range(4)) for c in range(4)]
        for r in range(4)
    ]


def local_matrix(position, rotation, scale):
    # Unity brains quaternions left-handed: the same numbers rotate the other way
    # round in the right-handed engine, so every scene quaternion is inverted
    # here. Without this the garage's floor faces down and its walls hang in the
    # air over the car.
    x, y, z, w = (-rotation[0], -rotation[1], -rotation[2], rotation[3])
    xx, yy, zz = x * x, y * y, z * z
    xy, xz, yz = x * y, x * z, y * z
    wx, wy, wz = w * x, w * y, w * z
    m = [
        [1 - 2 * (yy + zz), 2 * (xy - wz), 2 * (xz + wy), position[0]],
        [2 * (xy + wz), 1 - 2 * (xx + zz), 2 * (yz - wx), position[1]],
        [2 * (xz - wy), 2 * (yz + wx), 1 - 2 * (xx + yy), position[2]],
        [0.0, 0.0, 0.0, 1.0],
    ]
    for row in range(3):
        m[row][0] *= scale[0]
        m[row][1] *= scale[1]
        m[row][2] *= scale[2]
    return m


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
