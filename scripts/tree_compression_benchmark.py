#!/usr/bin/env python3
"""
scripts/tree_compression_benchmark.py -- l'arbre indente CSTL face au JSON,
APRES compression (2026-11-27, question d'Olivier : « on n'a jamais essaye la
compression avec l'arbre indente »).

Le banc `compression_benchmark.py` comparait le FORMAT DU FIL (messages du
serveur) au JSON : egalite une fois compresses (2 070 contre 2 076 octets).
Ici, ce sont les donnees elles-memes, dans les formes que lit un modele :
  json         JSON compact (separateurs sans espaces)
  json_indent  JSON indente (2 espaces), ce qu'on envoie souvent a un modele
  toon         TOON (encodeur fidele, quand il sait representer le document)
  arbre        arbre indente CSTL (`encode_tree_indented`)
  arbre_n      arbre indente avec comptes locaux `/N`
  strict       forme strict (`encode_objects_json`)

Compresseurs : aucun, gzip -9, zstd -19, brotli -11, et zstd -19 avec un
dictionnaire ENTRAINE sur d'autres documents du meme genre et du meme format
(jamais sur le document mesure). Tout est verifie : chaque forme se relit
identique au JSON d'origine, et chaque compression se decompresse a
l'identique.

Jeux :
  - imbrique : les 3 genres de `nested_benchmark.py` (commandes, organisation,
    contrats), tailles 8 et 24, graines 1 et 2 ; dictionnaire entraine sur 40
    autres documents du meme genre (graines 100 a 139) ;
  - reel : les 12 reponses d'API de `scripts/real_data/` ; dictionnaire
    entraine sur les 11 AUTRES (une seule source par document mesure).

  python3 scripts/tree_compression_benchmark.py      # tableau + tree_compression_results.json
"""
import gzip
import json
import sys
from pathlib import Path

import brotli
import zstandard

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO / "scripts"))
sys.path.insert(0, str(REPO / "sdk" / "python"))
import cstl_compact  # noqa: E402
import nested_benchmark as nb  # noqa: E402
import toon_lite  # noqa: E402

FORMATS = ("json", "json_indent", "toon", "arbre", "arbre_n", "strict")
COMPRESSORS = ("brut", "gzip", "zstd", "brotli", "zstd_dict")
DICT_SIZE = 4096


def render(fmt, d):
    if fmt == "json":
        return json.dumps(d, ensure_ascii=False, separators=(",", ":"))
    if fmt == "json_indent":
        return json.dumps(d, ensure_ascii=False, indent=2)
    if fmt == "toon":
        return toon_lite.encode_nested(d)
    if fmt == "arbre":
        return cstl_compact.encode_tree_indented(d)
    if fmt == "arbre_n":
        return cstl_compact.encode_tree_indented(d, counts="line")
    return cstl_compact.encode_objects_json(d)


def decode(fmt, text):
    if fmt.startswith("json"):
        return json.loads(text)
    if fmt == "toon":
        return toon_lite.decode_nested(text)
    if fmt.startswith("arbre"):
        return cstl_compact.decode_tree_indented(text)
    return cstl_compact.decode_objects_json(text)


def representable(fmt, d):
    try:
        return decode(fmt, render(fmt, d)) == d
    except Exception:  # noqa: BLE001 -- TOON ne sait pas tout representer
        return False


def train(samples):
    try:
        return zstandard.train_dictionary(DICT_SIZE, samples)
    except zstandard.ZstdError:
        return None


def sizes(raw, zdict):
    out = {"brut": len(raw)}
    g = gzip.compress(raw, 9, mtime=0)
    assert gzip.decompress(g) == raw
    out["gzip"] = len(g)
    z = zstandard.ZstdCompressor(level=19).compress(raw)
    assert zstandard.ZstdDecompressor().decompress(z) == raw
    out["zstd"] = len(z)
    b = brotli.compress(raw, quality=11)
    assert brotli.decompress(b) == raw
    out["brotli"] = len(b)
    if zdict is not None:
        zd = zstandard.ZstdCompressor(level=19, dict_data=zdict).compress(raw)
        assert zstandard.ZstdDecompressor(dict_data=zdict).decompress(zd) == raw
        out["zstd_dict"] = len(zd)
    return out


def nested_jobs():
    for kind in nb.KINDS:
        training = [nb.gen(kind, n, s) for n in nb.SIZES for s in range(100, 120)]
        for n in nb.SIZES:
            for seed in nb.SEEDS:
                yield "imbrique", f"{kind}_{n}_{seed}", nb.gen(kind, n, seed), training


def real_jobs():
    docs = {p.stem: json.loads(p.read_text()) for p in sorted((REPO / "scripts" / "real_data").glob("*.json"))}
    for name, d in docs.items():
        yield "reel", name, d, [o for k, o in docs.items() if k != name]


def main():
    rows = []
    for group, name, doc, training in [*nested_jobs(), *real_jobs()]:
        for fmt in FORMATS:
            if not representable(fmt, doc):
                continue
            samples = [render(fmt, t).encode() for t in training if representable(fmt, t)]
            zdict = train(samples) if len(samples) >= 8 else None
            rows.append({"group": group, "doc": name, "format": fmt,
                         **sizes(render(fmt, doc).encode(), zdict)})
    out = REPO / "scripts" / "tree_compression_results.json"
    out.write_text(json.dumps(rows, indent=1, ensure_ascii=False))

    for group in ("imbrique", "reel"):
        docs = sorted({r["doc"] for r in rows if r["group"] == group})
        # Comparaison sur les documents que TOUS les formats representent.
        common = [d for d in docs if all(any(r["doc"] == d and r["format"] == f for r in rows) for f in FORMATS)]
        common_no_toon = [d for d in docs if all(any(r["doc"] == d and r["format"] == f for r in rows)
                                                 for f in FORMATS if f != "toon")]
        for label, subset, fmts in (("tous formats", common, FORMATS),
                                    ("sans TOON", common_no_toon, [f for f in FORMATS if f != "toon"])):
            if not subset:
                continue
            print(f"\n{group} -- {label} -- {len(subset)} documents (octets totaux)")
            print(f"  {'format':12}" + "".join(f"{c:>11}" for c in COMPRESSORS))
            base = {}
            for fmt in fmts:
                sel = [r for r in rows if r["group"] == group and r["format"] == fmt and r["doc"] in subset]
                tot = {c: sum(r.get(c, 0) for r in sel) if all(c in r for r in sel) else None for c in COMPRESSORS}
                if fmt == "json":
                    base = tot
                print(f"  {fmt:12}" + "".join(f"{(tot[c] if tot[c] is not None else '-'):>11}" for c in COMPRESSORS))
            print(f"  {'vs json':12}" + "".join(
                f"{'':>11}" for _ in COMPRESSORS))
            for fmt in fmts:
                if fmt == "json":
                    continue
                sel = [r for r in rows if r["group"] == group and r["format"] == fmt and r["doc"] in subset]
                cells = []
                for c in COMPRESSORS:
                    if base.get(c) and all(c in r for r in sel):
                        cells.append(f"{100 * (sum(r[c] for r in sel) / base[c] - 1):+.0f} %")
                    else:
                        cells.append("-")
                print(f"  {fmt:12}" + "".join(f"{x:>11}" for x in cells))
    print(f"\nResultats : {out.relative_to(REPO)}")


if __name__ == "__main__":
    main()
