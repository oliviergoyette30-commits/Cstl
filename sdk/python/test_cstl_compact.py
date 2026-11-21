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
