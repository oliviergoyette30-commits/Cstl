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
                r[attr] = rnd.choice(["10", "f", "deux mots", 'avec "guillemets"', "x#y"])
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
                    r[attr] = rnd.choice(["10", "f", "deux mots", 'avec "guillemets"', "x#y", "-"])
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
