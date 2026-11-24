"""Tests des formes specialisees (cstl_formes.py, 2026-11-24) : chacune se
relit a l'identique, refuse ce qu'elle ne sait pas representer, et rejoint le
CSTL polymorphe."""
import json
import random
import subprocess
import sys
from pathlib import Path

import pytest

import cstl_formes as F
from cstl_compact import CompactError, choose_format, data_kind, decode_auto, encode_auto

SERIE = {"capteur": "s1", "mesures": [{"t": f"2026-11-24T10:{i:02d}:00Z", "temp": 21.5 + i, "etat": "a b"}
                                     for i in range(5)]}
GRAPHE = {"directed": True, "nodes": [{"id": "n1", "label": "Alice"}, {"id": "n2", "label": "Bob", "age": 3}],
          "edges": [{"source": "n1", "target": "n2", "type": "knows"}, {"source": "n2", "target": "n1", "type": "x"}]}
CONV = [{"role": "system", "content": "Tu es un agent.\n# pas un titre"}, {"role": "user", "content": ""},
        {"role": "assistant", "content": None, "tool_calls": [{"id": "c1"}]}, {"role": "tool", "content": "\\ok\n"},
        {"role": "assistant"}]
APPELS = [{"name": "rembourser", "arguments": {"commande": "o1", "montant": 80, "motif": "colis, abime"}},
          {"name": "g", "arguments": {"a b": None}, "result": {"ok": True}}]
MATRICE = [[1, 2], [3, 4], [5, 6]]


@pytest.mark.parametrize("form,data", [("serie", SERIE), ("graphe", GRAPHE), ("conversation", CONV),
                                       ("appels", APPELS), ("appels", APPELS[0]), ("matrice", MATRICE),
                                       ("matrice", [1, 2.5, -3])])
def test_each_form_round_trips(form, data):
    assert F.DECODERS[form](F.ENCODERS[form](data)) == data


def test_readable_shapes():
    assert F.encode_appels(APPELS[0]) == '@appel\nrembourser(commande="o1", montant=80, motif="colis, abime")'
    assert F.encode_conversation(CONV).splitlines()[:4] == ["@conversation", "#system", "Tu es un agent.",
                                                            "\\# pas un titre"]
    assert F.encode_serie(SERIE).splitlines()[1].startswith("@serie mesures[5] t de=2026-11-24T10:00:00Z pas=60")


def test_forms_refuse_what_they_cannot_represent():
    for form, bad in (("serie", [{"t": 1}, {"t": 2}, {"t": 4}]), ("graphe", {"nodes": []}),
                      ("conversation", [{"content": "x"}]), ("appels", {"nom": "f"}), ("matrice", [[1], [2, 3]])):
        with pytest.raises(CompactError):
            F.ENCODERS[form](bad)
    with pytest.raises(CompactError):
        F.decode_appels("@appel\nf(a=1")


def test_kinds_and_polymorphic_round_trip():
    assert [data_kind(d) for d in (SERIE, GRAPHE, CONV, APPELS, MATRICE)] == \
        ["serie", "graphe", "conversation", "appels", "matrice"]
    for d in (SERIE, GRAPHE, CONV, APPELS, MATRICE):
        for task in (None, "lire", "ecrire"):
            assert decode_auto(encode_auto(d, task)) == d
    assert choose_format(APPELS)[0] == "appels"


def test_random_documents_of_every_kind_round_trip():
    script = Path(__file__).with_name("fuzz_cstl_formes.py")
    out = subprocess.run([sys.executable, str(script), "600"], capture_output=True, text=True, check=True).stdout
    assert "echecs 0 / 600" in out, out


MIXTE = {"crate": {"name": "serde", "downloads": 17}, "keywords": [{"id": "a", "n": 1}, {"id": "b", "n": 2, "x": None}],
         "tags": ["web"], "deps": [{"a": 1}, [2]]}


def test_mixte_tables_regular_lists_with_json_values():
    """2026-11-24 : structure d'objets_json, listes d'objets en tables a
    cellules JSON ; un champ absent d'un element s'ecrit nom=valeur."""
    text = F.encode_mixte(MIXTE)
    assert text.splitlines() == ['crate:', '  name: "serde"', '  downloads: 17', 'keywords[2]: id n',
                                 '  "a" 1', '  "b" 2 x=null', 'tags: ["web"]', 'deps:', '  - {"a":1}', '  - [2]']
    assert F.decode_mixte(text) == MIXTE
    assert F.decode_mixte(F.encode_mixte([{"a": 1}, {"a": 2}])) == [{"a": 1}, {"a": 2}]
    with pytest.raises(CompactError):
        F.decode_mixte("keywords[2]: id\n  \"a\"")


def test_changements_diff_apply_and_text():
    """2026-11-24 : ce qui a change, en operations JSON Patch ; diff puis
    apply redonne le document ; la forme texte se relit a l'identique."""
    old = {"commandes": [{"id": "o1", "statut": "payee"}, {"id": "o2"}], "total": 3}
    new = {"commandes": [{"id": "o1", "statut": "expediee", "suivi": "X1"}], "total": 3, "note": "a b"}
    ops = F.diff(old, new)
    assert F.apply(old, ops) == new
    text = F.encode_changements(ops)
    assert text.splitlines()[:2] == ["@changements", '~ /commandes/0/statut "expediee"']
    assert F.decode_changements(text) == ops
    assert data_kind(ops) == "changements" and decode_auto(encode_auto(ops)) == ops
    with pytest.raises(CompactError):
        F.apply(old, [{"op": "replace", "path": "/absent", "value": 1}])


def test_relationnel_paths_strict_tables_and_unknown():
    """2026-11-24 : forme relationnel -- valeurs JSON, chemins condenses pour
    les objets a un seul champ, tables seulement si tous les elements ont
    exactement les memes champs, inconnu ?, comptes [N]."""
    doc = {"name": "react", "engines": {"node": ">=0.10"}, "repository": {"url": "git+x", "type": "git"},
           "maintainers": [{"name": "fb", "email": "a@x"}, {"name": "bot", "email": "b@x"}],
           "files": [{"p": "a"}, {"p": "b", "size": 3}], "pay": {"$unknown": {"why": "bank"}},
           "lodash.get": 1, "keywords": ["react"]}
    text = F.encode_relationnel(doc)
    assert text.splitlines()[:6] == ['name: "react"', 'engines.node: ">=0.10"', "repository:", '  url: "git+x"',
                                     '  type: "git"', "maintainers[2]: name email"]
    assert 'files[2]:' in text and '  - p: "a"' in text and 'pay: ?{"why":"bank"}' in text
    assert '"lodash.get": 1' in text
    assert F.decode_relationnel(text) == doc
    for d in ([{"a": 1}, {"a": 2}], [{"a": 1}, [2]], {}, [], 5, None, {"a": {"b": {"c": {}}}}):
        assert F.decode_relationnel(F.encode_relationnel(d)) == d
    with pytest.raises(CompactError):
        F.decode_relationnel('maintainers[3]: name\n  "a"\n  "b"')
