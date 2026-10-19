"""Matrix helpers for reading Unity scene transforms out of the data bundle.

Scene quaternions are left-handed: the same numbers rotate the other way round in
the right-handed engine, so every quaternion is inverted here. Without that the
garage floor faces down and each car part lands on the wrong side of its pivot.
"""
import math


def local_matrix(position, rotation, scale):
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


def mul(a, b):
    return [
        [sum(a[r][k] * b[k][c] for k in range(4)) for c in range(4)]
        for r in range(4)
    ]


def inverse(m):
    """Rigid-body inverse (rotation and translation only)."""
    r = [[m[c][r] for c in range(3)] for r in range(3)]
    t = [m[0][3], m[1][3], m[2][3]]
    out = [[0.0] * 4 for _ in range(4)]
    for row in range(3):
        for col in range(3):
            out[row][col] = r[row][col]
        out[row][3] = -sum(r[row][k] * t[k] for k in range(3))
    out[3][3] = 1.0
    return out


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
    return (
        [round(v, 4) for v in pos],
        [round(v, 6) for v in q],
        [round(v, 4) for v in scale],
    )
