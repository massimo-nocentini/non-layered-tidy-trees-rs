"""Differential check of `space::layout3` against the exact-arithmetic prototype.

    python3 prototype/xcheck.py [trees]

Draws random trees with the prototype's generator and lays each one out twice:

* under its height-parity rule, writing the axis the prototype picked for every family to
  a file that `examples/space_xcheck.rs` lays out again with `layout3`;
* under its `trial` rule scored by the longer side of the footprint, with no axes written,
  for `layout3` to choose them itself under `AxisRule::Compact`.

The two must agree to within 1e-9 on every coordinate. The first tree is the one of the
60-node picture.
"""
import os, random, subprocess, sys, tempfile

sys.setrecursionlimit(100000)
import proto

HERE = os.path.dirname(os.path.abspath(__file__))


def cases(count):
    yield proto.randtree(60, random.Random(6), maxk=4, sizes=(2, 6), deep=0.0)
    for seed in range(count - 1):
        rng = random.Random(seed)
        n = rng.randint(2, 60)
        yield proto.randtree(n, rng, maxk=rng.randint(2, 5), sizes=(1, 6),
                             margins=seed % 2 == 1, deep=rng.random())


def longest(S):
    b = S['bbox']
    return max(b[1] - b[0], b[3] - b[2])


def compare(count, rule, compact):
    trees, expected = [], []
    for t in cases(count):
        if compact:
            subs, axes = proto.layout(t, rule='trial', score=longest)
        else:
            subs, axes = proto.layout(t, rule=rule)
        n, pos = len(t.w), subs[0]['pos']
        parent = [-1] * n
        for v in range(n):
            for c in t.ch[v]:
                parent[c] = v
        axis = (lambda v: -1) if compact else (lambda v: axes.get(v, 0))
        trees.append(f"tree {n}\n" + "".join(
            f"{t.w[v]} {t.h[v]} {t.d[v]} {t.m[v]} {parent[v]} {axis(v)}\n" for v in range(n)))
        mx = min(pos[v][0] for v in range(n))
        mz = min(pos[v][1] for v in range(n))
        expected.append([c for v in range(n)
                         for c in (float(pos[v][0] - mx), float(t.top[v]), float(pos[v][1] - mz))])

    with tempfile.NamedTemporaryFile('w', suffix='.txt', delete=False) as f:
        f.write("".join(trees))
    try:
        cmd = ["cargo", "run", "--quiet", "--release", "--example", "space_xcheck", "--", f.name]
        if compact:
            cmd.append("compact")
        out = subprocess.run(cmd, cwd=os.path.dirname(HERE), check=True,
                             capture_output=True, text=True).stdout
    finally:
        os.unlink(f.name)

    got = [list(map(float, line.split())) for line in out.splitlines()]
    assert len(got) == len(expected), f"{len(got)} layouts for {len(expected)} trees"

    bad = [i for i, (e, g) in enumerate(zip(expected, got))
           if len(e) != len(g) or max(abs(a - b) for a, b in zip(e, g)) > 1e-9]
    exact = sum(e == g for e, g in zip(expected, got))
    name = "Compact against trial" if compact else "axes given"
    print(f"{name}: {len(expected)} trees, {len(expected) - len(bad)} agree within 1e-9, "
          f"{exact} bit for bit")
    if bad:
        print("disagreeing trees:", bad[:20])
    return not bad


def main():
    count = int(sys.argv[1]) if len(sys.argv) > 1 else 2000
    ok = compare(count, 'height', False)
    ok = compare(count, 'trial', True) and ok
    sys.exit(0 if ok else 1)


if __name__ == '__main__':
    main()
