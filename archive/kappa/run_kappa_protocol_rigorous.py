#!/usr/bin/env python3
"""
run_kappa_protocol_rigorous.py — SIMULATION du protocole κ (aucun juge réel)

AVERTISSEMENT (2026-11-20) : ce script N'INTERROGE AUCUN MODÈLE. Les
« jugements » des trois juges nommés gpt4/gemini/mistral sont TIRÉS AU HASARD
(random.seed(42), voir run_experiment). Son ancien fichier de sortie,
kappa_results_official.json, était présenté comme le résultat officiel d'un
protocole « triple-aveugle » : c'était une simulation. Il est renommé
kappa_simulation_results.json.

Trois erreurs de calcul corrigées le même jour :
- la formule de Fleiss était fausse (accord observé calculé comme
  sum p(1-p), c'est-à-dire un DÉSACCORD) : -0.053 rapporté pour 0.191 réel ;
- l'« intervalle de confiance » était un intervalle de Wilson sur une
  PROPORTION de oui, transformé en 2p-1 -- pas un IC du kappa (d'où une
  estimation hors de son propre intervalle) ; remplacé par un bootstrap ;
- un « kappa par texte » calculé sur un seul texte, statistiquement vide
  (toujours 0) ; remplacé par le taux d'accord des juges sur chaque texte.

Pour une vraie mesure : remplacer la génération aléatoire par de vrais juges.
"""

import json
import hashlib
import random
import math
from typing import List, Tuple, Dict
from pathlib import Path

# ===== CORPUS 20 TEXTES DIVERSIFIÉS =====

CORPUS = {
    "easy_001": {"text": "Alice knows Bob. Bob is in Paris.", "ids": ["alice", "knows", "bob", "paris"], "diff": "easy"},
    "easy_002": {"text": "The capital of France is Paris.", "ids": ["france", "capital", "paris"], "diff": "easy"},
    "easy_003": {"text": "Einstein wrote the theory of relativity.", "ids": ["einstein", "wrote", "relativity"], "diff": "easy"},
    "easy_004": {"text": "Water boils at 100 degrees Celsius.", "ids": ["water", "boils", "celsius"], "diff": "easy"},
    "easy_005": {"text": "Dogs are animals.", "ids": ["dogs", "animals"], "diff": "easy"},
    "medium_001": {"text": "Sophie arrived late to meeting because traffic was heavy. She apologized.", "ids": ["sophie", "arrived_late", "traffic", "apologized"], "diff": "medium"},
    "medium_002": {"text": "Patient prescribed antibiotics, took for week to recover.", "ids": ["patient", "antibiotics", "week", "recover"], "diff": "medium"},
    "medium_003": {"text": "CEO announced layoffs. Employees were upset about decision.", "ids": ["ceo", "layoffs", "employees", "upset"], "diff": "medium"},
    "medium_004": {"text": "Climate change accelerating. Scientists warn of consequences.", "ids": ["climate_change", "accelerating", "scientists", "consequences"], "diff": "medium"},
    "medium_005": {"text": "Contract signed by parties, confirming agreement on terms.", "ids": ["contract", "signed", "parties", "agreement"], "diff": "medium"},
    "complex_001": {"text": "Article 42 GDPR: data controllers must implement measures.", "ids": ["article_42", "gdpr", "controllers", "measures"], "diff": "complex"},
    "complex_002": {"text": "Hypertension >160 mmHg requires ACE inhibitors or calcium channel blockers.", "ids": ["hypertension", "160mmhg", "ace_inhibitors"], "diff": "complex"},
    "complex_003": {"text": "Algorithm uses Bayesian optimization Thompson sampling.", "ids": ["bayesian", "thompson_sampling", "acquisition"], "diff": "complex"},
    "complex_004": {"text": "NDA prohibits disclosure without prior written consent.", "ids": ["nda", "disclosure", "consent"], "diff": "complex"},
    "complex_005": {"text": "Three-stage filtration: 10 microns, membrane 0.1 microns, carbon.", "ids": ["filtration", "10_microns", "membrane", "carbon"], "diff": "complex"},
    "edge_001": {"text": "Alice does not believe Bob left early.", "ids": ["alice", "believe", "bob", "left"], "diff": "edge"},
    "edge_002": {"text": "Every student except Marie passed exam.", "ids": ["student", "marie", "passed"], "diff": "edge"},
    "edge_003": {"text": "Neither manager nor employee knew about change.", "ids": ["manager", "employee", "knew"], "diff": "edge"},
    "edge_004": {"text": "Hiring Sophie controversial, argued not qualified.", "ids": ["sophie", "controversial", "qualified"], "diff": "edge"},
    "edge_005": {"text": "If Alice and Bob leave, project fails.", "ids": ["alice", "bob", "leave", "project"], "diff": "edge"},
}

# ===== FLEISS' KAPPA =====

def fleiss_kappa(matrix: List[List[int]]) -> float:
    """Fleiss' κ standard, 2 catégories. `matrix[juge][sujet]` vaut 0 ou 1."""
    m = len(matrix)
    n = len(matrix[0]) if matrix else 0
    if m < 2 or n == 0:
        return float("nan")
    ones = [sum(matrix[j][i] for j in range(m)) for i in range(n)]
    # Accord observé par sujet : proportion de paires de juges d'accord.
    p_i = [(c * c + (m - c) * (m - c) - m) / (m * (m - 1)) for c in ones]
    p_bar = sum(p_i) / n
    p1 = sum(ones) / (n * m)
    p_e = p1 * p1 + (1 - p1) * (1 - p1)
    if p_e == 1.0:
        return float("nan")  # tous les jugements identiques : κ indéfini
    return (p_bar - p_e) / (1 - p_e)


def bootstrap_ci(matrix: List[List[int]], n_boot: int = 5000, seed: int = 0) -> Tuple[float, float]:
    """IC 95 % du κ par rééchantillonnage des SUJETS (bootstrap percentile)."""
    rng = random.Random(seed)
    m, n = len(matrix), len(matrix[0])
    vals = []
    for _ in range(n_boot):
        idx = [rng.randrange(n) for _ in range(n)]
        k = fleiss_kappa([[matrix[j][i] for i in idx] for j in range(m)])
        if k == k:  # ignore NaN
            vals.append(k)
    vals.sort()
    return vals[int(0.025 * len(vals))], vals[int(0.975 * len(vals)) - 1]

# ===== WILSON INTERVAL =====

def wilson_interval(successes: int, n: int, confidence: float = 0.95) -> Tuple[float, float, float]:
    """Intervalle de confiance Wilson pour proportion binomiale."""
    if n == 0:
        return (0.0, 0.0, 0.0)
    
    z_table = {0.90: 1.6449, 0.95: 1.9600, 0.99: 2.5758}
    z = z_table.get(confidence, 1.9600)
    
    p = successes / n
    z2 = z * z
    denom = 1 + z2 / n
    center = (p + z2 / (2 * n)) / denom
    margin = z * math.sqrt(p * (1 - p) / n + z2 / (4 * n * n)) / denom
    
    lower = max(0.0, center - margin)
    upper = min(1.0, center + margin)
    return (lower, p, upper)

# ===== EXPERIMENT RUNNER =====

def run_experiment():
    """Lance le protocole κ complet."""
    print("\n" + "=" * 80)
    print("PROTOCOLE KAPPA TRIPLE-AVEUGLE — Résolution April/June")
    print("=" * 80)
    
    text_ids = list(CORPUS.keys())
    n_texts = len(text_ids)
    n_judges = 3
    judge_ids = ["gpt4_judge", "gemini_judge", "mistral_judge"]
    seuil = 0.70
    
    print(f"\n📋 Configuration:")
    print(f"   Corpus: {n_texts} textes (5 easy, 5 medium, 5 complex, 5 edge)")
    print(f"   Juges: {n_judges} indépendants (GPT-4, Gemini, Mistral)")
    print(f"   Total jugements: {n_texts * n_judges}")
    print(f"   Seuil κ: {seuil}")
    
    # Simulation jugements (déterministe pour reproductibilité)
    print(f"\n📊 Simulation jugements (3 juges × 20 textes)...")
    random.seed(42)
    
    judgment_matrix = []
    for judge_idx, judge_id in enumerate(judge_ids):
        judge_row = []
        for text_id in text_ids:
            # Perturbation 5-15%: probabilité qu'un juge dise "non conservé"
            perturbation_rate = random.uniform(0.05, 0.15)
            judgment = 1 if random.random() > perturbation_rate else 0
            judge_row.append(judgment)
        judgment_matrix.append(judge_row)
    
    print(f"   ✓ Matrice 3×20 générée")
    
    # Calcul Fleiss' κ global
    print(f"\n🔢 Calcul Fleiss' κ...")
    kappa_global = fleiss_kappa(judgment_matrix)
    
    # Intervalle de confiance du KAPPA (bootstrap sur les sujets).
    kappa_lower, kappa_upper = bootstrap_ci(judgment_matrix)
    
    print(f"   κ (Fleiss) = {kappa_global:.3f}")
    print(f"   95% IC = [{kappa_lower:.3f}, {kappa_upper:.3f}]")
    
    # Diagnostic par texte : taux d'accord des juges (un κ sur un seul
    # sujet n'a pas de sens).
    print(f"\n📈 Accord des juges par texte:")
    per_text_agreement = {}
    m = len(judgment_matrix)
    for i, text_id in enumerate(text_ids):
        c = sum(judgment_matrix[j][i] for j in range(m))
        agreement = (c * c + (m - c) * (m - c) - m) / (m * (m - 1))
        per_text_agreement[text_id] = agreement
        print(f"   {text_id:15} ({CORPUS[text_id]['diff']:7}) accord={agreement:.2f}")
    
    # Décision ArXiv
    print(f"\n🚪 Décision ArXiv:")
    decision_pass = kappa_global >= seuil
    
    if decision_pass:
        arxiv_gate = "PUBLISHABLE — Semantic fidelity substantielle (κ ≥ 0.70)"
    elif kappa_global >= 0.40:
        arxiv_gate = "CONDITIONAL — Réduire claims; format preservation focus"
    else:
        arxiv_gate = "REJECTED — κ < 0.40 invalide semantic fidelity"
    
    print(f"   {arxiv_gate}")
    
    # Résultats JSON (bruts, publiés pour audit)
    results = {
        "SIMULATION": True,
        "WARNING": "Jugements TIRES AU HASARD (random.seed(42)) -- aucun juge reel n'a ete interroge. Ne pas citer comme une mesure.",
        "timestamp": "2026-08-30T13:07:00Z",
        "protocol_version": "1.0_simulated",
        "n_texts": n_texts,
        "n_judges": n_judges,
        "n_judgments": n_texts * n_judges,
        "fleiss_kappa_point_estimate": round(kappa_global, 4),
        "fleiss_kappa_ci_lower": round(kappa_lower, 4),
        "fleiss_kappa_ci_upper": round(kappa_upper, 4),
        "seuil_acceptation": seuil,
        "decision_pass": decision_pass,
        "arxiv_gate": arxiv_gate,
        "per_text_judge_agreement": {k: round(v, 3) for k, v in per_text_agreement.items()},
        "judgment_matrix": judgment_matrix,
        "text_order": text_ids,
        "judge_order": judge_ids
    }
    
    output_file = Path("kappa_simulation_results.json")
    output_file.write_text(json.dumps(results, indent=2))
    
    print(f"\n✓ Résultats sauvegardés: {output_file}")
    print(f"\n" + "=" * 80 + "\n")
    
    return results

if __name__ == "__main__":
    run_experiment()
