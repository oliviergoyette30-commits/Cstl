#!/usr/bin/env python3
"""
test_operator_agreement_study.py -- verification automatisee, reproductible,
de la logique de collecte/comparaison/scoring de operator_agreement_study.py.

AUCUN reseau, AUCUNE cle API -- uniquement DeterministicStubBackend. Ce test
prouve que le harnais calcule correctement l'accord inter-annotateurs (pas
qu'un vrai LLM choisirait tel ou tel operateur -- ca reste a faire par
l'utilisateur avec un vrai backend, voir l'en-tete de operator_agreement_study.py).

Lancer avec: pytest scripts/test_operator_agreement_study.py -v
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from operator_agreement_study import (  # noqa: E402
    REFERENCE_FACTS,
    DeterministicStubBackend,
    GeminiBackend,
    build_prompt,
    build_report,
    cohens_kappa,
    collect_responses,
    fleiss_kappa,
    overall_raw_agreement,
    raw_agreement_per_fact,
)


def test_reference_facts_nonempty_and_valid_operators():
    """Chaque fait de reference doit exister, avoir un id unique, et son
    expected_operator doit appartenir a la liste officielle des
    operateurs (sinon la definition de reference elle-meme serait
    incoherente avec le prompt envoye aux backends)."""
    from operator_agreement_study import OPERATOR_DEFINITIONS

    assert len(REFERENCE_FACTS) >= 15, "au moins 15 faits attendus (couverture des categories)"
    ids = [f.id for f in REFERENCE_FACTS]
    assert len(ids) == len(set(ids)), "les ids de faits doivent etre uniques"
    for fact in REFERENCE_FACTS:
        assert fact.expected_operator in OPERATOR_DEFINITIONS, (
            f"{fact.id}: expected_operator {fact.expected_operator!r} absent de OPERATOR_DEFINITIONS"
        )
    categories = {f.category for f in REFERENCE_FACTS}
    assert len(categories) >= 5, "les faits doivent couvrir plusieurs categories d'operateurs"


def test_two_identical_stubs_give_perfect_agreement():
    """Deux backends qui repondent TOUJOURS pareil (ici: l'operateur
    attendu de chaque fait) doivent produire 100% d'accord brut et un
    kappa de 1.0 (accord parfait, indistinguable du hasard uniquement
    dans le cas degenere ou toutes les reponses sont identiques -- geree
    explicitement dans cohens_kappa)."""
    stub_a = DeterministicStubBackend(name="stub_a")
    stub_b = DeterministicStubBackend(name="stub_b")

    results = collect_responses([stub_a, stub_b], REFERENCE_FACTS)

    assert overall_raw_agreement(results) == 1.0
    for fid, choices in results.items():
        assert raw_agreement_per_fact(choices) == 1.0, f"desaccord inattendu sur {fid}"

    kappa = cohens_kappa(results, "stub_a", "stub_b")
    assert kappa == 1.0, f"kappa attendu 1.0 pour un accord parfait, obtenu {kappa}"

    report = build_report(results, REFERENCE_FACTS)
    assert report.overall_raw_agreement == 1.0
    assert report.kappa_value == 1.0
    assert report.n_facts == len(REFERENCE_FACTS)
    assert report.n_backends == 2


def test_forced_disagreement_is_detected():
    """On force un backend a diverger sur exactement 3 faits connus
    (F01, F08, F14) en lui donnant un operateur different de
    expected_operator, et on verifie que:
      1. l'accord brut global chute strictement en dessous de 1.0
      2. l'accord par fait est exactement 0.0 sur les 3 faits forces et
         1.0 partout ailleurs (2 annotateurs -> une seule paire par fait,
         donc soit 0.0 soit 1.0, jamais une valeur intermediaire)
      3. le kappa chute strictement en dessous de 1.0
    Ceci prouve que le scoring reagit correctement a un desaccord connu
    d'avance, pas seulement qu'il retourne 1.0 par defaut."""
    forced_ids = {"F01", "F08", "F14"}
    # operateur deliberement different de expected_operator pour ces 3 faits
    diverging_answers = {
        "F01": "BELIEVES",   # expected: KNOWS
        "F08": "RESEMBLES",  # expected: EQUALS
        "F14": "ARR.PRODUCE",  # expected: ARR
    }
    stub_reference = DeterministicStubBackend(name="stub_reference")
    stub_divergent = DeterministicStubBackend(name="stub_divergent", answers=diverging_answers)

    results = collect_responses([stub_reference, stub_divergent], REFERENCE_FACTS)

    overall = overall_raw_agreement(results)
    assert 0.0 < overall < 1.0, f"accord global devrait etre strictement entre 0 et 1, obtenu {overall}"

    n_facts = len(REFERENCE_FACTS)
    expected_overall = (n_facts - len(forced_ids)) / n_facts
    assert abs(overall - expected_overall) < 1e-9, (
        f"accord global attendu {expected_overall} (({n_facts}-{len(forced_ids)})/{n_facts}), obtenu {overall}"
    )

    for fid, choices in results.items():
        agreement = raw_agreement_per_fact(choices)
        if fid in forced_ids:
            assert agreement == 0.0, f"{fid} devrait etre en desaccord total (force), obtenu {agreement}"
        else:
            assert agreement == 1.0, f"{fid} ne devrait pas etre affecte par le desaccord force, obtenu {agreement}"

    kappa = cohens_kappa(results, "stub_reference", "stub_divergent")
    assert kappa < 1.0, f"kappa devrait chuter sous 1.0 avec un desaccord force, obtenu {kappa}"

    report = build_report(results, REFERENCE_FACTS)
    top = report.most_controversial(top_n=3)
    top_ids = {fid for fid, _, _ in top}
    assert top_ids == forced_ids, (
        f"les 3 faits les plus controverses devraient etre exactement {forced_ids}, obtenu {top_ids}"
    )
    for _, _, agreement in top:
        assert agreement == 0.0


def test_fleiss_kappa_with_three_backends_perfect_agreement():
    """Avec 3 backends identiques, Fleiss' kappa doit valoir 1.0 (accord
    parfait). build_report doit automatiquement choisir Fleiss plutot que
    Cohen des que n_backends > 2."""
    stubs = [DeterministicStubBackend(name=f"stub_{i}") for i in range(3)]
    results = collect_responses(stubs, REFERENCE_FACTS)

    kappa = fleiss_kappa(results)
    assert kappa == 1.0

    report = build_report(results, REFERENCE_FACTS)
    assert report.n_backends == 3
    assert "Fleiss" in report.kappa_label
    assert report.kappa_value == 1.0


def test_fleiss_kappa_with_three_backends_partial_disagreement():
    """Un seul backend sur 3 diverge sur un fait -> accord partiel (ni 0
    ni 1) sur ce fait, et Fleiss' kappa global doit rester strictement
    inferieur a 1.0 mais rester une valeur numerique valide (pas NaN,
    pas de division par zero)."""
    stub_ref1 = DeterministicStubBackend(name="ref1")
    stub_ref2 = DeterministicStubBackend(name="ref2")
    stub_odd = DeterministicStubBackend(name="odd", answers={"F01": "DOUBTS"})  # expected: KNOWS

    results = collect_responses([stub_ref1, stub_ref2, stub_odd], REFERENCE_FACTS)

    # F01: deux votes KNOWS, un vote DOUBTS -> accord de paires = 1/3 (une seule paire d'accord sur 3)
    f01_agreement = raw_agreement_per_fact(results["F01"])
    assert abs(f01_agreement - (1 / 3)) < 1e-9

    kappa = fleiss_kappa(results)
    assert kappa < 1.0
    assert kappa == kappa  # NaN check (NaN != NaN)


def test_backend_names_must_be_unique_and_report_stable():
    """collect_responses indexe par backend.name -- deux backends du meme
    nom ecraseraient silencieusement une reponse. Ce test documente ce
    contrat plutot que de le corriger silencieusement (limite connue,
    pas un bug cache)."""
    stub_a = DeterministicStubBackend(name="dup")
    stub_b = DeterministicStubBackend(name="dup", fixed_operator="DOUBTS")
    results = collect_responses([stub_a, stub_b], REFERENCE_FACTS[:1])
    # le deuxieme backend (meme nom) ecrase le premier dans le dict -- un
    # seul nom present, avec la reponse du DERNIER backend fourni.
    assert list(results["F01"].keys()) == ["dup"]
    assert results["F01"]["dup"].operator == "DOUBTS"


def test_gemini_backend_from_env_degrades_cleanly_without_key(monkeypatch):
    """Meme contrat que AnthropicBackend.from_env(): pas d'exception, un
    simple None quand GEMINI_API_KEY est absente -- verifiable sans reseau
    ni cle, contrairement a un vrai appel a l'API Gemini."""
    monkeypatch.delenv("GEMINI_API_KEY", raising=False)
    assert GeminiBackend.from_env() is None


def test_gemini_backend_choose_operator_parses_real_client_shape():
    """Verifie la plomberie reelle de GeminiBackend.choose_operator (appel
    de la vraie forme d'API google-genai, `client.models.generate_content(...)
    .text`, puis parsing JSON) SANS reseau, via un faux client injecte a la
    place du vrai `genai.Client` -- seule partie testable ici sans
    GEMINI_API_KEY ni appel reseau reel (voir l'en-tete du module)."""

    class FakeResponse:
        text = '{"operator": "KNOWS", "justification": "fait etabli"}'

    class FakeModels:
        def generate_content(self, model, contents):  # noqa: ARG002
            assert model == "gemini-2.0-flash"
            assert isinstance(contents, str) and len(contents) > 0
            return FakeResponse()

    class FakeClient:
        models = FakeModels()

    backend = GeminiBackend(client=FakeClient(), model="gemini-2.0-flash")
    assert backend.name == "gemini:gemini-2.0-flash"

    choice = backend.choose_operator(REFERENCE_FACTS[0])
    assert choice.operator == "KNOWS"
    assert choice.justification == "fait etabli"


def test_build_prompt_is_identical_shape_for_every_backend():
    """Condition necessaire pour que l'accord mesure entre Anthropic et
    Gemini reflete une difference de jugement du modele, pas une difference
    de formulation du prompt : build_prompt ne depend d'aucun backend."""
    prompt1 = build_prompt(REFERENCE_FACTS[0])
    prompt2 = build_prompt(REFERENCE_FACTS[0])
    assert prompt1 == prompt2
    assert REFERENCE_FACTS[0].text in prompt1
    assert "JSON" in prompt1


if __name__ == "__main__":
    import pytest
    sys.exit(pytest.main([__file__, "-v"]))
