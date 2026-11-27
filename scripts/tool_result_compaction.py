#!/usr/bin/env python3
"""
scripts/tool_result_compaction.py -- combien `cstl mcp-proxy --compact-results`
economise sur des resultats d'outils (2026-11-27).

Les documents : les 12 vraies reponses d'API de scripts/real_data/ (PyPI, npm,
crates.io, Wikidata), et les 12 jeux imbriques de nested_benchmark.py. Chaque
document est presente comme un serveur MCP le renverrait : en JSON compact, ou
en JSON indente (2 espaces, frequent). On passe le resultat dans la vraie
fonction du proxy (`compact_result`), legende comprise, et on compte les
caracteres. Les tokens ont ete mesures a part, par les tokenizers des
fournisseurs (ROADMAP : -39 % sur l'imbrique, -8 a -13 % sur le reel riche en
objets) ; ici, aucun tokenizer n'est disponible hors ligne.

  python3 scripts/tool_result_compaction.py
"""
import json
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO / "scripts"))
sys.path.insert(0, str(REPO / "sdk" / "python"))
import cstl_mcp_proxy as proxy  # noqa: E402
import nested_benchmark as nb  # noqa: E402


def docs():
    for p in sorted((REPO / "scripts" / "real_data").glob("*.json")):
        yield "reel", p.stem, json.loads(p.read_text())
    for kind in nb.KINDS:
        for n in nb.SIZES:
            for seed in nb.SEEDS:
                yield "imbrique", f"{kind}_{n}_{seed}", nb.gen(kind, n, seed)


def main():
    rows = []
    for group, name, d in docs():
        for style, text in (("compact", json.dumps(d, ensure_ascii=False, separators=(",", ":"))),
                            ("indente", json.dumps(d, ensure_ascii=False, indent=2))):
            res, saved = proxy.compact_result({"content": [{"type": "text", "text": text}]})
            rows.append({"group": group, "doc": name, "style": style, "before": len(text),
                         "after": len(res["content"][0]["text"]), "converted": saved > 0})
    out = REPO / "scripts" / "tool_result_compaction.json"
    out.write_text(json.dumps(rows, indent=1, ensure_ascii=False) + "\n")
    print(f"{'jeu':10} {'JSON de depart':16} {'convertis':>10} {'avant':>9} {'apres':>9} {'gain':>7}")
    for group in ("reel", "imbrique"):
        for style in ("compact", "indente"):
            sel = [r for r in rows if r["group"] == group and r["style"] == style]
            b, a = sum(r["before"] for r in sel), sum(r["after"] for r in sel)
            conv = sum(r["converted"] for r in sel)
            print(f"{group:10} {style:16} {f'{conv}/{len(sel)}':>10} {b:>9} {a:>9} {100 * (a / b - 1):>6.0f} %")
    print(f"\nResultats : {out.relative_to(REPO)}")


if __name__ == "__main__":
    main()
