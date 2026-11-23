#!/usr/bin/env python3
"""
scripts/demo_payload_serveur.py -- un vrai echange avec le serveur CSTL,
capture tel quel dans deux fichiers (2026-11-23).

Lance un serveur CSTL temporaire (base jetable), puis :
  1. la source (agent `boutique`) s'engage sur le document : empreinte signee ;
  2. un autre agent (`assistant`) l'envoie, ecrit en arbre indente ;
  3. un transmetteur modifie un prix et essaie de l'envoyer au nom de la source ;
  4. l'assistant relit le document depuis le serveur.

Ecrit :
  <dossier>/1_payload.txt   ce que l'agent envoie : l'arbre indente, puis le
                            message CSTL signe reellement transmis
  <dossier>/2_serveur.txt   ce que le serveur repond a chaque etape, avec ce
                            qu'il a verifie

Usage : python3 scripts/demo_payload_serveur.py [dossier]   (cargo build --bin cstl_parser avant)
"""
import json
import os
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO / "sdk" / "python"))
BINARY = REPO / "target" / "debug" / "cstl_parser"
PORT = 5757

DOC = {"magasin": "Montreal", "commandes": [
    {"id": "c1", "client": "Alice", "date": "2026-11-20", "lignes": [
        {"article": "clavier", "qte": 2, "prix": 45},
        {"article": "souris", "qte": 1, "prix": 25, "options": [{"nom": "garantie", "prix": 10}]}]},
    {"id": "c2", "client": "Bob", "date": "2026-11-21", "note": "livrer avant 17h",
     "lignes": [{"article": "ecran", "qte": 1, "prix": 220}],
     "paiement": {"$unknown": {"raison": "en_attente_banque"}}}]}


def main():
    out = Path(sys.argv[1] if len(sys.argv) > 1 else ".")
    out.mkdir(parents=True, exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix="cstl_demo_"))
    os.environ["HOME"] = str(work)                       # cles des agents : jetables
    env = {**os.environ, "CSTL_PORT": str(PORT), "CSTL_REST_PORT": "8787"}
    proc = subprocess.Popen([str(BINARY)], cwd=work, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        for _ in range(120):
            try:
                socket.create_connection(("127.0.0.1", PORT), timeout=0.5).close()
                break
            except OSError:
                time.sleep(0.25)
        import copy
        import cstl_compact
        import cstl_llm_agent as m
        boutique = m.CstlAgent("boutique", "none", port=PORT)
        assistant = m.CstlAgent("assistant", "none", port=PORT)
        fraudeur = m.CstlAgent("fraudeur", "none", port=PORT)
        for a in (boutique, assistant, fraudeur):
            assert a.register()

        log = []

        def capture(agent, label):
            orig = agent.client.send_raw

            def send_raw(payload):
                resp = orig(payload)
                log.append((label, payload, resp.raw))
                return resp
            agent.client.send_raw = send_raw
            return orig

        text = cstl_compact.encode_tree_indented(DOC, counts="line")

        o = capture(boutique, "1. La source (boutique) s'engage : empreinte signee du document")
        boutique.commit_tree(DOC, "commandes_nov")
        boutique.client.send_raw = o

        o = capture(assistant, "2. L'assistant envoie le document, ecrit en arbre indente, au nom de la source")
        assistant.send_tree("server", text, tree_id="commandes_nov", source="boutique")
        assistant.client.send_raw = o

        fake = copy.deepcopy(DOC)
        fake["commandes"][1]["lignes"][0]["prix"] = 22
        o = capture(fraudeur, "3. Un fraudeur change le prix de l'ecran (220 -> 22) et l'envoie au nom de la source")
        fraudeur.send_tree("server", cstl_compact.encode_tree_indented(fake, counts="line"),
                           tree_id="commandes_nov", source="boutique")
        fraudeur.client.send_raw = o

        o = capture(assistant, "4. L'assistant relit le document depuis le serveur")
        back = assistant.fetch_tree("commandes_nov")
        assistant.client.send_raw = o

        (out / "1_payload.txt").write_text(
            "CE QUE L'AGENT ENVOIE\n=====================\n\n"
            "A. Le document, ecrit en arbre indente CSTL (ce qu'un modele lit ou ecrit) :\n\n"
            + text + "\n\n"
            "B. Le message CSTL reellement transmis au serveur (etape 2) : chaque element\n"
            "   devient une RELATION marquee tree=commandes_nov, avec tree_n (nombre\n"
            "   d'enfants), tree_types (types des valeurs) et, sur tree_root,\n"
            "   tree_source=boutique. L'ensemble est signe par la cle Ed25519 de\n"
            "   l'assistant (signature=...). La valeur inconnue (paiement) part en JSON\n"
            "   base64url, de type json : relue, elle redevient ?{raison:...}.\n\n"
            + log[1][1] + "\n", encoding="utf-8")

        lines = ["CE QUE LE SERVEUR FAIT", "======================", "",
                 "Echange reel avec le serveur CSTL (src/server/handler.rs), base jetable.", "",
                 "Note : dans un accuse de reception, receiver= n'est pas l'expediteur mais",
                 "l'agent que le serveur choisit par routage pondere par la confiance parmi",
                 "les agents enregistres (handler.rs, STEP 5) ; l'expediteur est dans",
                 "GOVERNANCE [sender=...].", ""]
        notes = {
            0: ["Le serveur verifie la signature de la boutique, puis inscrit l'engagement",
                "(RELATION type=tree_commit, object=sha256:...) dans la chaine d'audit signee."],
            1: ["Le serveur verifie, dans l'ordre :",
                "  - la signature de l'assistant (cle enregistree) ;",
                "  - la completude de l'arbre : chaque tree_n = enfants recus, aucun",
                "    element sans parent ni en double (src/server/tree_check.rs) ;",
                "  - l'empreinte : il recalcule le SHA-256 canonique de l'arbre recu et le",
                "    compare a celui que la boutique a signe : identiques ;",
                "  - les types declares (SCHEMA), la coherence, la deontique ;",
                "puis il stocke l'arbre et l'inscrit dans l'audit (AUDIT [hash, seq])."],
            2: ["Le prix change modifie l'empreinte : elle ne correspond plus a celle que la",
                "boutique a signee. Le message entier est REFUSE (purpose=tree_rejected),",
                "rien n'est stocke."],
            3: ["Requete signee purpose=tree_query. Le serveur rend les relations de",
                "l'arbre ; le SDK reverifie les comptes (tree_n) et reconstruit le document.",
                "Document relu identique a l'original : " + ("OUI" if back == DOC else "NON"),
                "Prix de l'ecran relu : " + str(back["commandes"][1]["lignes"][0]["prix"]),
                "Paiement relu : " + json.dumps(back["commandes"][1]["paiement"], ensure_ascii=False)]}
        for i, (label, _payload, raw) in enumerate(log):
            lines += [label, "-" * len(label), "Reponse du serveur :", "", raw.strip(), "", "Ce qu'il a fait :"]
            lines += notes.get(i, []) + ["", ""]
        (out / "2_serveur.txt").write_text("\n".join(lines), encoding="utf-8")
        print(f"Ecrit : {out / '1_payload.txt'} et {out / '2_serveur.txt'} ; relu identique : {back == DOC}")
    finally:
        proc.terminate()
        proc.wait(timeout=10)


if __name__ == "__main__":
    main()
