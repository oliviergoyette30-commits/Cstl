#!/usr/bin/env python3
"""
scripts/compression_benchmark.py -- la chaine de compression CSTL COMPLETE
contre un JSON compresse au mieux, sur de vrais messages (2026-11-21).

Pourquoi : `wire_size_benchmark.py` ne mesurait que gzip. CSTL a sa propre
architecture de compression :
  - le Master Compresseur (`src/compression/master.rs`) : il decoupe les
    relations en flux (opcodes, texte, identifiants, nombres) et garde le
    plus petit de trois candidats : deux dictionnaires reseau fixes
    pre-remplis avec delta zigzag et varint, gzip structurel, ou zstd avec un
    dictionnaire embarque dans le binaire ;
  - la compression des blocs (`src/compression/response.rs`) : dictionnaire
    de texte pre-entraine et deltas, sur tous les blocs ;
  - la compression des reponses sur le fil (`maybe_compress_response`).

Pour que la comparaison soit juste, le JSON a droit aux MEMES armes :
gzip -9, zstd -19, brotli -11 (qui a son propre dictionnaire integre), et
zstd -19 avec un dictionnaire ENTRAINE sur des messages JSON du meme genre.
Le texte CSTL a droit aux memes compresseurs generiques, et a un
dictionnaire zstd entraine sur des messages CSTL.

LES MESSAGES : 16 vrais messages captures sur le vrai serveur (8 requetes
et 8 reponses : enregistrement signe, politique a conditions, faits,
politique effective, autorisation avec jeton signe, refus, 20 relations).
Les dictionnaires sont entraines sur 8 AUTRES captures du meme scenario
(autres noms, autres cles, autres montants), jamais sur les messages
mesures.

RESULTAT (2026-11-21, 16 vrais messages, octets au total) :
                                                  CSTL      JSON
  brut                                           13 046    14 010
  gzip -9                                         6 517     6 325
  zstd -19                                        6 404     6 343
  brotli -11                                      5 502     5 383
  zstd -19 + dictionnaire entraine (8 Ko)         2 070     2 076
  CSTL : blocs, dictionnaire texte + deltas      10 458       -
  CSTL : Master hybride                          10 067       -
  CSTL : reponse compressee du serveur (base64)  12 319       -
- Avec la meilleure methode de chaque cote, CSTL et JSON font la meme
  taille (2 070 contre 2 076 octets, -0,3 %). La methode gagnante est la
  meme pour les deux : zstd avec un dictionnaire entraine.
- Les compresseurs maison de CSTL (dictionnaires fixes, deltas, Master
  hybride) font beaucoup moins bien que zstd + dictionnaire : environ
  10 000 octets contre 2 070. Le dictionnaire de texte code chaque symbole
  sans exploiter les repetitions entre champs, et il ne couvre que les
  relations (Master) ou un vocabulaire general (blocs). La reponse
  compressee du serveur gagne a peine 6 %, a cause du base64.
- Les dictionnaires sont entraines sur 8 autres captures du MEME scenario :
  c'est un cas tres favorable, mais il l'est autant pour les deux formats.
- Defaut trouve : `master_decompress` ajoute `object=""` a une relation
  qui n'avait pas de champ `object` (ex. RELATION [type=received,
  subject=inform, status=valid]). 2 messages sur 16 ne repassent donc pas
  a l'identique par le Master ; leurs tailles restent valables.

Necessite : cargo build --release --example full_compression_sizes,
pip install zstandard brotli.

Usage : python3 scripts/compression_benchmark.py
"""
import gzip
import json
import subprocess
import sys
import tempfile
from pathlib import Path

import brotli
import zstandard

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO / "scripts"))
import wire_size_benchmark as ws  # noqa: E402

DICT_SIZE = 8192   # meme taille que le dictionnaire embarque de CSTL (zstd_cstl_dict.bin)


def compact(wire):
    return json.dumps(ws.to_json(wire), ensure_ascii=False, separators=(",", ":"))


def generic(data, zdict=None):
    out = {"gzip": len(gzip.compress(data, compresslevel=9, mtime=0)),
           "zstd": len(zstandard.ZstdCompressor(level=19).compress(data)),
           "brotli": len(brotli.compress(data, quality=11))}
    if zdict is not None:
        c = zstandard.ZstdCompressor(level=19, dict_data=zdict, write_content_size=False, write_checksum=False,
                                     write_dict_id=False)
        blob = c.compress(data)
        d = zstandard.ZstdDecompressor(dict_data=zdict).decompressobj()
        assert d.decompress(blob) == data
        out["zstd_dict"] = len(blob)
    return out


def main():
    train = []
    for i in range(8):
        train += [w for _, w in ws.capture(tag=f"train{i}", seed=i + 1)]
    test = ws.capture(tag="size", seed=0)
    cstl_dict = zstandard.train_dictionary(DICT_SIZE, [w.encode() for w in train])
    json_dict = zstandard.train_dictionary(DICT_SIZE, [compact(w).encode() for w in train])

    with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as f:
        json.dump([{"name": n, "wire": w} for n, w in test], f)
    rust = subprocess.run(["cargo", "run", "--quiet", "--release", "--example", "full_compression_sizes", "--", f.name],
                          cwd=REPO, capture_output=True, text=True, check=True)
    native = json.loads(rust.stdout.strip().splitlines()[-1])

    rows = []
    for (name, wire), nat in zip(test, native):
        assert nat["name"] == name
        c, j = wire.encode(), compact(wire).encode()
        assert ws.same(wire, ws.to_json(wire))
        rows.append({"message": name,
                     "cstl": {"brut": len(c), **generic(c, cstl_dict),
                              "blocs_dico": nat["blocks"], "master_hybride": nat["master"],
                              "fil_serveur": nat["wire_compressed"]},
                     "json": {"brut": len(j), **generic(j, json_dict)},
                     "roundtrip_ok": nat["blocks_ok"] and nat["master_ok"] and nat["lossless_text"]})

    def total(fmt, key):
        return sum(r[fmt][key] for r in rows)

    def best(r, fmt, keys):
        return min(r[fmt][k] for k in keys)

    cstl_keys = ["brut", "gzip", "zstd", "brotli", "zstd_dict", "blocs_dico", "master_hybride", "fil_serveur"]
    json_keys = ["brut", "gzip", "zstd", "brotli", "zstd_dict"]
    print(f"16 vrais messages, {sum(r['roundtrip_ok'] for r in rows)}/16 decompressions verifiees a l'identique\n")
    print(f"  {'methode':48} | {'CSTL':>7} | {'JSON':>7}")
    labels = {"brut": "brut (sans compression)", "gzip": "gzip -9", "zstd": "zstd -19", "brotli": "brotli -11",
              "zstd_dict": "zstd -19 + dictionnaire entraine (8 Ko)",
              "blocs_dico": "CSTL : blocs, dictionnaire texte + deltas",
              "master_hybride": "CSTL : Master hybride (dicos fixes/gzip/zstd)",
              "fil_serveur": "CSTL : reponse compressee du serveur (base64)"}
    for k in cstl_keys:
        jv = f"{total('json', k):>7}" if k in json_keys else f"{'-':>7}"
        print(f"  {labels[k]:48} | {total('cstl', k):>7} | {jv}")
    best_c = sum(best(r, "cstl", cstl_keys) for r in rows)
    best_j = sum(best(r, "json", json_keys) for r in rows)
    print(f"  {'MEILLEURE methode par message':48} | {best_c:>7} | {best_j:>7}")
    print(f"\n  Meilleur CSTL contre meilleur JSON : {100 * (best_c - best_j) / best_j:+.1f} %")
    winners = {}
    for r in rows:
        k = min(cstl_keys, key=lambda k: r["cstl"][k])
        winners[k] = winners.get(k, 0) + 1
    print(f"  Methode CSTL gagnante par message : {winners}")
    print(f"\n  {'message':52} | {'CSTL meilleur':>13} | {'JSON meilleur':>13}")
    for r in rows:
        print(f"  {r['message'][:52]:52} | {best(r, 'cstl', cstl_keys):>13} | {best(r, 'json', json_keys):>13}")
    dest = REPO / "scripts" / "compression_benchmark.json"
    dest.write_text(json.dumps({"rows": rows, "best_cstl": best_c, "best_json": best_j}, indent=2, ensure_ascii=False) + "\n")
    print(f"\n  Resultats bruts : {dest.relative_to(REPO)}")


if __name__ == "__main__":
    main()
