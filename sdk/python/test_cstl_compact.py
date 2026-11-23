"""Tests de cstl_compact : aller-retour sans perte, erreurs franches."""
import random

import pytest

from cstl_compact import CompactError, decode, encode, policy_to_compact

FULL = [
    {"type": "POSSESSES", "subject": "agent_a", "object": "role:finance"},
    {"type": "STATE", "subject": "patient_consent", "object": "false"},
    {"type": "PERFORM", "subject": "role:finance", "object": "refund_order", "modality": "MAY",
     "amount_at_most": "100", "total_at_most": "1500", "total_window_secs": "86400",
     "reason": "remboursements dans le plafond, et dans le budget"},
    {"type": "PERFORM", "subject": "agent_a", "object": "refund_order", "modality": "MUST_NOT", "amount_above": "100"},
    {"type": "PERFORM", "subject": "agent_a", "object": "share_medical_record", "modality": "MUST_NOT",
     "unless_fact": "patient_consent", "reason": 'il a dit "non"'},
    {"type": "PERFORM", "subject": "agent_a", "object": "log_transaction", "modality": "MUST", "if_fact": "audit_open",
     "valid_from": "2026-01-01", "valid_until": "2026-12-31"},
    {"type": "PERFORM", "subject": "agent_a", "object": "grant_admin", "modality": "MUST_NOT", "total_above": "3",
     "custom": "x y"},
    {"type": "CAUSES", "subject": "step_1", "object": "step 2", "confidence": "0.9"},
    {"type": "STATE", "subject": "order_1", "object": "shipped"},
    {"type": "POSSESSES", "subject": "agent_b", "object": "role:ops", "role": "false"},
]


def _value(rnd, attr, extra=()):
    """Valeur aleatoire : valide si l'attribut est type (montant, priorite...),
    sinon n'importe quelle chaine, y compris bizarre."""
    typed = {"num": ["10", "-3", "2.5"], "int": ["7", "-2"], "date": ["2026-11-22", "2026-01-01T10:00:00Z"]}
    from cstl_compact import CONDITION_TYPES
    if attr in CONDITION_TYPES:
        return rnd.choice(typed[CONDITION_TYPES[attr]])
    return rnd.choice(["10", "f", "deux mots", 'avec "guillemets"', "x#y", *extra])


def test_round_trip_keeps_everything():
    assert decode(encode(FULL)) == FULL


def test_compact_is_short_and_readable():
    text = encode(FULL[:4])
    assert text.splitlines()[0] == "ROLE agent_a finance"
    assert text.splitlines()[1] == "FACT patient_consent false"
    assert text.splitlines()[3] == "agent_a MUST_NOT refund_order >100"


def test_written_by_hand():
    rels = decode('agent_f MUST_NOT wipe_db >100 if:audit_open "pas pendant un audit"  # commentaire\n\n'
                  'role:ops MUST backup_db\nROLE agent_f ops\nFACT audit_open true')
    assert rels == [
        {"type": "PERFORM", "subject": "agent_f", "object": "wipe_db", "modality": "MUST_NOT",
         "amount_above": "100", "if_fact": "audit_open", "reason": "pas pendant un audit"},
        {"type": "PERFORM", "subject": "role:ops", "object": "backup_db", "modality": "MUST"},
        {"type": "POSSESSES", "subject": "agent_f", "object": "role:ops"},
        {"type": "STATE", "subject": "audit_open", "object": "true"},
    ]


def test_quoted_condition_is_not_a_reason():
    assert decode('a MUST b if:"fait long"') == [
        {"type": "PERFORM", "subject": "a", "object": "b", "modality": "MUST", "if_fact": "fait long"}]


@pytest.mark.parametrize("bad", ["agent_a DOIT wipe_db", "ROLE agent_a", "FACT x maybe", 'a MUST b "raison',
                                 "a MUST b ???", "REL CAUSES a"])
def test_unreadable_lines_are_refused(bad):
    with pytest.raises(CompactError):
        decode(bad)


def test_random_round_trips():
    rnd = random.Random(7)
    for _ in range(300):
        rels = []
        for _ in range(rnd.randint(1, 8)):
            r = {"type": "PERFORM", "subject": rnd.choice(["a", "role:x"]), "object": rnd.choice(["b", "c_d"]),
                 "modality": rnd.choice(["MUST", "MUST_NOT", "MAY"])}
            for attr in rnd.sample(["amount_above", "amount_at_most", "if_fact", "unless_fact", "total_above", "reason"],
                                   rnd.randint(0, 3)):
                r[attr] = _value(rnd, attr)
            if r.get("reason") is None:
                r.pop("reason", None)
            rels.append(r)
        assert decode(encode(rels)) == rels


def test_policy_view():
    p = {"default": "allow", "required": [{"action": "log_transaction", "reason": "tracabilite"}],
         "permitted": [{"action": "refund_order", "amount_at_most": "100", "total_at_most": "1500", "spent_now": "200"}],
         "forbidden": [{"action": "share_medical_record", "unless_fact": "patient_consent", "unless_fact_now": "false"}],
         "ignored": [{"action": "grant_admin", "author": "relay2"}]}
    text = policy_to_compact(p)
    assert 'MUST log_transaction "tracabilite"' in text
    assert "MAY refund_order <=100 total<=1500 spent=200" in text
    assert "MUST_NOT share_medical_record unless:patient_consent facts=patient_consent=false" in text
    assert "IGNORE grant_admin par:relay2" in text


# ---- mode tableau (en-tetes de colonnes) ----
from cstl_compact import GENERIC_TYPE, decode_report, _is_fact, _is_role, _is_rule  # noqa: E402


def _generic_ok(r):
    return (r.get("subject") is not None and r.get("object") is not None and GENERIC_TYPE.match(r.get("type", ""))
            and r["type"] not in ("ROLES", "FACTS", "RULES")
            and all(GENERIC_TYPE.match(k) for k in r if k not in ("type", "subject", "object")))


def _grouped(rels):
    """Ordre du mode tableau : roles, faits, regles, relations restees en
    ligne, puis les sections generiques par type (ordre d'apparition)."""
    other = [r for r in rels if not (_is_role(r) or _is_fact(r) or _is_rule(r))]
    by_type = {}
    for r in other:
        if _generic_ok(r):
            by_type.setdefault(r["type"], []).append(r)
    return ([r for r in rels if _is_role(r)] + [r for r in rels if _is_fact(r)] + [r for r in rels if _is_rule(r)]
            + [r for r in other if not _generic_ok(r)] + [r for rs in by_type.values() for r in rs])


def test_table_round_trip_keeps_everything():
    assert decode(encode(FULL, table=True)) == _grouped(FULL)


def test_table_written_by_hand():
    text = ("ROLES[1] agent role\n  agent_f ops\nFACTS[1] fait valeur\n  audit_open true\n"
            "RULES[2] qui modalite action au_dela si sauf_si\n"
            "  agent_f MUST_NOT wipe_db 100 - audit_open\n  role:ops MUST backup_db - - - \"sauvegarde\"")
    assert decode(text) == [
        {"type": "POSSESSES", "subject": "agent_f", "object": "role:ops"},
        {"type": "STATE", "subject": "audit_open", "object": "true"},
        {"type": "PERFORM", "subject": "agent_f", "object": "wipe_db", "modality": "MUST_NOT",
         "amount_above": "100", "unless_fact": "audit_open"},
        {"type": "PERFORM", "subject": "role:ops", "object": "backup_db", "modality": "MUST", "reason": "sauvegarde"},
    ]


def test_table_wrong_count_is_caught():
    text = "RULES[3] qui modalite action au_dela si sauf_si\n  a MUST b - - -\n  a MUST_NOT c 5 - -"
    with pytest.raises(CompactError, match=r"RULES\[3\] annonce, 2 ligne"):
        decode(text)
    rels, problems = decode_report(text)
    assert len(rels) == 2 and problems == ["RULES[3] annonce, 2 ligne(s) ecrite(s)"]


def test_table_and_lines_can_mix():
    text = "FACTS[1] fait valeur\n  x true\nagent_a MUST b if:x\nROLE agent_a ops"
    assert decode(text) == [{"type": "STATE", "subject": "x", "object": "true"},
                            {"type": "PERFORM", "subject": "agent_a", "object": "b", "modality": "MUST", "if_fact": "x"},
                            {"type": "POSSESSES", "subject": "agent_a", "object": "role:ops"}]


@pytest.mark.parametrize("bad", ["RULES[1] qui modalite action au_dela si sauf_si\n  a MUST b -",
                                 "RULES[1] x\n  a DOIT b - - -", "FACTS[1] x\n  f peut-etre"])
def test_table_unreadable_rows_are_refused(bad):
    with pytest.raises(CompactError):
        decode(bad)


def test_table_random_round_trips():
    rnd = random.Random(11)
    for _ in range(300):
        rels = []
        for _ in range(rnd.randint(1, 8)):
            kind = rnd.random()
            if kind < 0.2:
                rels.append({"type": "POSSESSES", "subject": rnd.choice(["a", "b c"]), "object": "role:" + rnd.choice(["x", "-"])})
            elif kind < 0.4:
                rels.append({"type": "STATE", "subject": rnd.choice(["f", "g"]), "object": rnd.choice(["true", "false"])})
            else:
                r = {"type": "PERFORM", "subject": rnd.choice(["a", "role:x"]), "object": rnd.choice(["b", "c_d"]),
                     "modality": rnd.choice(["MUST", "MUST_NOT", "MAY"])}
                for attr in rnd.sample(["amount_above", "amount_at_most", "if_fact", "unless_fact", "total_above", "reason"],
                                       rnd.randint(0, 4)):
                    r[attr] = _value(rnd, attr, extra=["-"])
                rels.append(r)
        assert decode(encode(rels, table=True)) == _grouped(rels)


# ---- sections generiques (2026-11-22) ----
def test_generic_section_round_trip_and_shape():
    rels = [{"type": "ORDER", "subject": "o1", "object": "client_3", "amount": "450", "status": "paid"},
            {"type": "ORDER", "subject": "o2", "object": "client_1", "amount": "80"},
            {"type": "EVENT", "subject": "agent_a", "object": "error", "code": "E500", "note": "-"},
            {"type": "ORDER", "subject": "o3", "object": "", "amount": "", "status": "deux mots"}]
    text = encode(rels, table=True)
    assert text.splitlines()[0] == "ORDER[3] subject object amount status"
    assert text.splitlines()[2] == "  o2 client_1 80 -"
    assert decode(text) == _grouped(rels)


def test_generic_section_wrong_width_or_count_is_refused():
    with pytest.raises(CompactError, match="attend 4 valeurs"):
        decode("ORDER[1] subject object amount status\n  o1 client 5")
    with pytest.raises(CompactError, match=r"ORDER\[2\] annonce, 1 ligne"):
        decode("ORDER[2] subject object amount\n  o1 client 5")


def test_generic_random_round_trips():
    rnd = random.Random(23)
    for _ in range(300):
        rels = []
        for _ in range(rnd.randint(1, 10)):
            r = {"type": rnd.choice(["ORDER", "EVENT", "TASK", "x.y"]), "subject": rnd.choice(["a", "b c", "-", ""]),
                 "object": rnd.choice(["o", "role:z", "#x"])}
            for k in rnd.sample(["amount", "status", "due", "weird key", "ms"], rnd.randint(0, 4)):
                r[k] = rnd.choice(["1", "", "-", "deux mots", 'q"uote', "x#y"])
            rels.append(r)
        assert decode(encode(rels, table=True)) == _grouped(rels)


# ---- mode sans mot-cle (2026-11-22) ----
from cstl_compact import encode_symbols  # noqa: E402


def _key(r):
    return sorted(r.items())


def test_symbols_shape_and_round_trip():
    rels = [{"type": "POSSESSES", "subject": "agent_a", "object": "role:finance"},
            {"type": "STATE", "subject": "kyc_done", "object": "false"},
            {"type": "PERFORM", "subject": "agent_f", "object": "wipe_db", "modality": "MUST_NOT", "amount_above": "100",
             "unless_fact": "audit_open", "reason": "pas pendant un audit"},
            {"type": "PERFORM", "subject": "role:ops", "object": "backup_db", "modality": "MUST", "if_fact": "night"},
            {"type": "PERFORM", "subject": "agent_a", "object": "refund", "modality": "MAY", "amount_at_most": "50"},
            {"type": "ORDER", "subject": "o1", "object": "client_3", "amount": "450", "status": "paid"},
            {"type": "ORDER", "subject": "o2", "object": "client_1", "amount": "80"}]
    text = encode_symbols(rels)
    assert text.splitlines()[:5] == ["agent_a @finance", "kyc_done=0",
                                     'agent_f !wipe_db >100 -audit_open "pas pendant un audit"',
                                     "role:ops *backup_db +night", "agent_a ?refund <=50"]
    assert text.splitlines()[5:] == ["ORDER[2]: amount status", "  o1 client_3 450 paid", "  o2 client_1 80 -"]
    assert sorted(map(_key, decode(text))) == sorted(map(_key, rels))


def test_symbols_fall_back_when_needed():
    rels = [{"type": "PERFORM", "subject": "deux mots", "object": "x", "modality": "MUST"},
            {"type": "PERFORM", "subject": "a", "object": "b", "modality": "FORBID"},
            {"type": "STATE", "subject": "order_1", "object": "shipped"}]
    assert sorted(map(_key, decode(encode_symbols(rels)))) == sorted(map(_key, rels))


def test_symbols_random_round_trips():
    rnd = random.Random(31)
    for _ in range(400):
        rels = []
        for _ in range(rnd.randint(1, 10)):
            k = rnd.random()
            if k < 0.15:
                rels.append({"type": "POSSESSES", "subject": rnd.choice(["a", "b c", "x.y"]), "object": "role:" + rnd.choice(["r", "-", "s t"])})
            elif k < 0.3:
                rels.append({"type": "STATE", "subject": rnd.choice(["f", "g h", "k=1"]), "object": rnd.choice(["true", "false"])})
            elif k < 0.7:
                r = {"type": "PERFORM", "subject": rnd.choice(["a", "role:x", "!a", "b c"]), "object": rnd.choice(["b", "*c", "d e"]),
                     "modality": rnd.choice(["MUST", "MUST_NOT", "MAY", "REQUIRE"])}
                for attr in rnd.sample(["amount_above", "amount_at_most", "if_fact", "unless_fact", "total_above", "reason"],
                                       rnd.randint(0, 4)):
                    r[attr] = _value(rnd, attr, extra=['q"t', "-x", "+y"])
                rels.append(r)
            else:
                r = {"type": rnd.choice(["ORDER", "EVENT"]), "subject": rnd.choice(["o1", "-", "", "a b"]), "object": rnd.choice(["c", "@d", "e=1"])}
                for col in rnd.sample(["amount", "status", "odd key"], rnd.randint(0, 3)):
                    r[col] = rnd.choice(["1", "", "-", "deux mots", "x#y"])
                rels.append(r)
        assert sorted(map(_key, decode(encode_symbols(rels)))) == sorted(map(_key, rels)), rels


# ---- typage (2026-11-22) ----
def test_typed_columns_are_written_and_checked():
    rels = [{"type": "SCHEMA", "subject": "order_r", "object": "v1", "amount": "num", "date": "date"},
            {"type": "order_r", "subject": "o1", "object": "c3", "amount": "450", "date": "2026-11-04", "status": "paid"},
            {"type": "order_r", "subject": "o2", "object": "c1", "amount": "80"}]
    text = encode_symbols(rels)
    assert "order_r[2]: amount:num date:date status" in text
    assert sorted(map(_key, decode(text))) == sorted(map(_key, rels))
    assert "order_r[2] subject object amount:num date:date status" in encode(rels, table=True)
    with pytest.raises(CompactError, match="order_r.amount='beaucoup'"):
        decode("order_r[1]: amount:num\n  o1 c3 beaucoup")
    with pytest.raises(CompactError, match="type inconnu 'money'"):
        decode("order_r[1]: amount:money\n  o1 c3 5")
    with pytest.raises(CompactError, match="n'est pas de type num"):
        encode_symbols([{"type": "order_r", "subject": "o1", "object": "c", "amount": "x"}], types={"order_r": {"amount": "num"}})


def test_rule_conditions_are_typed():
    with pytest.raises(CompactError, match="amount_above='cent'"):
        decode("agent_a !refund >cent")
    with pytest.raises(CompactError, match="valid_until='hier'"):
        decode("agent_a MUST_NOT refund until:hier")
    with pytest.raises(CompactError, match="priority"):
        decode("agent_a ?refund priority=haute")
    with pytest.raises(CompactError):
        encode([{"type": "PERFORM", "subject": "a", "object": "b", "modality": "MUST_NOT", "amount_above": "beaucoup"}])
    assert decode("agent_a ?refund <=50 priority=10 until:2026-12-31")[0]["priority"] == "10"


# ---- vue de la politique sans mot-cle (2026-11-22) ----
from cstl_compact import policy_to_symbols  # noqa: E402


def test_policy_view_without_keywords():
    p = {"default": "deny", "required": [{"action": "log_transaction", "reason": "tracabilite"}],
         "permitted": [{"action": "refund_order", "amount_at_most": "100", "total_at_most": "1500", "spent_now": "200",
                        "priority": "10"}],
         "forbidden": [{"action": "liquidate", "if_fact": "incident_open", "if_fact_now": "unknown"},
                       {"action": "share_record", "unless_fact": "consent", "unless_fact_now": "false", "via_role": "support"}],
         "ignored": [{"action": "grant_admin", "author": "relay2"}]}
    lines = policy_to_symbols(p).splitlines()
    assert "Sans regle : interdit" in lines[0]
    assert lines[1:5] == ['*log_transaction "tracabilite"',
                          "?refund_order <=100 total<=1500 priority=10 spent=200",
                          "!liquidate +incident_open etat=incident_open=inconnu",
                          "!share_record -consent etat=consent=faux role:support"]
    assert lines[5].startswith("# IGNORE") and "grant_admin" in lines[5]
    assert "priority=10" in policy_to_compact(p)


# ---- arbres (donnees imbriquees, 2026-11-22) ----
from cstl_compact import decode_tree, encode_tree  # noqa: E402


def test_tree_round_trip_and_shape():
    data = {"orders": [{"id": "o0", "client": "c5", "lines": [
                {"item": "gpu", "qty": 5, "price": 714.5, "options": [{"name": "warranty", "price": 9}]},
                {"item": "lic", "qty": 1, "price": 20}]},
               {"id": "o1", "client": "c3", "note": "two words", "lines": [{"item": "gpu", "qty": 2, "price": 1}]}]}
    text = encode_tree(data)
    assert text.splitlines()[0] == "orders[2]#: client note"
    assert "lines[3]^: item qty price" in text and "options[1]^: name price" in text
    assert "  o0.0 warranty 9" in text, text
    assert decode_tree(text) == data


def test_tree_keeps_types_and_odd_values():
    data = {"x": [{"id": "a", "s": "05", "n": 5, "f": 1.5, "b": True, "z": None, "d": "-", "e": "", "w": "two words",
                   "tags": ["p", "q"], "meta": {"k": 1}, "tilde": "x~"}]}
    assert decode_tree(encode_tree(data)) == data


def test_tree_errors_are_not_guessed():
    with pytest.raises(CompactError, match=r"lines\[2\] annonce"):
        decode_tree("orders[1]#: c\n  o0 x\nlines[2]^: item\n  o0 gpu")
    with pytest.raises(CompactError, match="parent inconnu"):
        decode_tree("lines[1]^: item\n  o9 gpu")
    with pytest.raises(CompactError, match="attend 2 valeurs"):
        decode_tree("orders[1]#: c\n  o0")


def test_tree_random_round_trips():
    rnd = random.Random(41)

    def obj(depth, with_id, i):
        o = {"id": f"n{depth}_{i}_{rnd.randint(0, 9999)}"} if with_id else {}
        for k in rnd.sample(["a", "b", "c", "d"], rnd.randint(1, 3)):
            o[k] = rnd.choice([1, 2.5, "x", "two words", "-", "", None, True, "07", ["l"], {"m": 1}])
        if depth < 3 and rnd.random() < 0.6:
            o["kids"] = [obj(depth + 1, rnd.random() < 0.3, j) for j in range(rnd.randint(1, 3))]
        return o
    for _ in range(300):
        with_id = rnd.random() < 0.5
        data = {"root": [obj(0, with_id, i) for i in range(rnd.randint(1, 4))]}
        assert decode_tree(encode_tree(data)) == data


# ---- arbres <-> relations pour le serveur (2026-11-22) ----
from cstl_compact import relations_to_tree, tree_to_relations  # noqa: E402


def test_tree_relations_round_trip_and_escaping():
    data = {"contracts": [{"id": "c1", "party": "p", "sections": [
        {"title": "pay", "clauses": [{"ref": "1.1", "modality": "MUST_NOT", "type": "x", "f_note": "y", "tree": 3,
                                      "amount": 5.5, "ok": False, "none": None}]}]}]}
    rels = tree_to_relations(data, "t1")
    assert rels[0]["type"] == "tree_root" and rels[0]["tree_kind"] == "tables" and rels[0]["tree_n"] == "1"
    assert rels[1]["tree_n"] == "1" and rels[2]["tree_n"] == "1" and "tree_n" not in rels[3]
    clause = rels[3]
    assert clause["f_modality"] == "MUST_NOT" and "modality" not in clause and clause["f_type"] == "x"
    assert clause["f_f_note"] == "y" and clause["f_tree"] == "3" and clause["tree"] == "t1"
    assert relations_to_tree(rels, "t1") == data


def test_tree_relations_refuse_what_the_server_would_lose():
    for bad in ({"a": [{"x": "y" * 301}]}, {"a": [{"bad name": 1}]}, {"a": [{"id": "d"}, {"id": "d"}]},
                {"a": [{"x": list(range(200))}]}):
        with pytest.raises(CompactError):
            tree_to_relations(bad, "t1")


def test_tree_relations_carry_lists_text_and_any_root():
    """2026-11-22 : listes, objets et textes multilignes passent en JSON
    base64url (type json) ; la racine peut etre libre."""
    from cstl_compact import relations_to_tree
    for data in ({"a": [{"x": "avec [crochet]", "q": 'guil"lemet', "t": "l1\nl2", "tags": ["p", "q"],
                         "geo": {"lat": 1.5}, "m": [[1, 2], [3, 4]]}]},
                 {"service": "billing", "tags": ["eu"], "tickets": [{"id": "t1", "n": 2}]},
                 [{"a": 1}, {"a": 2, "kids": [{"b": True}]}], 42, "texte", [1, "deux", None], {}):
        rels = tree_to_relations(data, "t1")
        assert all(not any(c in str(v) for c in '"[]\n') for r in rels for v in r.values())
        assert relations_to_tree(rels, "t1") == data


def test_tree_relations_random_round_trips():
    rnd = random.Random(53)

    def obj(depth, i):
        o = {"id": f"n{depth}_{i}_{rnd.randint(0, 10**6)}"} if rnd.random() < 0.4 else {}
        for k in rnd.sample(["a", "b", "modality", "type", "f_x", "tree"], rnd.randint(1, 3)):
            o[k] = rnd.choice([1, 2.5, "x", "two words", "-", "", None, True, "07", "a,b"])
        if depth < 3 and rnd.random() < 0.6:
            o[rnd.choice(["kids", "items"])] = [obj(depth + 1, j) for j in range(rnd.randint(1, 3))]
        return o
    for _ in range(300):
        data = {"root": [obj(0, i) for i in range(rnd.randint(1, 4))]}
        assert relations_to_tree(tree_to_relations(data, "t"), "t") == data


# ---- arbres indentes (v2, 2026-11-22) ----
from cstl_compact import decode_tree_indented, encode_tree_indented  # noqa: E402


def test_indented_tree_shape_and_round_trip():
    data = {"orders": [{"id": "o0", "client": "c5", "lines": [{"item": "gpu", "qty": 5, "options": [{"name": "w", "price": 9}]},
                                                              {"item": "lic", "qty": 1}]},
                       {"id": "o1", "client": "c3", "note": "two words", "lines": [{"item": "r", "qty": 3}]}]}
    text = encode_tree_indented(data)
    assert text.splitlines()[:3] == ["@orders[2]: id client", "@lines[3]^orders: item qty", "@options[1]^lines: name price"]
    assert '  o1 c3 note="two words"'.strip() in text and "\n    w 9" in text, text
    assert decode_tree_indented(text) == data


def test_indented_tree_tags_when_a_parent_has_several_lists():
    data = {"a": [{"x": 1, "b": [{"y": 2}], "c": [{"z": 3}]}], "d": [{}]}
    text = encode_tree_indented(data)
    assert "a> 1" in text and "  b> 2" in text and "  c> 3" in text and "d> ." in text, text
    assert decode_tree_indented(text) == data


def test_indented_tree_errors_are_not_guessed():
    with pytest.raises(CompactError, match=r"lines\[2\] annonce, 1"):
        decode_tree_indented("@orders[1]: id\n@lines[2]^orders: item\no0\n  gpu")
    with pytest.raises(CompactError, match="sans parent"):
        decode_tree_indented("@orders[1]: id\n@lines[1]^orders: item\n    gpu\no0")
    with pytest.raises(CompactError, match="attend 2 valeurs"):
        decode_tree_indented("@orders[1]: id client\no0")


def test_indented_tree_random_round_trips():
    rnd = random.Random(61)

    def obj(depth):
        o = {}
        for k in rnd.sample(["a", "b", "c", "id"], rnd.randint(0, 3)):
            o[k] = rnd.choice([1, 2.5, "x", "two words", "-", "", None, True, "07", "k=v", "x>", ".", ["l"], {"m": 1}])
        if depth < 3 and rnd.random() < 0.6:
            for key in rnd.sample(["kids", "items"], rnd.randint(1, 2)):
                o[key] = [obj(depth + 1) for _ in range(rnd.randint(1, 3))]
        return o
    for _ in range(300):
        data = {"root": [obj(0) for _ in range(rnd.randint(1, 4))]}
        assert decode_tree_indented(encode_tree_indented(data)) == data


def test_indented_tree_inline_lists_blocks_and_free_root():
    """2026-11-22 : listes et objets en ligne, texte multiligne en bloc « | »,
    racine libre (champs, liste, valeur seule)."""
    data = {"service": "billing", "tags": ["prod", "eu"],
            "tickets": [{"id": "t1", "geo": {"lat": 45.5, "lon": -73.6}, "desc": "Bloque.\nRelance demain.",
                         "m": [[1, 2], [3, 4]]}, {"id": "t2", "desc": "ok"}]}
    text = encode_tree_indented(data)
    assert text.splitlines() == [
        "@@ service=billing tags=[prod,eu]", "@tickets[2]: id desc",
        "t1 |2 geo={lat:45.5,lon:-73.6} m=[[1,2],[3,4]]", "  | Bloque.", "  | Relance demain.", "t2 ok"]
    assert decode_tree_indented(text) == data
    assert encode_tree_indented([1, "deux", None]) == "@@= [1,deux,null]"
    assert encode_tree_indented([{"a": 1}, {"a": 2}]).splitlines()[0] == "@@list"
    for d in ({}, [], 7, "x y", None, [{"a": 1}], {"orders": [{"lines": [{"x": 1}]}], "lines": [{"x": 2}]},
              {"a b": 1, '"': "@x", "k": "|2", "e": "", "n": "\n"}):
        assert decode_tree_indented(encode_tree_indented(d)) == d


def test_indented_tree_blocks_are_checked():
    with pytest.raises(CompactError):
        decode_tree_indented("@t[1]: d\n|2\n  | une seule ligne")
    with pytest.raises(CompactError):
        decode_tree_indented("@t[1]: d\n|1\n  pas un bloc")
    with pytest.raises(CompactError):
        decode_tree_indented("@t[1]: d\n[a,b")


def test_indented_tree_any_json_random_round_trips():
    nasty = ["", " ", "a b", "-", ".", "null", "true", "1", "01", "1e5", "x>", "k=v", "|", "|2", "[a", "{", "a,b",
             "a:b", '"q"', "back\\slash", "#tag", "l1\nl2", "\n", "fin\n", "\r\n", "tab\tx", "é 漢", "\u2028",
             "  lead", "trail  ", "~", "@x", "@@", "| x"]
    keys = ["id", "name", "a b", "x=y", "", "list", "_", "tree", "lines", "opts", "é", '"', "k.1", "-"]
    for seed in range(3000):
        rnd = random.Random(seed)

        def scalar():
            return rnd.choice([None, True, False, rnd.randint(-5, 99), rnd.random() * 100, rnd.choice(nasty)])

        def val(d):
            c = rnd.random()
            if d > 3 or c < 0.5:
                return scalar()
            if c < 0.7:
                return [val(d + 1) for _ in range(rnd.randint(0, 3))]
            if c < 0.85:
                return {rnd.choice(keys): val(d + 1) for _ in range(rnd.randint(0, 3))}
            return [obj(d + 1) for _ in range(rnd.randint(1, 3))]

        def obj(d):
            return {rnd.choice(keys): val(d) for _ in range(rnd.randint(0, 4))}

        data = rnd.choice([lambda: obj(0), lambda: [obj(0) for _ in range(2)], lambda: val(0)])()
        assert decode_tree_indented(encode_tree_indented(data)) == data, (seed, data)
        assert decode_tree_indented(encode_tree_indented(data), lenient=True) == data, (seed, data)
        assert decode_tree_indented(encode_tree_indented(data, counts="end")) == data, (seed, data)
        assert decode_tree_indented(encode_tree_indented(data, counts="line")) == data, (seed, data)


def test_indented_tree_lenient_mode_for_model_written_text():
    """2026-11-22 : un modele avait ecrit `@orders[8]: id client date note`
    (note n'existe que sur une commande) et `@options[7]` (il y en avait 10).
    Le mode tolerant relit ce texte sans rien deviner ; le mode strict le
    refuse toujours."""
    text = ("@orders[2]: id client note\n@lines[9]^orders: item qty\n"
            "o0 c5\n    gpu 5\n    lic 3\no1 c3 note=fragile\n\trouter 1")
    with pytest.raises(CompactError):
        decode_tree_indented(text)
    warn = []
    got = decode_tree_indented(text, lenient=True, warnings=warn)
    assert got == {"orders": [{"id": "o0", "client": "c5", "lines": [{"item": "gpu", "qty": 5}, {"item": "lic", "qty": 3}]},
                              {"id": "o1", "client": "c3", "note": "fragile", "lines": [{"item": "router", "qty": 1}]}]}
    assert warn == ["lines[9] annonce, 3 element(s) ecrit(s)"]
    # [N] facultatif, dans les deux modes.
    assert decode_tree_indented("@t: a\n1\n2") == {"t": [{"a": 1}, {"a": 2}]}
    # Une indentation incoherente reste refusee.
    with pytest.raises(CompactError):
        decode_tree_indented("@o: a\n@l^o: b\nx\n    y\n  z", lenient=True)


def test_indented_tree_counts_at_the_end():
    """2026-11-22 : les nombres d'elements peuvent venir APRES l'arbre
    (`@end orders=2 lines=3`) : qui ecrit de gauche a droite compte ce qu'il
    vient d'ecrire au lieu de l'annoncer. `@end` seul detecte deja une coupure."""
    data = {"orders": [{"id": "o0", "lines": [{"q": 1}, {"q": 2}]}, {"id": "o1", "lines": [{"q": 3}]}]}
    text = encode_tree_indented(data, counts="end")
    assert text.splitlines() == ["@orders: id", "@lines^orders: q", "o0", "  1", "  2", "o1", "  3",
                                 "@end orders=2 lines=3"]
    assert decode_tree_indented(text) == data
    assert decode_tree_indented(text.replace("@end orders=2 lines=3", "@end")) == data
    for bad in (text.replace("lines=3", "lines=4"), text + "\no2", text.replace("@end", "@end x=1"),
                text.replace("lines=3", "lines=trois")):
        with pytest.raises(CompactError):
            decode_tree_indented(bad)
    warn = []
    assert decode_tree_indented(text.replace("lines=3", "lines=4"), lenient=True, warnings=warn) == data
    assert warn == ["lines[4] annonce, 3 element(s) ecrit(s)"]


def test_indented_tree_counts_at_the_end_of_each_line():
    """2026-11-23 : `/N` en fin de ligne = nombre d'enfants directs. Un modele
    compte bien 1 a 5 elements qu'il vient d'ecrire ; et le compte local dit
    OU il manque une ligne."""
    data = {"c": [{"id": "c1", "s": [{"t": "a", "k": [{"r": 1}, {"r": 2}]}, {"t": "b", "k": [{"r": 3}]}]},
                  {"id": "c2"}], "v": "/3"}
    text = encode_tree_indented(data, counts="line")
    assert text.splitlines() == ['@@ v="/3"', "@c: id", "@s^c: t", "@k^s: r",
                                 "c1 /2", "  a /2", "    1", "    2", "  b /1", "    3", "c2", "@end"]
    assert decode_tree_indented(text) == data
    bad = text.replace("a /2", "a /3")
    with pytest.raises(CompactError, match="ligne 6"):
        decode_tree_indented(bad)
    warn = []
    assert decode_tree_indented(bad, lenient=True, warnings=warn) == data
    assert warn == ["ligne 6 : /3 annonce, 2 enfant(s) ecrit(s)"]


def test_tree_relations_counts_are_checked_on_read():
    """2026-11-23 : tree_n (enfants annonces) est reverifie a la relecture :
    un arbre auquel il manque une branche est refuse, pas rendu incomplet."""
    from cstl_compact import relations_to_tree
    data = {"orders": [{"id": "o1", "lines": [{"q": 1}, {"q": 2}]}, {"id": "o2"}]}
    rels = tree_to_relations(data, "t1")
    assert relations_to_tree(rels, "t1") == data
    for drop in (2, 4):          # une ligne, puis une commande entiere
        with pytest.raises(CompactError, match="arbre incomplet"):
            relations_to_tree(rels[:drop] + rels[drop + 1:], "t1")


def test_tree_hash_is_the_servers():
    """2026-11-23 : meme empreinte que src/server/tree_check.rs (test
    canonical_hash_matches_the_python_sdk, valeur figee des deux cotes)."""
    from cstl_compact import tree_hash
    rels = tree_to_relations({"orders": [{"id": "o1", "lines": [{"q": 1}, {"q": 2}]}, {"id": "o2"}]}, "t1")
    assert tree_hash(rels, "t1") == "sha256:f3a92ea780204ab228c0c413ad11595615f81a7708a918a248711b000a157db5"
    with_source = tree_to_relations({"orders": [{"id": "o1"}]}, "t1", source="billing")
    assert with_source[0]["tree_source"] == "billing"
    from cstl_compact import relations_to_tree
    assert relations_to_tree(with_source, "t1") == {"orders": [{"id": "o1"}]}
