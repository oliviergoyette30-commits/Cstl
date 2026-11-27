#!/usr/bin/env python3
"""
scripts/real_data_build.py -- jeux de donnees REELS, hors distribution, pour
les bancs d'essai (2026-11-24).

Pourquoi : une critique du banc d'essai (Gemini, 2026-11-23) a releve que tous
les jeux etaient generes par nous, et que le format avait ete affine sur les
erreurs d'un modele. Ici : de vraies reponses d'API publiques, irregulieres,
profondes, jamais vues pendant la conception du format.

Sources (accessibles depuis l'environnement cloud) :
  PyPI      https://pypi.org/pypi/<paquet>/json
  npm       https://registry.npmjs.org/<paquet>/latest
  crates.io https://crates.io/api/v1/crates/<crate>
  Wikidata  https://www.wikidata.org/wiki/Special:EntityData/<Q>.json

Les reponses brutes font jusqu'a plusieurs Mo : on garde leur STRUCTURE reelle
(memes cles, memes imbrications, memes irregularites) mais on coupe les
listes et on choisit les champs, de facon deterministe, pour tenir dans
quelques milliers de caracteres. Le resultat est ecrit dans
scripts/real_data/*.json et versionne : le banc relit ces fichiers, pas l'API.

Usage : python3 scripts/real_data_build.py <dossier des reponses brutes>
        (sans argument : telecharge les reponses dans un dossier temporaire)
"""
import json
import sys
import tempfile
import urllib.request
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
OUT = REPO / "scripts" / "real_data"

SOURCES = {
    "pypi_requests": "https://pypi.org/pypi/requests/json",
    "pypi_flask": "https://pypi.org/pypi/flask/json",
    "pypi_django": "https://pypi.org/pypi/django/json",
    "npm_express": "https://registry.npmjs.org/express/latest",
    "npm_react": "https://registry.npmjs.org/react/latest",
    "npm_axios": "https://registry.npmjs.org/axios/latest",
    "npm_lodash": "https://registry.npmjs.org/lodash/latest",
    "crates_serde": "https://crates.io/api/v1/crates/serde",
    "crates_tokio": "https://crates.io/api/v1/crates/tokio",
    "crates_rand": "https://crates.io/api/v1/crates/rand",
    "wd_paris": "https://www.wikidata.org/wiki/Special:EntityData/Q90.json",
    "wd_adams": "https://www.wikidata.org/wiki/Special:EntityData/Q42.json",
}


def short(v, n=120):
    """Texte long coupe (descriptions, readme) : la structure compte, pas la prose."""
    return v[:n] if isinstance(v, str) else v


def pypi(d):
    info = d["info"]
    keep = ("name", "version", "summary", "author", "license", "requires_python", "home_page", "yanked",
            "requires_dist", "project_urls", "classifiers")
    out = {"info": {k: info.get(k) for k in keep}}
    out["info"]["requires_dist"] = (info.get("requires_dist") or [])[:8]
    out["info"]["classifiers"] = (info.get("classifiers") or [])[:8]
    files = ("filename", "packagetype", "python_version", "requires_python", "size", "upload_time", "yanked",
             "yanked_reason", "digests")
    out["urls"] = [{k: f.get(k) for k in files} for f in d.get("urls", [])[:4]]
    versions = sorted(d.get("releases", {}), key=lambda v: d["releases"][v][0]["upload_time"]
                      if d["releases"][v] else "")[-4:]
    out["releases"] = {v: [{k: f.get(k) for k in ("filename", "packagetype", "size", "yanked")}
                           for f in d["releases"][v][:2]] for v in versions}
    out["vulnerabilities"] = [{k: short(x.get(k)) for k in ("id", "aliases", "fixed_in", "withdrawn")}
                              for x in d.get("vulnerabilities", [])[:3]]
    return out


def npm(d):
    keep = ("name", "version", "description", "license", "main", "engines", "dependencies", "devDependencies",
            "repository", "maintainers", "files", "scripts", "keywords", "dist")
    out = {k: d[k] for k in keep if k in d}
    for k in ("devDependencies", "dependencies", "scripts"):
        if isinstance(out.get(k), dict):
            out[k] = dict(list(out[k].items())[:10])
    if "dist" in out:
        out["dist"] = {k: v for k, v in out["dist"].items() if k in ("shasum", "tarball", "fileCount", "unpackedSize")}
    out["description"] = short(out.get("description"))
    return out


def crates(d):
    c = d["crate"]
    out = {"crate": {k: c.get(k) for k in ("name", "description", "downloads", "recent_downloads", "max_version",
                                           "max_stable_version", "repository", "num_versions", "yanked")},
           "keywords": [{k: x.get(k) for k in ("id", "keyword", "crates_cnt")} for x in d.get("keywords", [])[:5]],
           "categories": [{k: x.get(k) for k in ("id", "category", "crates_cnt")} for x in d.get("categories", [])[:5]]}
    out["crate"]["description"] = short(out["crate"]["description"])
    out["versions"] = []
    for v in d.get("versions", [])[:6]:
        item = {k: v.get(k) for k in ("num", "created_at", "downloads", "yanked", "yank_message", "license",
                                      "crate_size", "rust_version", "features")}
        if v.get("published_by"):
            item["published_by"] = {k: v["published_by"].get(k) for k in ("login", "name")}
        out["versions"].append(item)
    return out


def wikidata(d):
    (qid, e), = d["entities"].items()
    out = {"id": qid, "type": e.get("type"),
           "labels": {lang: e["labels"][lang]["value"] for lang in ("en", "fr", "de", "es") if lang in e["labels"]},
           "claims": {}}
    for prop in ("P31", "P17", "P1082", "P625", "P19", "P106", "P569", "P800"):
        statements = e.get("claims", {}).get(prop)
        if not statements:
            continue
        kept = []
        for st in statements[:3]:
            snak = st["mainsnak"]
            item = {"rank": st.get("rank"), "snaktype": snak.get("snaktype"),
                    "datatype": snak.get("datatype"), "value": snak.get("datavalue", {}).get("value")}
            q = st.get("qualifiers", {})
            if q:
                item["qualifiers"] = {p: [s.get("datavalue", {}).get("value") for s in v[:2]]
                                      for p, v in list(q.items())[:2]}
            refs = st.get("references", [])
            if refs:
                item["references"] = len(refs)
            kept.append(item)
        out["claims"][prop] = kept
    if len(out["claims"]) > 4:
        out["claims"] = dict(list(out["claims"].items())[:4])
    return out


TRIM = {"pypi": pypi, "npm": npm, "crates": crates, "wd": wikidata}


def main():
    raw = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(tempfile.mkdtemp(prefix="cstl_real_"))
    OUT.mkdir(exist_ok=True)
    for name, url in SOURCES.items():
        src = raw / f"{name}.json"
        if not src.exists():
            req = urllib.request.Request(url, headers={"User-Agent": "cstl-bench/1.0"})
            src.write_bytes(urllib.request.urlopen(req, timeout=60).read())
        doc = TRIM[name.split("_")[0]](json.loads(src.read_text()))
        (OUT / f"{name}.json").write_text(json.dumps(doc, indent=1, ensure_ascii=False) + "\n")
        print(f"{name:15} {len(json.dumps(doc, separators=(',', ':'))):>6} caracteres  <- {url}")


if __name__ == "__main__":
    main()
